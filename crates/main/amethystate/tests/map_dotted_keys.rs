use amethystate::store::builder::StoreBuilder;
use amethystate::store::reactive_map_with_path_only;
use amethystate_core::access::WritableMode;
use amethystate_core::test_utils::TempPath;
use std::collections::HashMap;
use uuid::Uuid;

#[test]
#[ignore = "known: text backends split map keys on the separator - see TODO.md"]
fn keys_containing_the_separator_stay_separate_entries() {
    let path = TempPath::new("map_dotted");
    let store = StoreBuilder::new(path.path()).build().unwrap();
    let map = reactive_map_with_path_only::<String, u32, WritableMode>(
        &store,
        "dotted.items".into(),
        HashMap::new(),
        Uuid::new_v4(),
    )
    .unwrap();

    map.insert("a.exe".to_string(), &1).unwrap();
    map.insert("a.dll".to_string(), &2).unwrap();
    map.insert("b.exe".to_string(), &3).unwrap();

    assert_eq!(map.get(&"a.exe".to_string()).unwrap(), Some(1), "get a.exe");
    assert_eq!(map.get(&"a.dll".to_string()).unwrap(), Some(2), "get a.dll");
    assert_eq!(map.get(&"b.exe".to_string()).unwrap(), Some(3), "get b.exe");
    assert_eq!(map.keys().unwrap().len(), 3, "keys");
    assert_eq!(map.entries().unwrap().count(), 3, "entries");
    assert_eq!(map.len().unwrap(), 3, "len");

    drop(map);
    let reopened = reactive_map_with_path_only::<String, u32, WritableMode>(
        &store,
        "dotted.items".into(),
        HashMap::new(),
        Uuid::new_v4(),
    )
    .unwrap();

    assert_eq!(reopened.len().unwrap(), 3, "len after a reopen");
    assert_eq!(
        reopened.get(&"a.exe".to_string()).unwrap(),
        Some(1),
        "get after a reopen"
    );
}

#[test]
#[ignore = "known: text backends split map keys on the separator - see TODO.md"]
fn a_key_that_is_a_prefix_of_another_keeps_its_own_value() {
    let path = TempPath::new("map_dotted_collide");
    let store = StoreBuilder::new(path.path()).build().unwrap();
    let map = reactive_map_with_path_only::<String, u32, WritableMode>(
        &store,
        "collide.items".into(),
        HashMap::new(),
        Uuid::new_v4(),
    )
    .unwrap();

    map.insert("a".to_string(), &1).unwrap();
    map.insert("a.b".to_string(), &2).unwrap();

    assert_eq!(map.get(&"a".to_string()).unwrap(), Some(1), "leaf survived");
    assert_eq!(map.get(&"a.b".to_string()).unwrap(), Some(2), "branch");

    drop(map);
    let reopened = reactive_map_with_path_only::<String, u32, WritableMode>(
        &store,
        "collide.items".into(),
        HashMap::new(),
        Uuid::new_v4(),
    )
    .unwrap();

    assert_eq!(
        reopened.get(&"a".to_string()).unwrap(),
        Some(1),
        "leaf survived a reopen"
    );
    assert_eq!(
        reopened.get(&"a.b".to_string()).unwrap(),
        Some(2),
        "branch survived a reopen"
    );
}
