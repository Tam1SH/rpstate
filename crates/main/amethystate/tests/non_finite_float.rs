//! What a float that is not a number does to a store.
//!
//! `NaN` and the infinities are ordinary `f64` values a GUI produces by
//! dividing badly. Three of the five engines carry them: msgpack writes the
//! IEEE bits, and TOML and RON have `nan` and `inf` in their grammars.
//!
//! JSON does not, and neither `serde_json` nor `sonic_rs` refuses it - both
//! write `null`, which reads back as nothing at all. That costs two engines
//! rather than one, because sqlite stores its values as JSON and nothing in
//! its name says so.
//!
//! So it is a property of the format rather than of the file, and the tests
//! below say which engines have it.

use amethystate::amethystate;
use amethystate::store::builder::StoreBuilder;
use amethystate_core::path::StorePath;
use amethystate_core::test_utils::TempPath;

mod common;

#[amethystate(prefix = "nonfinite")]
pub struct Readings {
    #[amestate(default = 0.0f64)]
    pub ratio: f64,
}

fn ratio_path() -> StorePath {
    StorePath::from_segments(["nonfinite", "ratio"])
}

#[cfg(any(feature = "redb", feature = "toml", feature = "ron"))]
#[test]
fn a_format_that_can_hold_it_keeps_it() {
    let backend = common::enabled_backends()
        .into_iter()
        .find(|b| matches!(common::engine_name(*b), "redb" | "toml" | "ron"))
        .expect("msgpack writes the bits, and toml and ron have the words");

    let path = TempPath::new("nonfinite_ok");
    let store = StoreBuilder::new(path.path())
        .backend(backend)
        .build()
        .unwrap();

    let state = Readings::new_with(&store).unwrap();
    state.ratio().set(f64::NAN).unwrap();

    assert!(
        state.ratio().get().is_nan(),
        "the handle should still hold what was written"
    );

    let read: Option<f64> = store
        .get(ratio_path())
        .expect("a typed read should not fail on a value the format can hold");
    assert!(
        read.map(f64::is_nan).unwrap_or(false),
        "read back: {read:?}"
    );
}

#[cfg(all(feature = "json", not(feature = "redb")))]
#[test]
fn json_takes_a_write_it_cannot_store() {
    use amethystate::StoreExt;
    use amethystate::store::StoreBackend;
    use amethystate::store::builder::Backend;

    let path = TempPath::new("nonfinite_json");
    let store = StoreBuilder::new(path.path())
        .backend(Backend::Json)
        .build()
        .unwrap();

    let state = Readings::new_with(&store).unwrap();
    state.ratio().set(5.0).unwrap();

    assert!(
        state.ratio().set(f64::NAN).is_ok(),
        "the write is accepted today, which is the defect"
    );
    assert_eq!(
        state.ratio().get(),
        5.0,
        "the handle keeps the last value the store agreed with"
    );
    assert!(
        state.ratio().try_get().is_err(),
        "and says the store no longer agrees, which is what separates it \
         from a write that worked"
    );
    assert!(
        StoreExt::get::<f64>(&store, ratio_path()).is_err(),
        "a typed read of the same path fails, where the handle tolerates"
    );

    store.save_now().unwrap();
    let document = std::fs::read_to_string(path.path()).unwrap();
    assert!(
        document.contains("null"),
        "the document holds nothing where the value was: {document}"
    );

    // And the field says so for exactly as long as it is true.
    state.ratio().set(1.5).unwrap();
    assert_eq!(state.ratio().get(), 1.5);
    assert_eq!(
        state.ratio().try_get().unwrap(),
        1.5,
        "a change that decodes clears it"
    );
}

#[cfg(all(feature = "sqlite", not(feature = "redb")))]
#[test]
fn sqlite_loses_it_too_because_it_stores_json() {
    use amethystate::StoreExt;
    use amethystate::store::builder::Backend;

    let path = TempPath::new("nonfinite_sqlite");
    let store = StoreBuilder::new(path.path())
        .backend(Backend::Sqlite)
        .build()
        .unwrap();

    let state = Readings::new_with(&store).unwrap();
    state.ratio().set(5.0).unwrap();

    assert!(state.ratio().set(f64::NAN).is_ok(), "the write is accepted");
    assert_eq!(
        state.ratio().get(),
        5.0,
        "the handle keeps the last value the store agreed with"
    );
    assert!(
        state.ratio().try_get().is_err(),
        "and says so, as it does on the json engine"
    );
    assert!(StoreExt::get::<f64>(&store, ratio_path()).is_err());
}
