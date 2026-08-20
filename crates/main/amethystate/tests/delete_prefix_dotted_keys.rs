use amethystate::store::builder::StoreBuilder;
use amethystate::store::reactive_map_with_path_only;
use amethystate_core::access::WritableMode;
use amethystate_core::test_utils::TempPath;
use std::collections::HashMap;
use uuid::Uuid;

/// Deleting a subtree has to take the entries whose names hold the separator
/// with it. The tree engines re-read the scanned key by splitting it on the
/// separator, which turns such a name back into levels and addresses a node
/// that is not there.
#[test]
fn deleting_a_subtree_takes_the_dotted_names_with_it() {
    let path = TempPath::new("delete_prefix_dotted");
    let store = StoreBuilder::new(path.path()).build().unwrap();
    let map = reactive_map_with_path_only::<String, u32, WritableMode>(
        &store,
        ["dotted", "items"],
        HashMap::new(),
        Uuid::new_v4(),
    )
    .unwrap();

    map.insert("a.exe".to_string(), &1).unwrap();
    map.insert("plain".to_string(), &2).unwrap();
    assert_eq!(map.len().unwrap(), 2, "both entries were written");

    drop(map);
    store.delete_prefix(["dotted"]).unwrap();

    let reopened = reactive_map_with_path_only::<String, u32, WritableMode>(
        &store,
        ["dotted", "items"],
        HashMap::new(),
        Uuid::new_v4(),
    )
    .unwrap();

    assert_eq!(reopened.get(&"plain".to_string()).unwrap(), None, "plain");
    assert_eq!(reopened.get(&"a.exe".to_string()).unwrap(), None, "a.exe");
}
