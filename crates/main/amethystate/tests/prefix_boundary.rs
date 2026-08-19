use amethystate::StoreBackend;
use amethystate::store::builder::StoreBuilder;
use amethystate_core::test_utils::TempPath;

/// `scan_keys` is documented as "the keys under `prefix`", but the flat
/// backends match a raw string prefix, so a sibling whose name merely starts
/// with it is included.
#[test]
#[ignore = "known: scan_prefix matches a string prefix, not a segment boundary - see TODO.md"]
fn scanning_a_prefix_stops_at_a_segment_boundary() {
    let path = TempPath::new("prefix_scan");
    let store = StoreBuilder::new(path.path()).build().unwrap();
    let kv = store.kv();

    kv.namespace("ui").unwrap().set("width", &1280u32).unwrap();
    kv.namespace("uix").unwrap().set("width", &640u32).unwrap();

    assert_eq!(
        kv.namespace("ui").unwrap().keys().unwrap(),
        ["ui.width"],
        "uix is a different subtree"
    );
}

/// The same mismatch reached through a delete: removing everything under
/// `ui` also removes the unrelated `uix` subtree.
#[test]
#[ignore = "known: delete_prefix matches a string prefix, not a segment boundary - see TODO.md"]
fn deleting_a_prefix_leaves_a_sibling_with_a_shared_leading_string() {
    let path = TempPath::new("prefix_delete");
    let store = StoreBuilder::new(path.path()).build().unwrap();
    let kv = store.kv();

    kv.namespace("ui").unwrap().set("width", &1280u32).unwrap();
    kv.namespace("uix").unwrap().set("width", &640u32).unwrap();

    store.delete_prefix("ui").unwrap();

    assert_eq!(
        kv.namespace("ui").unwrap().get::<u32>("width").unwrap(),
        None
    );
    assert_eq!(
        kv.namespace("uix").unwrap().get::<u32>("width").unwrap(),
        Some(640),
        "a sibling subtree survives"
    );
}

/// Control: with the separator spelled out the boundary is respected, so the
/// cause is the missing boundary and not the delete itself.
#[test]
fn deleting_a_dotted_prefix_leaves_the_sibling_alone() {
    let path = TempPath::new("prefix_delete_dot");
    let store = StoreBuilder::new(path.path()).build().unwrap();
    let kv = store.kv();

    kv.namespace("ui").unwrap().set("width", &1280u32).unwrap();
    kv.namespace("uix").unwrap().set("width", &640u32).unwrap();

    store.delete_prefix("ui.").unwrap();

    assert_eq!(
        kv.namespace("ui").unwrap().get::<u32>("width").unwrap(),
        None
    );
    assert_eq!(
        kv.namespace("uix").unwrap().get::<u32>("width").unwrap(),
        Some(640)
    );
}

/// A path segment containing GLOB metacharacters: sqlite builds its scan
/// pattern with `format!("{}*", prefix)` and runs it through `GLOB`, so
/// `[`, `?` and `*` in a path are read as wildcards.
#[test]
#[ignore = "known: sqlite scan_prefix passes the prefix to GLOB unescaped - see TODO.md"]
fn a_path_segment_with_glob_metacharacters_scans_as_a_literal() {
    let path = TempPath::new("prefix_glob");
    let store = StoreBuilder::new(path.path()).build().unwrap();
    let kv = store.kv();

    kv.namespace("cfg[a]").unwrap().set("width", &1u32).unwrap();
    kv.namespace("cfg[a]")
        .unwrap()
        .set("height", &2u32)
        .unwrap();
    store.flush_prefix("").unwrap();

    assert_eq!(
        kv.namespace("cfg[a]").unwrap().get::<u32>("width").unwrap(),
        Some(1)
    );

    let mut keys = kv.namespace("cfg[a]").unwrap().keys().unwrap();
    keys.sort();
    assert_eq!(
        keys,
        ["cfg[a].height", "cfg[a].width"],
        "the bracket is part of the path, not a character class"
    );
}

/// The same map operations over a path holding a `[`: `len`, `keys` and
/// `clear` all ride `scan_prefix`.
#[test]
#[ignore = "known: sqlite scan_prefix passes the prefix to GLOB unescaped - see TODO.md"]
fn a_map_at_a_path_with_glob_metacharacters_reports_its_entries() {
    let path = TempPath::new("map_glob");
    let store = StoreBuilder::new(path.path()).build().unwrap();
    let map = store
        .kv()
        .namespace("panel[0]")
        .unwrap()
        .map::<String, u32>("widths")
        .unwrap();

    map.insert("cpu".into(), &120).unwrap();
    map.insert("mem".into(), &80).unwrap();
    store.flush_prefix("").unwrap();

    assert_eq!(map.get(&"cpu".to_string()).unwrap(), Some(120));
    assert_eq!(map.len().unwrap(), 2, "both entries are visible");

    map.clear().unwrap();
    store.flush_prefix("").unwrap();
    assert_eq!(
        store.get::<u32>("panel[0].widths.cpu").unwrap(),
        None,
        "clear removed the entries"
    );
}

/// Control: the identical map with no metacharacter in its path.
#[test]
fn a_map_at_a_plain_path_reports_its_entries() {
    let path = TempPath::new("map_plain");
    let store = StoreBuilder::new(path.path()).build().unwrap();
    let map = store
        .kv()
        .namespace("panel0")
        .unwrap()
        .map::<String, u32>("widths")
        .unwrap();

    map.insert("cpu".into(), &120).unwrap();
    map.insert("mem".into(), &80).unwrap();
    store.flush_prefix("").unwrap();

    assert_eq!(map.len().unwrap(), 2);

    map.clear().unwrap();
    store.flush_prefix("").unwrap();
    assert_eq!(store.get::<u32>("panel0.widths.cpu").unwrap(), None);
}
