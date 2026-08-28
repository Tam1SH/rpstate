//! What each engine writes for a value that is there and holds nothing.
//!
//! `{}` and `null` are different documents, and an absent key is a third thing
//! again. Which of the three an engine produces for `None` decides what a
//! schema can say about it: a property that may be null is not a property that
//! may be missing, and the two are written differently.
//!
//! Measured rather than assumed, and the engines answer in two ways:
//!
//! | engine | the document for `None` | `get::<Option<T>>` |
//! | --- | --- | --- |
//! | json | `"note": null` | `Some(None)` |
//! | ron | `"note": None` | `Some(None)` |
//! | toml | no such key | `None` |
//!
//! TOML has no null, so a key holding nothing is a key that is not written.
//! That is how every TOML config expresses an optional setting, and it means
//! the format answers `set(None)` and `delete` with the same document - a
//! distinction the other engines keep and this one cannot.

use amethystate::amethystate;
use amethystate::store::StoreBackend;
use amethystate::store::builder::{Backend, StoreBuilder, default_backend};
use amethystate_core::path::StorePath;
use amethystate_core::test_utils::TempPath;

mod common;

#[amethystate(prefix = "maybe")]
pub struct Held {
    #[amestate(default = None)]
    pub note: Option<String>,
}

fn note_path() -> StorePath {
    StorePath::from_segments(["maybe", "note"])
}

/// Whether the engine can write a key that is there and holds nothing.
fn holds_nothing(backend: Backend) -> Option<Option<String>> {
    if backend.extension() == "toml" {
        None
    } else {
        Some(None)
    }
}

/// The document engines write the file, so the file is the answer.
#[cfg(any(feature = "json", feature = "toml", feature = "ron"))]
#[test]
fn what_a_document_holds_for_nothing() {
    let path = TempPath::new("absent_or_null");
    let backend = common::text_backend();
    let store = StoreBuilder::new(path.path())
        .backend(backend)
        .build()
        .unwrap();

    let held = Held::new_with(&store).unwrap();
    held.note().set(Some("here".to_string())).unwrap();
    store.save_now().unwrap();
    let with_value = std::fs::read_to_string(path.path()).unwrap();

    held.note().set(None).unwrap();
    store.save_now().unwrap();
    let with_nothing = std::fs::read_to_string(path.path()).unwrap();

    println!("--- some ---\n{with_value}");
    println!("--- none ---\n{with_nothing}");

    assert_ne!(
        with_value, with_nothing,
        "a value and its absence must not write the same document"
    );

    let read: Option<Option<String>> = store.get(note_path()).unwrap();
    assert_eq!(
        read,
        holds_nothing(backend),
        "what the engine reads back for a value set to nothing"
    );
}

/// And a key that was deleted is a fourth state, which the engines that have a
/// null keep apart from the third.
#[test]
fn nothing_and_gone_are_different() {
    let path = TempPath::new("absent_or_gone");
    let store = StoreBuilder::new(path.path()).build().unwrap();

    let held = Held::new_with(&store).unwrap();
    held.note().set(None).unwrap();

    assert_eq!(
        store.get::<Option<String>>(note_path()).unwrap(),
        holds_nothing(default_backend()),
        "set to nothing"
    );

    StoreBackend::delete(&store, &note_path()).unwrap();

    assert_eq!(
        store.get::<Option<String>>(note_path()).unwrap(),
        None,
        "deleted: there is no key"
    );
}
