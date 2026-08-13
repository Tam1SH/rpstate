use amethystate::store::builder::StoreBuilder;
use amethystate::{AmeType, ReactiveMap, amethystate};
use amethystate_core::test_utils::unique_path;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, AmeType)]
pub struct Limits {
    pub warning: u64,
}

#[amethystate(prefix = "dbg")]
pub struct Settings {
    #[amestate(default = 8080)]
    pub port: u16,

    #[amestate(default = "dark".to_string())]
    pub theme: String,

    #[amestate(default = { "cpu": Limits { warning: 70 } })]
    pub limits: ReactiveMap<String, Limits>,
}

/// Deliberately not `Debug`: the framework must not demand it.
#[derive(Clone, Default, PartialEq, Serialize, Deserialize, AmeType)]
pub struct Opaque {
    pub inner: u8,
}

#[amethystate(prefix = "dbg_opaque")]
pub struct HasOpaque {
    #[amestate(default = 1)]
    pub port: u16,

    #[amestate(default = Opaque { inner: 7 })]
    pub opaque: Opaque,
}

fn assert_debug<T: std::fmt::Debug>() {}

#[test]
fn a_field_type_need_not_be_printable() {
    let store = StoreBuilder::new(unique_path("dbg_opaque"))
        .build()
        .unwrap();
    let state = HasOpaque::new_with(&store).unwrap();

    assert_eq!(state.opaque().get().inner, 7);
    assert_eq!(state.port().get(), 1);
}

#[test]
fn a_printable_struct_still_gets_its_impl() {
    assert_debug::<Settings>();
}

fn settings(tag: &str) -> Settings {
    let store = StoreBuilder::new(unique_path(tag)).build().unwrap();
    Settings::new_with(&store).unwrap()
}

#[test]
fn a_field_shows_its_path_and_value() {
    let state = settings("dbg_field");
    let shown = format!("{:?}", state.port());

    assert!(shown.contains("Field"), "{shown}");
    assert!(shown.contains("dbg.port"), "{shown}");
    assert!(shown.contains("8080"), "{shown}");
}

#[test]
fn a_field_shows_the_current_value_not_the_default() {
    let state = settings("dbg_current");
    state.port().set(9090).unwrap();

    let shown = format!("{:?}", state.port());
    assert!(shown.contains("9090"), "{shown}");
    assert!(!shown.contains("8080"), "{shown}");
}

#[test]
fn a_state_struct_shows_every_field_by_name() {
    let state = settings("dbg_struct");
    let shown = format!("{state:?}");

    assert!(shown.starts_with("Settings {"), "{shown}");
    for expected in ["port", "8080", "theme", "dark", "limits", "cpu", "70"] {
        assert!(shown.contains(expected), "missing {expected} in {shown}");
    }
}

#[test]
fn the_instance_id_stays_out_of_the_output() {
    let state = settings("dbg_no_id");
    let shown = format!("{state:?}");

    assert!(
        !shown.contains("__amethystate_instance_id"),
        "internals leaked: {shown}"
    );
}
