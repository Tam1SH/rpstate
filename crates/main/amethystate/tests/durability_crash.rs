//! What a durable write buys, proved the only way that proves it: by killing a
//! process that never got to flush.
//!
//! The writer aborts rather than returning, which skips destructors exactly
//! like a crash - and for redb it is the only option, since it holds its lock
//! for the life of the process and suppressing `Drop` would not release it.
//!
//! This is a statement about a store, not about one engine, so it runs against
//! every engine that is compiled in.

use amethystate::store::builder::Backend;
use amethystate::store::field_with_path;
use amethystate::{StoreBuilder, WritableMode};
use amethystate_core::test_utils::unique_path;
use std::path::Path;

const CHILD: &str = "AME_DURABILITY_CHILD";

fn write_then_abort(backend: Backend, path: &Path) -> ! {
    let store = StoreBuilder::new(path)
        .backend(backend)
        .debounce(60_000)
        .build()
        .unwrap();

    let plain = field_with_path::<u16, WritableMode>(
        &store,
        ["n", "plain"],
        1,
        amethystate::uuid::Uuid::new_v4(),
    )
    .unwrap();
    let durable = field_with_path::<u16, WritableMode>(
        &store,
        ["n", "durable"],
        1,
        amethystate::uuid::Uuid::new_v4(),
    )
    .unwrap();

    plain.set(777).unwrap();
    durable.durable().set(888).unwrap();

    std::process::abort();
}

/// How much of what was pending a commit takes with it.
#[derive(Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
enum Granularity {
    /// The commit covers the durable write and nothing else, so a plain write
    /// still buffered when the process died is gone.
    OneWrite,

    /// The commit covers the whole store, so every buffered write rides along.
    /// The text engines rewrite the document to save any of it - `flush_prefix`
    /// there ignores its prefix and calls `save_now`.
    EveryWrite,
}

fn crash_then_reopen(test_name: &str, backend: Backend, granularity: Granularity) {
    if let Ok(child_path) = std::env::var(CHILD) {
        write_then_abort(backend, Path::new(&child_path));
    }

    let path = unique_path(test_name);

    let status = std::process::Command::new(std::env::current_exe().unwrap())
        .arg("--exact")
        .arg(test_name)
        .env(CHILD, &path)
        .output()
        .expect("spawning the writer failed");

    assert!(
        !status.status.success(),
        "the writer was supposed to abort, not exit cleanly"
    );

    let store = StoreBuilder::new(&path)
        .backend(backend)
        .build()
        .expect("the crashed writer left the store openable");

    let n = store.kv().namespace("n");

    assert_eq!(
        n.get::<u16>("durable").unwrap(),
        Some(888),
        "the durable write had committed before the abort"
    );

    let plain = n.get::<u16>("plain").unwrap();
    match granularity {
        Granularity::OneWrite => assert_eq!(
            plain, None,
            "the plain write was still buffered and died with the process"
        ),
        Granularity::EveryWrite => assert_eq!(
            plain,
            Some(777),
            "committing anything on this engine commits everything"
        ),
    }
}

macro_rules! durability_across_a_crash {
    ($feature:literal, $name:ident, $backend:expr, $granularity:expr) => {
        #[cfg(feature = $feature)]
        #[test]
        fn $name() {
            crash_then_reopen(stringify!($name), $backend, $granularity);
        }
    };
}

durability_across_a_crash!(
    "redb",
    a_durable_write_survives_a_crash_on_redb,
    Backend::Redb,
    Granularity::OneWrite
);
durability_across_a_crash!(
    "sqlite",
    a_durable_write_survives_a_crash_on_sqlite,
    Backend::Sqlite,
    Granularity::OneWrite
);
durability_across_a_crash!(
    "json",
    a_durable_write_survives_a_crash_on_json,
    Backend::Json,
    Granularity::EveryWrite
);
durability_across_a_crash!(
    "toml",
    a_durable_write_survives_a_crash_on_toml,
    Backend::Toml,
    Granularity::EveryWrite
);
durability_across_a_crash!(
    "ron",
    a_durable_write_survives_a_crash_on_ron,
    Backend::Ron,
    Granularity::EveryWrite
);
