//! A path with nothing in it addresses everything.
//!
//! `StorePath::root()` is public and writing to it replaces the whole
//! document, which is at least something a person had to ask for by name. This
//! is about arriving there without asking: a list of segments computed at run
//! time can come out empty, and an empty list is the root.

#![cfg(any(feature = "json", feature = "toml", feature = "ron"))]

use amethystate::store::builder::StoreBuilder;
use amethystate::store::field_with_path;
use amethystate::uuid::Uuid;
use amethystate_core::path::StorePath;
use amethystate_core::test_utils::TempPath;

mod common;
use common::text_backend;

/// One empty segment is refused by name and by position. No segments at all is
/// the whole store.
///
/// `StorePathError` has an `EmptySegment` and nothing for an empty path, so
/// `try_from_segments` walks a list of no segments, finds nothing to object to,
/// and returns the root.
#[test]
#[ignore = "known: an empty segment is an error and an empty list of segments \
            is the root, so a path that filters down to nothing addresses \
            everything"]
fn an_empty_segment_is_refused_and_an_empty_list_is_not() {
    assert!(
        StorePath::try_from_segments(["ui", ""]).is_err(),
        "an empty segment is refused, which is the behaviour this contrasts with"
    );

    let nothing: Vec<String> = Vec::new();
    let path = StorePath::try_from_segments(&nothing)
        .expect("a list of no segments is accepted rather than refused");

    assert!(
        !path.is_root(),
        "a list of no segments became the root, so a path computed at run time \
         that filters down to nothing addresses the entire store"
    );
}

/// What that costs when the empty list reaches a write.
///
/// Nothing here names the root. The segments are computed, the filter happens
/// to remove all of them, and the write that follows returns success.
///
/// A scalar at the root is refused by the guard that stops a scalar landing on
/// a branch, so the shape that gets through is the ordinary one: a struct or a
/// map, written at a path that came out empty.
#[test]
#[ignore = "known: a struct written at a path that computed to nothing replaces \
            the whole document and returns Ok"]
fn a_path_that_filtered_down_to_nothing_does_not_replace_the_store() {
    let path = TempPath::new("empty_path_write");

    let store = StoreBuilder::new(path.path())
        .backend(text_backend())
        .build()
        .unwrap();

    let kept = field_with_path::<u32>(&store, ["ui", "width"], 1280, Uuid::new_v4()).unwrap();
    kept.set(1920).unwrap();
    store.save_now().unwrap();

    let wanted = ["", ""];
    let computed: Vec<&str> = wanted.iter().copied().filter(|s| !s.is_empty()).collect();

    let mut value = std::collections::HashMap::new();
    value.insert("theme".to_string(), "dark".to_string());
    let wrote = store.set(computed, &value);

    store.save_now().unwrap();

    assert!(
        wrote.is_err(),
        "a write at a path that came out empty was accepted; the file is now {}",
        std::fs::read_to_string(path.path()).unwrap_or_default()
    );
    assert_eq!(
        kept.get(),
        1920,
        "a write at a path that came out empty replaced the whole document"
    );
}
