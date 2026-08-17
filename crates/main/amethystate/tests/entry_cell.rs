use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use amethystate::store::builder::StoreBuilder;
use amethystate::{ReactiveMap, amethystate};
use amethystate_core::test_utils::unique_path;

#[amethystate(prefix = "app")]
pub struct TableConfig {
    #[amestate(default = 110u64)]
    pub default_width: u64,

    #[amestate(default = {
        "name": 280u64,
        "cpu": 80u64,
    })]
    pub widths: ReactiveMap<String, u64>,
}

#[test]
fn entry_cell_reads_existing_value() {
    let path = unique_path("entry_existing");
    let store = StoreBuilder::new(&path).build().unwrap();
    let config = TableConfig::new_with(&store).unwrap();

    let entry = config.widths().entry_cell("name".to_string(), 110);
    assert_eq!(entry.get(), 280);
}

#[test]
fn entry_cell_uses_default_for_missing_key() {
    let path = unique_path("entry_default");
    let store = StoreBuilder::new(&path).build().unwrap();
    let config = TableConfig::new_with(&store).unwrap();

    let entry = config.widths().entry_cell("disk".to_string(), 110);
    assert_eq!(entry.get(), 110);
}

#[test]
fn write_lands_in_store() {
    let path = unique_path("entry_write");
    let store = StoreBuilder::new(&path).build().unwrap();
    let config = TableConfig::new_with(&store).unwrap();

    let entry = config.widths().entry_cell("cpu".to_string(), 110);
    entry.set(144).unwrap();

    assert_eq!(config.widths().get(&"cpu".to_string()).unwrap(), Some(144));
    assert_eq!(entry.get(), 144);
}

#[test]
fn external_map_write_lands_in_cell() {
    let path = unique_path("entry_external");
    let store = StoreBuilder::new(&path).build().unwrap();
    let config = TableConfig::new_with(&store).unwrap();

    let entry = config.widths().entry_cell("cpu".to_string(), 110);
    config.widths().update("cpu".to_string(), &99).unwrap();

    assert_eq!(entry.get(), 99);
}

#[test]
fn removed_key_falls_back_to_default() {
    let path = unique_path("entry_remove");
    let store = StoreBuilder::new(&path).build().unwrap();
    let config = TableConfig::new_with(&store).unwrap();

    let entry = config.widths().entry_cell("cpu".to_string(), 110);
    assert_eq!(entry.get(), 80);
    config.widths().remove("cpu".to_string()).unwrap();
    assert_eq!(entry.get(), 110);
}

/// Two independent cells on the same key - a table column and, say, a settings
/// editor. Each side must see the other's writes, and each write must raise a
/// subscriber exactly once.
///
/// This is the case the whole refactor is about. The old entry signal wrote its
/// own cache *and* took the change again when the store echoed it back, so a
/// local write fired twice; only a remote write fired once. With the map
/// subscription as the sole writer, both directions cost one fire.
#[test]
fn two_cells_on_one_key_fire_once_per_write() {
    let path = unique_path("entry_no_echo");
    let store = StoreBuilder::new(&path).build().unwrap();
    let config = TableConfig::new_with(&store).unwrap();

    let a = config.widths().entry_cell("cpu".to_string(), 110);
    let b = config.widths().entry_cell("cpu".to_string(), 110);

    let fires = Arc::new(AtomicUsize::new(0));
    let fires_clone = fires.clone();
    let _watch = a.subscribe(move |_: &u64| {
        fires_clone.fetch_add(1, Ordering::SeqCst);
    });

    b.set(200).unwrap();

    assert_eq!(a.get(), 200, "b's write must reach a");
    assert_eq!(config.widths().get(&"cpu".to_string()).unwrap(), Some(200));
    assert_eq!(
        fires.load(Ordering::SeqCst),
        1,
        "a remote write must fire a exactly once"
    );

    a.set(300).unwrap();

    assert_eq!(b.get(), 300, "a's write must reach b");
    assert_eq!(
        fires.load(Ordering::SeqCst),
        2,
        "a local write must fire once too, not twice"
    );
}

/// A rejected write must say so and leave the cell reporting what is stored.
/// The old entry signal swallowed the error and kept the value it had already
/// written into its own cache, so the cell quietly disagreed with the store.
#[test]
fn rejected_write_reports_and_leaves_the_cell_alone() {
    let path = unique_path("entry_rejected");
    let store = StoreBuilder::new(&path).build().unwrap();
    let config = TableConfig::new_with(&store).unwrap();

    let entry = config.widths().entry_cell("cpu".to_string(), 110);
    let _guard = config.widths().intercept(|_change| None);

    let result = entry.set(999);

    assert!(result.is_err(), "a refused write must not report success");
    assert_eq!(
        entry.get(),
        80,
        "cell must not hold a value the map refused"
    );
    assert_eq!(config.widths().get(&"cpu".to_string()).unwrap(), Some(80));
}

/// The map subscription feeding the cache is owned by the cell alone, so this
/// is the one place the keepalive actually earns its keep: without it the cell
/// would go on answering `get()` with a value that stopped being updated.
#[test]
fn cell_keeps_receiving_after_its_map_handle_is_dropped() {
    let path = unique_path("entry_keepalive");
    let store = StoreBuilder::new(&path).build().unwrap();
    let config = TableConfig::new_with(&store).unwrap();

    let entry = {
        let widths = config.widths();
        widths.entry_cell("cpu".to_string(), 110)
    };

    config.widths().update("cpu".to_string(), &55).unwrap();

    assert_eq!(entry.get(), 55, "cache must still be fed");
}

/// Integration: a write through the entry cell survives a store rebuild.
#[test]
fn entry_cell_write_persists_across_store_rebuild() {
    let path = unique_path("entry_persist");

    {
        let store = StoreBuilder::new(&path).build().unwrap();
        let config = TableConfig::new_with(&store).unwrap();
        let entry = config.widths().entry_cell("memory".to_string(), 110);
        entry.set(256).unwrap();
    }

    {
        let store = StoreBuilder::new(&path).build().unwrap();
        let config = TableConfig::new_with(&store).unwrap();
        assert_eq!(
            config.widths().get(&"memory".to_string()).unwrap(),
            Some(256)
        );

        let entry = config.widths().entry_cell("memory".to_string(), 110);
        assert_eq!(entry.get(), 256);
    }
}
