//! Reading a map back on one core and on several gives the same map.
//!
//! The choice is a setting on the store rather than a build flag, so both
//! paths are in every binary and one test covers both. A flag would have
//! doubled the matrix instead, and the path nobody built would be the one that
//! rots.
//!
//! Sized past the point where the work is divided at all - below roughly a
//! thousand entries neither setting splits anything, and a test under that
//! would compare one code path with itself.

use amethystate::store::StoreBackend;
use amethystate::store::builder::StoreBuilder;
use amethystate_core::test_utils::TempPath;

const ENTRIES: usize = 5_000;

fn key(i: usize) -> String {
    format!("k{i:05}")
}

#[test]
fn both_settings_read_back_the_same_map() {
    let path = TempPath::new("parallel_reads");

    {
        let store = StoreBuilder::new(path.path()).build().unwrap();
        let map = store.kv().map::<String, u64>("wide").unwrap();
        for i in 0..ENTRIES {
            map.insert(key(i), &(i as u64)).unwrap();
        }
        store.save_now().unwrap();
    }

    let sequential = {
        let store = StoreBuilder::new(path.path())
            .parallel_reads(false)
            .build()
            .unwrap();
        let map = store.kv().map::<String, u64>("wide").unwrap();
        map.entries().collect::<Vec<_>>()
    };

    let parallel = {
        let store = StoreBuilder::new(path.path())
            .parallel_reads(true)
            .build()
            .unwrap();
        let map = store.kv().map::<String, u64>("wide").unwrap();
        map.entries().collect::<Vec<_>>()
    };

    assert_eq!(sequential.len(), ENTRIES, "every entry came back");
    assert_eq!(
        sequential, parallel,
        "dividing the work across cores must not change what is read, nor the \
         order it is read in"
    );
}

/// A key one entry cannot read is still an error, and still names that entry,
/// whichever way the work was divided. Rayon reports one failure out of many,
/// so this is where "which one" could quietly become "some one".
#[test]
fn a_bad_entry_is_reported_either_way() {
    let path = TempPath::new("parallel_reads_bad");

    {
        let store = StoreBuilder::new(path.path()).build().unwrap();
        let map = store.kv().map::<String, u64>("wide").unwrap();
        for i in 0..ENTRIES {
            map.insert(key(i), &(i as u64)).unwrap();
        }
        store.save_now().unwrap();

        // One entry that will not read back as the map's value type.
        store
            .kv()
            .namespace("wide")
            .set("k00042", &"not a number".to_string())
            .unwrap();
        store.save_now().unwrap();
    }

    for parallel in [false, true] {
        let store = StoreBuilder::new(path.path())
            .parallel_reads(parallel)
            .build()
            .unwrap();

        let failure = store
            .kv()
            .map::<String, u64>("wide")
            .expect_err("a value that will not decode is an error, not an absence");

        let text = format!("{failure:?}");
        assert!(
            text.contains("k00042"),
            "parallel = {parallel}: the failure should name the entry: {text}"
        );
    }
}
