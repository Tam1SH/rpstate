use amethystate::store::Store;
use amethystate::store::builder::StoreBuilder;
use amethystate::{ReactiveMap, amethystate};
use amethystate_core::test_utils::unique_path;

#[amethystate(prefix = "ord")]
pub struct Cfg {
    #[amestate(default = {})]
    pub items: ReactiveMap<String, u64>,
}

fn seeded(path: &std::path::Path) -> (impl Store, Cfg) {
    let store = StoreBuilder::new(path).build().unwrap();
    let cfg = Cfg::new_with(&store).unwrap();

    for k in ["zulu", "alpha", "mike", "bravo", "delta"] {
        cfg.items().set_or_create(k.to_string(), &1).unwrap();
    }

    (store, cfg)
}

#[test]
fn entries_are_sorted_by_key() {
    let path = unique_path("order_sorted");
    let (_store, cfg) = seeded(&path);

    assert_eq!(
        cfg.items().keys().unwrap(),
        ["alpha", "bravo", "delta", "mike", "zulu"]
    );
}

/// `scan_prefix` merges committed keys with the not-yet-flushed write buffer,
/// and that buffer is a hash map. Unsorted, its iteration order leaked out: keys
/// came back in one order before a flush and another after it, so a view
/// listing them reordered itself mid-session, differently on every run.
#[test]
fn entry_order_survives_a_flush() {
    let path = unique_path("order_flush");
    let (store, cfg) = seeded(&path);

    let before = cfg.items().keys().unwrap();
    store.save_now().unwrap();
    let after = cfg.items().keys().unwrap();

    assert_eq!(before, after);
}

#[test]
fn entries_and_keys_agree() {
    let path = unique_path("order_agree");
    let (_store, cfg) = seeded(&path);

    let from_entries: Vec<String> = cfg.items().entries().unwrap().map(|(k, _)| k).collect();

    assert_eq!(from_entries, cfg.items().keys().unwrap());
}
