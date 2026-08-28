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
    assert_eq!(
        line.get(),
        "idle:1",
        "a write in one file moved the pipeline"
    );

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

/// A pipeline keeps every source it was built from, so one whose store the
/// caller no longer holds is still read on the next recompute.
///
/// Nothing is dropped here, and the name used to say otherwise: the tuple
/// `pipe` clones each source into its `refresh` closure and collects a
/// keepalive besides, so the slow store outlives the block by construction and
/// only the local binding goes. That is the behaviour worth pinning, and the
/// assertion reaches it because a change on *any* source re-runs `refresh`,
/// which calls `get` on *every* source - so `busy` in the result is the slow
/// half being read after its binding is gone, not a cached string.
#[test]
fn a_pipeline_reads_a_source_whose_store_the_caller_dropped() {
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

/// The other half of the same claim, and the one the test above cannot make:
/// the store whose binding went out of scope is not merely readable, it is
/// still *live* - a write to it reaches the pipeline like any other.
///
/// The handle is kept and the store's own binding is not, which is how an
/// application ends up here: a state struct built in a setup function, its
/// fields handed out, the `Store` never held again.
#[test]
fn a_write_to_the_dropped_store_still_reaches_the_pipeline() {
    let fast_path = TempPath::new("two_stores_live_fast");
    let slow_path = TempPath::new("two_stores_live_slow");

    let fast_store = StoreBuilder::new(fast_path.path()).build().unwrap();
    let fast = Fast::new_with(&fast_store).unwrap();
    let ticks = fast.ticks();

    let (line, phase) = {
        let slow_store = StoreBuilder::new(slow_path.path()).build().unwrap();
        let slow = Slow::new_with(&slow_store).unwrap();
        let phase = slow.phase();
        let line = (ticks.clone(), phase.clone())
            .pipe()
            .map(|(ticks, phase)| format!("{phase}:{ticks}"));
        (line, phase)
    };

    let seen = Arc::new(Mutex::new(Vec::new()));
    let seen_in = seen.clone();
    let _sub = line.subscribe(move |v| seen_in.lock().unwrap().push(v.clone()));

    phase.set("busy".to_string()).unwrap();
    ticks.set(3).unwrap();

    assert_eq!(
        *seen.lock().unwrap(),
        vec!["busy:0".to_string(), "busy:3".to_string()],
        "a write to the store nobody holds any more was not delivered"
    );
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
