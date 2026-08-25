use amethystate::store::builder::StoreBuilder;
use amethystate::{LocalScope, MapChange, ReactiveMap, amethystate};
use amethystate_core::test_utils::unique_path;
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

#[amethystate(prefix = "w")]
pub struct Cfg {
    #[amestate(default = 0u64)]
    pub counter: u64,

    #[amestate(default = {})]
    pub items: ReactiveMap<String, u64>,
}

fn cfg() -> (impl amethystate::store::StoreBackend, Cfg) {
    let path = unique_path("watch_builder");
    let store = StoreBuilder::new(&path).build().unwrap();
    let cfg = Cfg::new_with(&store).unwrap();
    (store, cfg)
}

#[test]
fn immediate_register_behaves_like_subscribe() {
    let (_s, cfg) = cfg();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let cap = seen.clone();

    let _sub = cfg.counter().subscription_with().register(move |v: &u64| {
        cap.lock().unwrap().push(*v);
    });

    cfg.counter().set(1).unwrap();
    cfg.counter().set(2).unwrap();

    assert_eq!(*seen.lock().unwrap(), vec![1, 2]);
}

/// `external` and `with_source` could not be combined before: each pair of
/// axes was its own method, and this pair had none.
#[test]
fn external_and_with_source_compose() {
    let (_s, cfg) = cfg();
    let field = cfg.counter();
    let other = field.fork();

    let seen = Arc::new(Mutex::new(Vec::new()));
    let cap = seen.clone();

    let _sub = field
        .subscription_with()
        .external()
        .register_with_source(move |v: &u64, src| {
            cap.lock().unwrap().push((*v, src.is_some()));
        });

    field.set(1).unwrap();
    other.set(2).unwrap();

    assert_eq!(
        *seen.lock().unwrap(),
        vec![(2, true)],
        "own write filtered, the fork's arrives with its provenance"
    );
}

#[test]
fn local_callback_need_not_be_send() {
    let (_s, cfg) = cfg();
    let mut ui = LocalScope::new();

    let seen = Rc::new(RefCell::new(Vec::new()));
    let cap = Rc::clone(&seen);

    cfg.counter()
        .subscription_with()
        .local(&mut ui)
        .register(move |v: &u64| cap.borrow_mut().push(*v));

    cfg.counter().set(5).unwrap();
    assert!(seen.borrow().is_empty(), "nothing runs before a drain");

    ui.drain();
    assert_eq!(*seen.borrow(), vec![5]);
}

#[test]
fn local_coalesces_by_default() {
    let (_s, cfg) = cfg();
    let mut ui = LocalScope::new();

    let seen = Rc::new(RefCell::new(Vec::new()));
    let cap = Rc::clone(&seen);

    cfg.counter()
        .subscription_with()
        .local(&mut ui)
        .register(move |v: &u64| cap.borrow_mut().push(*v));

    for n in 1..=4 {
        cfg.counter().set(n).unwrap();
    }
    ui.drain();

    assert_eq!(*seen.borrow(), vec![4]);
}

#[test]
fn every_keeps_the_intermediate_changes() {
    let (_s, cfg) = cfg();
    let mut ui = LocalScope::new();

    let seen = Rc::new(RefCell::new(Vec::new()));
    let cap = Rc::clone(&seen);

    cfg.counter()
        .subscription_with()
        .local(&mut ui)
        .every()
        .register(move |v: &u64| cap.borrow_mut().push(*v));

    for n in 1..=4 {
        cfg.counter().set(n).unwrap();
    }
    ui.drain();

    assert_eq!(*seen.borrow(), vec![1, 2, 3, 4]);
}

#[test]
fn map_changes_reach_a_local_subscriber() {
    let (_s, cfg) = cfg();
    let mut ui = LocalScope::new();

    let seen = Rc::new(RefCell::new(Vec::new()));
    let cap = Rc::clone(&seen);

    cfg.items()
        .subscription_with()
        .local(&mut ui)
        .every()
        .register(move |change: &MapChange<String, u64>| {
            cap.borrow_mut().push(match change {
                MapChange::Insert { .. } => "insert",
                MapChange::Update { .. } => "update",
                MapChange::Remove { .. } => "remove",
                MapChange::Clear { .. } => "clear",
            });
        });

    cfg.items().insert("a".into(), &1).unwrap();
    cfg.items().update("a".into(), &2).unwrap();
    cfg.items().remove("a").unwrap();
    ui.drain();

    assert_eq!(*seen.borrow(), vec!["insert", "update", "remove"]);
}

