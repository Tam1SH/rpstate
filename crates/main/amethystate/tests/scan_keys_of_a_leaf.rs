//! What a scan of a path with nothing under it answers.
//!
//! `scan_keys` names the children of a level. A leaf has none, so the answer is
//! the empty list - the same answer as for a path that is not there at all,
//! which is right: neither has children.
//!
//! This lives in the shared text store rather than in any one engine, so it is
//! the same question on json, toml and ron.

#![cfg(any(feature = "json", feature = "toml", feature = "ron"))]

use amethystate::store::builder::StoreBuilder;
use amethystate::store::field_with_path;
use amethystate::uuid::Uuid;
use amethystate_core::path::StorePath;
use amethystate_core::test_utils::TempPath;

mod common;
use common::text_backend;

/// A leaf is not its own child.
///
/// `scan_keys_recursive` pushes the prefix it was given when the node under it
/// has no children, so a leaf comes back naming itself. Anything walking the
/// key space by scanning what a scan returned therefore never gets closer to
/// the bottom - it recurses on the same path until the stack ends, and a stack
/// overflow is not something a caller can catch.
#[test]
#[ignore = "known: scan_keys of a leaf returns the leaf itself, so a recursive \
            walk of the key space does not terminate"]
fn a_leaf_is_not_listed_as_a_child_of_itself() {
    let path = TempPath::new("scan_leaf");

    let store = StoreBuilder::new(path.path())
        .backend(text_backend())
        .build()
        .unwrap();

    let leaf = field_with_path::<u32>(&store, ["leafy", "value"], 1, Uuid::new_v4()).unwrap();
    leaf.set(7).unwrap();
    store.save_now().unwrap();

    let under_the_level = store
        .scan_keys(&StorePath::from_segments(["leafy"]))
        .unwrap();
    assert_eq!(
        under_the_level,
        vec![StorePath::from_segments(["leafy", "value"])],
        "the level above the leaf names it, which is the control for the case below"
    );

    let under_the_leaf = store
        .scan_keys(&StorePath::from_segments(["leafy", "value"]))
        .unwrap();
    assert!(
        under_the_leaf.is_empty(),
        "a scan of a leaf answered {under_the_leaf:?} - it named the leaf as its \
         own child, so a walk that recurses into what a scan returns never ends"
    );
}

/// The control: a path that was never written answers the same way, and does.
#[test]
fn a_path_that_is_not_there_has_no_children() {
    let path = TempPath::new("scan_absent");

    let store = StoreBuilder::new(path.path())
        .backend(text_backend())
        .build()
        .unwrap();

    let absent = store
        .scan_keys(&StorePath::from_segments(["nothing", "here"]))
        .unwrap();
    assert!(absent.is_empty());
}
