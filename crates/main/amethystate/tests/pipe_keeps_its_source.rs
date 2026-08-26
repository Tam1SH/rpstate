//! A pipeline outlives the handle it was built from.
//!
//! Subscribing does not hold what you subscribed to, so a pipeline that kept
//! only the subscription went quiet the moment its source was dropped - and
//! quiet is the worst way for it to fail: the first value is right, so the
//! screen looks correct and simply stops changing.
//!
//! It is the shape of the README's own pattern. A component takes one field,
//! pipes it, and lets go of the struct the field came from; the struct is what
//! owns the subscription to the store.
//!
//! Piping several sources never had the bug, because the closure that re-reads
//! them all captures a clone of each - so the two forms of one method
//! disagreed about ownership, which is what this pins.

use amethystate::store::builder::StoreBuilder;
use amethystate::{IntoPipeline, Store, StoreExt, amethystate};
use amethystate_core::path::StorePath;
use amethystate_core::test_utils::TempPath;

/// Writes without holding any handle the pipeline could have kept alive.
///
/// A `Field` clone shares the same inner, so keeping one to write with would
/// keep the subscription alive by itself and the test would pass either way.
fn set_port(store: &Store, port: u16) {
    StoreExt::set(store, StorePath::from_segments(["piped", "port"]), &port).unwrap();
}

#[amethystate(prefix = "piped")]
pub struct Settings {
    #[amestate(default = "localhost".to_string())]
    pub host: String,

    #[amestate(default = 8080u16)]
    pub port: u16,
}

#[test]
fn one_source_keeps_reporting_after_the_struct_is_dropped() {
    let path = TempPath::new("pipe_one_source");
    let store = StoreBuilder::new(path.path()).build().unwrap();

    let shown = {
        let state = Settings::new_with(&store).unwrap();
        state.port().pipe().map(|p| format!("port {p}"))
    };
    assert_eq!(shown.get(), "port 8080");

    // The component holds the pipeline and nothing else holds the state.
    set_port(&store, 9090);
    assert_eq!(
        shown.get(),
        "port 9090",
        "a pipeline built from one source stopped following it once the \
         source's owner was dropped"
    );
}

/// The same, through a cell - which exists precisely to be the handle that
/// owns, and used to have that ownership dropped on the way into a pipeline.
#[test]
fn a_cell_carries_its_owner_into_a_pipeline() {
    let path = TempPath::new("pipe_cell");
    let store = StoreBuilder::new(path.path()).build().unwrap();

    let shown = {
        let state = Settings::new_with(&store).unwrap();
        state.port().into_cell().pipe().map(|p| p.unwrap_or(0))
    };
    assert_eq!(shown.get(), 8080);

    set_port(&store, 7070);
    assert_eq!(shown.get(), 7070, "the cell's owner did not survive `pipe`");
}

/// The form that always worked, so the two stay together rather than drifting.
#[test]
fn several_sources_keep_reporting_too() {
    let path = TempPath::new("pipe_several");
    let store = StoreBuilder::new(path.path()).build().unwrap();

    let shown = {
        let state = Settings::new_with(&store).unwrap();
        (state.host(), state.port())
            .pipe()
            .map(|(h, p)| format!("{h}:{p}"))
    };
    assert_eq!(shown.get(), "localhost:8080");

    set_port(&store, 1234);
    assert_eq!(shown.get(), "localhost:1234");
}
