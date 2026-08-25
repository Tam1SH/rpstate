use amethystate::store::builder::StoreBuilder;
use amethystate::{MapChange, ReactiveMap, amethystate};
use amethystate_core::test_utils::unique_path;
use std::sync::{Arc, Mutex};

#[amethystate(prefix = "probe")]
pub struct Cfg {
    #[amestate(default = {"a": 1u64, "b": 2u64})]
    pub items: ReactiveMap<String, u64>,
}

fn label<K, V>(change: &MapChange<K, V>) -> &'static str {
    match change {
        MapChange::Insert { .. } => "insert",
        MapChange::Update { .. } => "update",
        MapChange::Remove { .. } => "remove",
        MapChange::Clear { .. } => "clear",
    }
}

fn watch<K, V>(
    map: &ReactiveMap<K, V, amethystate::WritableMode>,
) -> (
    Arc<Mutex<Vec<&'static str>>>,
    amethystate::SignalSubscription,
)
where
    K: amethystate::ReactiveMapKey,
    V: amethystate::ReactiveMapValue,
{
    let seen = Arc::new(Mutex::new(Vec::new()));
    let cap = seen.clone();
    let sub = map.subscribe_any(move |change| cap.lock().unwrap().push(label(change)));
    (seen, sub)
}

#[test]
fn clear_reports_once_to_every_handle() {
    let path = unique_path("clear_one_event");
    let store = StoreBuilder::new(&path).build().unwrap();

    let a = Cfg::new_with(&store).unwrap();
    let b = Cfg::new_with(&store).unwrap();

    let (seen_a, _sa) = watch(&a.items());
    let (seen_b, _sb) = watch(&b.items());

    a.items().clear().unwrap();

    assert_eq!(*seen_a.lock().unwrap(), vec!["clear"]);
    assert_eq!(*seen_b.lock().unwrap(), vec!["clear"]);
}

#[test]
fn clear_empties_every_handle() {
    let path = unique_path("clear_empties");
    let store = StoreBuilder::new(&path).build().unwrap();

    let a = Cfg::new_with(&store).unwrap();
    let b = Cfg::new_with(&store).unwrap();

    a.items().clear().unwrap();

    assert_eq!(a.items().len(), 0);
    assert_eq!(b.items().len(), 0);
    assert_eq!(a.items().get("a"), None);
    assert_eq!(b.items().get("a"), None);
}

#[test]
fn clear_survives_a_store_rebuild() {
    let path = unique_path("clear_persists");

    {
        let store = StoreBuilder::new(&path).build().unwrap();
        let cfg = Cfg::new_with(&store).unwrap();
        cfg.items().clear().unwrap();
        store.save_now().unwrap();
    }

    {
        let store = StoreBuilder::new(&path).build().unwrap();
        let cfg = Cfg::new_with(&store).unwrap();
        assert_eq!(cfg.items().len(), 0);
    }
}

#[test]
fn clear_carries_the_provenance_of_the_handle_that_issued_it() {
    let path = unique_path("clear_source");
    let store = StoreBuilder::new(&path).build().unwrap();
    let cfg = Cfg::new_with(&store).unwrap();

    let items = cfg.items();
    let observer = items.fork();

    let seen = Arc::new(Mutex::new(Vec::new()));
    let cap = seen.clone();
    let _sub = observer.subscribe_any(move |change| {
        cap.lock().unwrap().push(change.source());
    });

    items.clear().unwrap();

    assert_eq!(*seen.lock().unwrap(), vec![Some(items.instance_id())]);
}
