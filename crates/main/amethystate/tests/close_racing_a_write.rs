use amethystate::store::builder::StoreBuilder;
use amethystate_core::test_utils::TempPath;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;

const ROUNDS: u32 = 24;
const WRITERS: u32 = 4;
const EACH: u32 = 50;

#[test]
fn a_write_that_was_accepted_survives_a_close_racing_it() {
    for round in 0..ROUNDS {
        let path = TempPath::new("close_race");
        let store = StoreBuilder::new(path.path()).build().unwrap();

        let go = Arc::new(AtomicBool::new(false));

        let writers: Vec<_> = (0..WRITERS)
            .map(|w| {
                let store = store.clone();
                let go = go.clone();
                thread::spawn(move || {
                    while !go.load(Ordering::Acquire) {
                        std::hint::spin_loop();
                    }

                    let mut accepted = Vec::new();
                    for i in 0..EACH {
                        let key = format!("k{w}_{i}");
                        if store.set(["race", key.as_str()], &i).is_ok() {
                            accepted.push((key, i));
                        }
                    }
                    accepted
                })
            })
            .collect();

        let closer = {
            let store = store.clone();
            let go = go.clone();
            thread::spawn(move || {
                while !go.load(Ordering::Acquire) {
                    std::hint::spin_loop();
                }
                for _ in 0..(round % 8) * 200 {
                    std::hint::spin_loop();
                }
                store.close()
            })
        };

        go.store(true, Ordering::Release);

        let accepted: Vec<(String, u32)> = writers
            .into_iter()
            .flat_map(|w| w.join().expect("a writer panicked"))
            .collect();
        closer.join().expect("the closer panicked").unwrap();
        drop(store);

        let reopened = StoreBuilder::new(path.path()).build().unwrap();
        for (key, value) in &accepted {
            assert_eq!(
                reopened.get::<u32>(["race", key.as_str()]).unwrap(),
                Some(*value),
                "round {round}: {key} was accepted and is not on disk"
            );
        }
    }
}
