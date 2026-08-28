//! Whether two stores compose at the reactive level.
//!
//! The question behind it: splitting state across files - one per debounce
//! policy, or just one per concern - is only an option if handles from
//! different stores still combine. The reactive layer is built on `Signal` and
//! `ReactiveMapCore`, which know nothing about storage, so it ought to; this
//! measures it rather than reasoning about it.

use amethystate::store::builder::StoreBuilder;
use amethystate::{IntoPipeline, amethystate};
use amethystate_core::test_utils::TempPath;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

#[amethystate(prefix = "fast")]
pub struct Fast {
    #[amestate(default = 0)]
    pub ticks: u32,
}

#[amethystate(prefix = "slow")]
pub struct Slow {
    #[amestate(default = "idle".to_string())]
    pub phase: String,
}

/// A pipeline whose two sources live in different files, and therefore under
/// different debouncers, different flush policies and different schemas.
#[test]
fn a_pipeline_spans_two_stores() {
    let fast_path = TempPath::new("two_stores_fast");
    let slow_path = TempPath::new("two_stores_slow");

    let fast_store = StoreBuilder::new(fast_path.path()).build().unwrap();
    let slow_store = StoreBuilder::new(slow_path.path()).build().unwrap();

    let fast = Fast::new_with(&fast_store).unwrap();
    let slow = Slow::new_with(&slow_store).unwrap();

    let line = (fast.ticks(), slow.phase())
        .pipe()
        .map(|(ticks, phase)| format!("{phase}:{ticks}"));

    assert_eq!(line.get(), "idle:0");

    let seen = Arc::new(Mutex::new(Vec::new()));
    let seen_in = seen.clone();
    let _sub = line.subscribe(move |v| seen_in.lock().unwrap().push(v.clone()));

    fast.ticks().set(1).unwrap();
    assert_eq!(line.get(), "idle:1", "a write in one file moved the pipeline");

    slow.phase().set("busy".to_string()).unwrap();
    assert_eq!(
        line.get(),
        "busy:1",
        "a write in the other file moved it too"
    );

    assert_eq!(
        *seen.lock().unwrap(),
        vec!["idle:1".to_string(), "busy:1".to_string()],
        "both stores' writes reached the subscriber, in order"
    );
}

/// Dropping one store must not silence a pipeline that also reads the other -
/// and must not silence the half that is still alive.
#[test]
fn dropping_one_store_leaves_the_other_half_alive() {
    let fast_path = TempPath::new("two_stores_drop_fast");
    let slow_path = TempPath::new("two_stores_drop_slow");

    let fast_store = StoreBuilder::new(fast_path.path()).build().unwrap();
    let fast = Fast::new_with(&fast_store).unwrap();
    let ticks = fast.ticks();

    let line = {
        let slow_store = StoreBuilder::new(slow_path.path()).build().unwrap();
        let slow = Slow::new_with(&slow_store).unwrap();
        let line = (ticks.clone(), slow.phase())
            .pipe()
            .map(|(ticks, phase)| format!("{phase}:{ticks}"));
        slow.phase().set("busy".to_string()).unwrap();
        line
    };

    let hits = Arc::new(AtomicUsize::new(0));
    let hits_in = hits.clone();
    let _sub = line.subscribe(move |_| {
        hits_in.fetch_add(1, Ordering::SeqCst);
    });

    ticks.set(7).unwrap();

    assert_eq!(
        hits.load(Ordering::SeqCst),
        1,
        "the surviving store's writes still reach the pipeline"
    );
    assert_eq!(line.get(), "busy:7");
}

/// Composing is one half; not leaking into each other is the other. A
/// subscriber on one store must not hear the other store's writes, however
/// alike the paths under them look.
#[test]
fn subscriptions_from_two_stores_are_independent() {
    let fast_path = TempPath::new("two_stores_indep_fast");
    let slow_path = TempPath::new("two_stores_indep_slow");

    let fast_store = StoreBuilder::new(fast_path.path()).build().unwrap();
    let slow_store = StoreBuilder::new(slow_path.path()).build().unwrap();

    let fast = Fast::new_with(&fast_store).unwrap();
    let slow = Slow::new_with(&slow_store).unwrap();

    let fast_hits = Arc::new(AtomicUsize::new(0));
    let slow_hits = Arc::new(AtomicUsize::new(0));

    let f = fast_hits.clone();
    let _a = fast.ticks().subscribe(move |_| {
        f.fetch_add(1, Ordering::SeqCst);
    });
    let s = slow_hits.clone();
    let _b = slow.phase().subscribe(move |_| {
        s.fetch_add(1, Ordering::SeqCst);
    });

    fast.ticks().set(1).unwrap();
    fast.ticks().set(2).unwrap();
    slow.phase().set("busy".to_string()).unwrap();

    assert_eq!(fast_hits.load(Ordering::SeqCst), 2);
    assert_eq!(
        slow_hits.load(Ordering::SeqCst),
        1,
        "a subscriber in one store did not hear the other store's writes"
    );
}
