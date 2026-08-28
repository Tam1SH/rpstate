use amethystate::store::builder::StoreBuilder;
use amethystate::{ReactiveMap, amethystate};
use amethystate_core::test_utils::unique_path;
use std::sync::atomic::Ordering;
use std::sync::mpsc;
use std::time::Duration;

#[amethystate(prefix = "re")]
pub struct Cfg {
    #[amestate(default = 0u64)]
    pub counter: u64,

    #[amestate(default = {})]
    pub items: ReactiveMap<String, u64>,
}

fn within<F>(what: &str, body: F)
where
    F: FnOnce() + Send + 'static,
{
    let (tx, rx) = mpsc::channel();

    std::thread::spawn(move || {
        body();
        let _ = tx.send(());
    });

    if rx.recv_timeout(Duration::from_secs(5)).is_err() {
        panic!("{what} deadlocked");
    }
}

/// Reacting to a change by writing is ordinary, and the subscriber lists are
/// behind non-reentrant mutexes, so notifying while holding them hangs the
/// thread outright.
#[test]
fn a_map_subscriber_may_write_to_the_map_it_watches() {
    within("map subscriber writing to its own map", || {
        let path = unique_path("reentrancy_map");
        let store = StoreBuilder::new(&path).build().unwrap();
        let cfg = Cfg::new_with(&store).unwrap();
        let items = cfg.items();

        let writer = items.clone();
        let _sub = items.subscribe_any(move |change| {
            if change.key().map(String::as_str) == Some("mirror") {
                return;
            }
            let _ = writer.insert("mirror".to_string(), &99);
        });

        items.insert("a".to_string(), &1).unwrap();
    });
}

#[test]
fn a_keyed_subscriber_may_write_to_the_map_it_watches() {
    within("keyed subscriber writing to its own map", || {
        let path = unique_path("reentrancy_key");
        let store = StoreBuilder::new(&path).build().unwrap();
        let cfg = Cfg::new_with(&store).unwrap();
        let items = cfg.items();

        let writer = items.clone();
        let _sub = items.subscribe_key("a".to_string(), move |_| {
            let _ = writer.insert("mirror".to_string(), &99);
        });

        items.insert("a".to_string(), &1).unwrap();
    });
}

#[test]
fn a_subscriber_may_add_another_subscription_while_being_notified() {
    within("subscriber subscribing during a notification", || {
        let path = unique_path("reentrancy_sub");
        let store = StoreBuilder::new(&path).build().unwrap();
        let cfg = Cfg::new_with(&store).unwrap();
        let items = cfg.items();

        let nested = items.clone();
        let _sub = items.subscribe_any(move |_| {
            let extra = nested.subscribe_any(|_| {});
            drop(extra);
        });

        items.insert("a".to_string(), &1).unwrap();
    });
}

/// A panicking callback used to poison the subscriber lists, after which every
/// later subscribe panicked and every later notify silently delivered nothing.
#[test]
fn a_panicking_subscriber_does_not_disable_the_map() {
    let path = unique_path("reentrancy_panic");
    let store = StoreBuilder::new(&path).build().unwrap();
    let cfg = Cfg::new_with(&store).unwrap();
    let items = cfg.items();

    let boom = items.subscribe_any(|_| panic!("subscriber blew up"));

    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _ = items.insert("a".to_string(), &1);
    }));
    std::panic::set_hook(previous);
    assert!(
        result.is_err(),
        "the panic should surface, not be swallowed"
    );
    drop(boom);

    let seen = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let cap = seen.clone();
    let _sub = items.subscribe_any(move |_| {
        cap.fetch_add(1, Ordering::SeqCst);
    });

    items.insert("b".to_string(), &2).unwrap();

    assert_eq!(
        seen.load(Ordering::SeqCst),
        1,
        "notifications must survive an earlier panicking subscriber"
    );
}
