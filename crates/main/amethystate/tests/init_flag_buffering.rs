use amethystate::{Store, StoreBuilder};
use amethystate_core::test_utils::unique_path;

#[test]
fn a_mark_is_visible_before_it_reaches_disk() {
    let store = StoreBuilder::new(unique_path("readback")).build().unwrap();

    assert!(!store.is_initialized("settings").unwrap());
    store.mark_initialized("settings").unwrap();

    assert!(
        store.is_initialized("settings").unwrap(),
        "the buffer answers, as it does for get"
    );
}

#[test]
fn a_mark_survives_a_flush_and_reopen() {
    let path = unique_path("persist");

    {
        let store = StoreBuilder::new(path.clone()).build().unwrap();
        store.mark_initialized("settings").unwrap();
        store.save_now().unwrap();
    }

    let store = StoreBuilder::new(path).build().unwrap();
    assert!(store.is_initialized("settings").unwrap());
}

#[test]
fn a_prefix_flush_carries_the_mark_for_that_namespace() {
    let path = unique_path("prefix");

    {
        let store = StoreBuilder::new(path.clone()).build().unwrap();
        store.set("settings.port", &8080u16).unwrap();
        store.mark_initialized("settings").unwrap();

        store.flush_prefix("settings").unwrap();
    }

    let store = StoreBuilder::new(path).build().unwrap();
    assert!(
        store.is_initialized("settings").unwrap(),
        "the mark is selected by the same prefix rule as its values"
    );
    assert_eq!(store.get::<u16>("settings.port").unwrap(), Some(8080));
}

#[test]
fn a_mark_does_not_show_up_as_data() {
    let store = StoreBuilder::new(unique_path("notdata")).build().unwrap();

    store.mark_initialized("settings").unwrap();
    store.set("settings.port", &1u16).unwrap();

    let found = store.scan_prefix("settings").unwrap();
    let keys: Vec<&str> = found.iter().map(|(k, _)| k.as_str()).collect();

    assert_eq!(
        keys,
        vec!["settings.port"],
        "the flag shares the buffer but is not a value"
    );
}
