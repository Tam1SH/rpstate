use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

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
fn entry_signal_reads_existing_value() {
    let path = unique_path("entry_existing");
    let store = StoreBuilder::new(&path).build().unwrap();
    let config = TableConfig::new_with(&store).unwrap();

    let entry = config.widths().entry_signal("name".to_string(), 110);
    assert_eq!(entry.get(), 280);
}

#[test]
fn entry_signal_uses_default_for_missing_key() {
    let path = unique_path("entry_default");
    let store = StoreBuilder::new(&path).build().unwrap();
    let config = TableConfig::new_with(&store).unwrap();

    let entry = config.widths().entry_signal("disk".to_string(), 110);
    assert_eq!(entry.get(), 110);
}

#[test]
fn signal_write_lands_in_store() {
    let path = unique_path("entry_write");
    let store = StoreBuilder::new(&path).build().unwrap();
    let config = TableConfig::new_with(&store).unwrap();

    let entry = config.widths().entry_signal("cpu".to_string(), 110);
    entry.signal().set(144, None);

    assert_eq!(config.widths().get(&"cpu".to_string()).unwrap(), Some(144));
}

#[test]
fn external_map_write_lands_in_signal() {
    let path = unique_path("entry_external");
    let store = StoreBuilder::new(&path).build().unwrap();
    let config = TableConfig::new_with(&store).unwrap();

    let entry = config.widths().entry_signal("cpu".to_string(), 110);
    config.widths().set("cpu".to_string(), &99).unwrap();

    assert_eq!(entry.get(), 99);
}

#[test]
fn removed_key_falls_back_to_default() {
    let path = unique_path("entry_remove");
    let store = StoreBuilder::new(&path).build().unwrap();
    let config = TableConfig::new_with(&store).unwrap();

    let entry = config.widths().entry_signal("cpu".to_string(), 110);
    assert_eq!(entry.get(), 80);
    config.widths().remove("cpu".to_string()).unwrap();
    assert_eq!(entry.get(), 110);
}

/// Two independent handles on the same store (a table column and, say, a
/// settings editor): each side must see the other's writes exactly once -
/// the marker-source + instance-id filtering must prevent a ping-pong.
#[test]
fn two_entry_signals_sync_without_echo_loop() {
    let path = unique_path("entry_no_echo");
    let store = StoreBuilder::new(&path).build().unwrap();
    let config = TableConfig::new_with(&store).unwrap();

    let a = config.widths().entry_signal("cpu".to_string(), 110);
    let b = config.widths().entry_signal("cpu".to_string(), 110);

    let fires = Arc::new(AtomicUsize::new(0));
    let fires_clone = fires.clone();
    let _watch = a.signal().subscribe(move |_| {
        fires_clone.fetch_add(1, Ordering::SeqCst);
    });

    b.set(200);

    assert_eq!(a.get(), 200, "b's write must reach a");
    assert_eq!(config.widths().get(&"cpu".to_string()).unwrap(), Some(200));
    // Delivery is synchronous: b.set -> store -> a's read (1 fire). The
    // store-originated re-set carries the marker source, so it is not written
    // back - no second fire, no echo.
    assert_eq!(fires.load(Ordering::SeqCst), 1, "b's write must fire a exactly once");

    a.set(300);

    assert_eq!(b.get(), 300, "a's write must reach b");
    // a.set fires the watch directly (2nd), then the store round-trip re-set
    // fires once more (3rd, marked, not written back). Still no echo.
    assert_eq!(fires.load(Ordering::SeqCst), 3, "a's write must settle without echo");
}

/// Integration: a write through the entry signal survives a store rebuild.
#[test]
fn entry_signal_write_persists_across_store_rebuild() {
    let path = unique_path("entry_persist");

    {
        let store = StoreBuilder::new(&path).build().unwrap();
        let config = TableConfig::new_with(&store).unwrap();
        let entry = config.widths().entry_signal("memory".to_string(), 110);
        entry.set(256);
    }

    {
        let store = StoreBuilder::new(&path).build().unwrap();
        let config = TableConfig::new_with(&store).unwrap();
        assert_eq!(config.widths().get(&"memory".to_string()).unwrap(), Some(256));

        let entry = config.widths().entry_signal("memory".to_string(), 110);
        assert_eq!(entry.get(), 256);
    }
}