#[test]
fn a_single_key_can_be_watched() {
    let (_s, cfg) = cfg();
    let seen = Arc::new(Mutex::new(0usize));
    let cap = seen.clone();

    let _sub = cfg
        .items()
        .subscription_with()
        .key("watched".to_string())
        .register(move |_: &MapChange<String, u64>| {
            *cap.lock().unwrap() += 1;
        });

    cfg.items().insert("watched".into(), &1).unwrap();
    cfg.items().insert("ignored".into(), &1).unwrap();

    assert_eq!(*seen.lock().unwrap(), 1);
}

/// `external` on a map keeps the rule the flat methods had: only `Update` is
/// filtered, because a key appearing or disappearing changes what the map holds
/// and goes to everyone.
#[test]
fn external_on_a_map_filters_updates_only() {
    let (_s, cfg) = cfg();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let cap = seen.clone();

    let _sub = cfg.items().subscription_with().external().register(
        move |change: &MapChange<String, u64>| {
            cap.lock().unwrap().push(match change {
                MapChange::Insert { .. } => "insert",
                MapChange::Update { .. } => "update",
                MapChange::Remove { .. } => "remove",
                MapChange::Clear { .. } => "clear",
            });
        },
    );

    cfg.items().insert("a".into(), &1).unwrap();
    cfg.items().update("a".into(), &2).unwrap();
    cfg.items().remove("a").unwrap();

    assert_eq!(*seen.lock().unwrap(), vec!["insert", "remove"]);
}

/// A field has no such carve-out: every one of its own writes is filtered.
#[test]
fn external_on_a_field_filters_everything_of_its_own() {
    let (_s, cfg) = cfg();
    let seen = Arc::new(Mutex::new(0usize));
    let cap = seen.clone();

    let _sub = cfg
        .counter()
        .subscription_with()
        .external()
        .register(move |_: &u64| *cap.lock().unwrap() += 1);

    cfg.counter().set(1).unwrap();
    cfg.counter().set(2).unwrap();

    assert_eq!(*seen.lock().unwrap(), 0);
}

mod stream {
    use super::*;
    use futures::StreamExt;
    use futures::executor::block_on;

    #[test]
    fn yields_each_change_in_order() {
        let (_s, cfg) = cfg();
        let mut changes = cfg.counter().subscription_with().stream();

        for n in 1..=3 {
            cfg.counter().set(n).unwrap();
        }

        let got: Vec<u64> = block_on(async { changes.by_ref().take(3).collect().await });
        assert_eq!(got, vec![1, 2, 3]);
    }

    #[test]
    fn a_write_from_another_thread_arrives() {
        let (_s, cfg) = cfg();
        let mut changes = cfg.counter().subscription_with().stream();

        let writer = cfg.counter();
        let handle = std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(20));
            writer.set(99).unwrap();
        });

        let got = block_on(changes.next());
        handle.join().unwrap();

        assert_eq!(got, Some(99));
    }

    #[test]
    fn external_still_filters() {
        let (_s, cfg) = cfg();
        let field = cfg.counter();
        let other = field.fork();

        let mut changes = field.subscription_with().external().stream();

        field.set(1).unwrap();
        other.set(2).unwrap();

        assert_eq!(block_on(changes.next()), Some(2));
    }

    /// A stream yields what arrives after it exists, and nothing a stream
    /// before it queued. That the drop also released the subscription is not
    /// shown here and cannot be from outside: a dropped stream is not
    /// observable, and nothing exposes how many subscribers a signal has.
    #[test]
    fn a_stream_starts_empty_and_does_not_inherit_a_dropped_one() {
        let (_s, cfg) = cfg();
        let counter = cfg.counter();

        let changes = counter.subscription_with().stream();
        counter.set(1).unwrap();
        drop(changes);

        let mut fresh = counter.subscription_with().stream();
        counter.set(2).unwrap();

        assert_eq!(
            block_on(fresh.next()),
            Some(2),
            "the dropped stream's queue must not resurface"
        );
    }

    #[test]
    fn map_changes_stream_too() {
        let (_s, cfg) = cfg();
        let mut changes = cfg.items().subscription_with().stream();

        cfg.items().insert("a".into(), &1).unwrap();

        let got = block_on(changes.next()).unwrap();
        assert!(matches!(got, MapChange::Insert { .. }));
    }
}
