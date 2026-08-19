//! What a durable write buys, proved the only way redb allows: by killing a
//! process that never got to flush.
//!
//! redb holds its lock for the life of the process, so the file cannot be
//! reopened while the writer is alive - suppressing `Drop` is not enough. The
//! writer here aborts instead, which skips destructors exactly like a crash.

use amethystate::store::builder::Backend;
use amethystate::store::field_with_path;
use amethystate::{StoreBuilder, WritableMode};
use amethystate_core::test_utils::unique_path;
use std::sync::Arc;

const CHILD: &str = "AME_DURABILITY_CHILD";

fn write_then_abort(path: &std::path::Path) -> ! {
    let store = StoreBuilder::new(path)
        .backend(Backend::Redb)
        .debounce(60_000)
        .build()
        .unwrap();

    let plain = field_with_path::<u16, WritableMode>(
        &store,
        Arc::from("n.plain"),
        1,
        amethystate::uuid::Uuid::new_v4(),
    )
    .unwrap();
    let durable = field_with_path::<u16, WritableMode>(
        &store,
        Arc::from("n.durable"),
        1,
        amethystate::uuid::Uuid::new_v4(),
    )
    .unwrap();

    plain.set(777).unwrap();
    durable.durable().set(888).unwrap();

    std::process::abort();
}

#[test]
fn a_durable_write_survives_a_crash_and_a_plain_one_does_not() {
    let path = unique_path("durability_crash");

    if let Ok(child_path) = std::env::var(CHILD) {
        write_then_abort(std::path::Path::new(&child_path));
    }

    let status = std::process::Command::new(std::env::current_exe().unwrap())
        .arg("--exact")
        .arg("a_durable_write_survives_a_crash_and_a_plain_one_does_not")
        .env(CHILD, &path)
        .output()
        .expect("spawning the writer failed");

    assert!(
        !status.status.success(),
        "the writer was supposed to abort, not exit cleanly"
    );

    let store = StoreBuilder::new(&path)
        .backend(Backend::Redb)
        .build()
        .expect("the crashed writer released the lock");

    assert_eq!(
        store
            .kv()
            .namespace("n")
            .unwrap()
            .get::<u16>("durable")
            .unwrap(),
        Some(888),
        "the durable write had committed before the abort"
    );
    assert_eq!(
        store
            .kv()
            .namespace("n")
            .unwrap()
            .get::<u16>("plain")
            .unwrap(),
        None,
        "the plain write was still buffered and died with the process"
    );
}
