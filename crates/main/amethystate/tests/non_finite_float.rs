//! What a float that is not a number does to a store.
//!
//! `NaN` and the infinities are ordinary `f64` values a GUI produces by
//! dividing badly, and four of the five engines carry them: msgpack writes the
//! IEEE bits, and TOML and RON have `nan` and `inf` in their grammars. JSON
//! does not, and `serde_json` does not refuse it either - it writes `null`,
//! which reads back as nothing at all.
//!
//! So this is one engine's problem rather than the document engines' problem,
//! and the tests below say which is which.

use amethystate::store::StoreBackend;
use amethystate::store::builder::StoreBuilder;
use amethystate::{StoreExt, amethystate};
use amethystate_core::path::StorePath;
use amethystate_core::test_utils::TempPath;

#[amethystate(prefix = "nonfinite")]
pub struct Readings {
    #[amestate(default = 0.0f64)]
    pub ratio: f64,
}

fn ratio_path() -> StorePath {
    StorePath::from_segments(["nonfinite", "ratio"])
}

/// Every format that can spell it, spells it: the handle keeps the value and a
/// typed read of the same path agrees.
#[cfg(any(feature = "redb", feature = "toml", feature = "ron"))]
#[test]
fn a_format_that_can_hold_it_keeps_it() {
    use amethystate::store::builder::Backend;

    #[cfg(feature = "redb")]
    let backend = Backend::Redb;
    #[cfg(all(feature = "toml", not(feature = "redb")))]
    let backend = Backend::Toml;
    #[cfg(all(feature = "ron", not(feature = "redb"), not(feature = "toml")))]
    let backend = Backend::Ron;

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

/// JSON takes a write it cannot store, and the write is lost between the store
/// and the handle.
///
/// `serde_json` maps a non-finite float to `null` rather than refusing it, so
/// the write returns `Ok` and the document holds `null`. The field's own
/// subscription then cannot decode what it is handed, logs it, and leaves the
/// signal alone - so the handle goes on reporting **the value it held before**,
/// which is an ordinary number indistinguishable from one that was written.
/// Only a typed read of the same path fails. The same bytes mean two things
/// depending on which read reaches them.
///
/// The stale value is the part worth pinning, and the part easiest to mistake
/// for something milder: nothing substitutes a default here, and a field whose
/// last good value was `5.0` keeps saying `5.0` about a store that holds
/// nothing.
///
/// Pinned as it stands rather than as it should be. The repair belongs in the
/// codec: a format that cannot represent a value should refuse it at the write,
/// where the caller still holds the value and can decide what to do.
#[cfg(all(feature = "json", not(feature = "redb")))]
#[test]
fn json_takes_a_write_it_cannot_store() {
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
        "the handle still reports what it held before the write it accepted"
    );
    assert!(
        StoreExt::get::<f64>(&store, ratio_path()).is_err(),
        "a typed read of the same path fails, where the handle did not"
    );

    store.save_now().unwrap();
    let document = std::fs::read_to_string(path.path()).unwrap();
    assert!(
        document.contains("null"),
        "the document holds nothing where the value was: {document}"
    );
}
