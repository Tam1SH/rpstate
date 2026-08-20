use amethystate::store::builder::StoreBuilder;
use amethystate::{LocalScope, amethystate};
use amethystate_core::test_utils::unique_path;
use std::cell::RefCell;
use std::rc::Rc;

mod common;
use common::shape;

#[amethystate(prefix = "typed")]
pub struct Typed {
    #[amestate(default = 8080u16)]
    pub port: u16,
}

fn store() -> amethystate::Store {
    StoreBuilder::new(unique_path("kv")).build().unwrap()
}

#[test]
fn raw_round_trip() {
    let store = store();
    let kv = store.kv();

    kv.set("theme", &"dark".to_string()).unwrap();
    assert_eq!(kv.get::<String>("theme").unwrap().as_deref(), Some("dark"));

    kv.remove("theme").unwrap();
    assert_eq!(kv.get::<String>("theme").unwrap(), None);
}

#[test]
fn a_cell_is_an_ordinary_reactive_cell() {
    let store = store();
    let kv = store.kv();

    let width = kv.namespace("ui").unwrap().cell("width", 800u32).unwrap();
    assert_eq!(width.get(), Some(800), "seeded with the default");

    let mut ui = LocalScope::new();
    let seen = Rc::new(RefCell::new(Vec::new()));
    let cap = Rc::clone(&seen);

    width
        .subscription_with()
        .local(&mut ui)
        .register(move |w: &Option<u32>| cap.borrow_mut().push(w.unwrap()));

    width.set(1024).unwrap();
    ui.drain();

    assert_eq!(*seen.borrow(), vec![1024]);
    assert_eq!(store.get::<u32>(["ui", "width"]).unwrap(), Some(1024));
}

#[test]
fn a_map_takes_a_key_set_that_is_not_known_up_front() {
    let store = store();
    let kv = store.kv();

    let flags = kv.map::<String, bool>("flags").unwrap();
    flags.insert("beta".into(), &true).unwrap();
    flags.insert("alpha".into(), &false).unwrap();

    assert_eq!(flags.keys().unwrap(), ["alpha", "beta"]);
}

#[test]
fn keys_are_sorted_and_scoped_to_the_prefix() {
    let store = store();
    let kv = store.kv();

    let ui = kv.namespace("ui").unwrap();
    ui.set("zoom", &2u8).unwrap();
    ui.set("theme", &"dark".to_string()).unwrap();
    kv.namespace("net")
        .unwrap()
        .set("host", &"localhost".to_string())
        .unwrap();

    assert_eq!(ui.keys().unwrap(), ["ui.theme", "ui.zoom"]);
}

/// A declared struct owns its prefix. Writing there through Kv would not merely
/// store the wrong thing: the field's subscription fails to decode and keeps
/// its old value, and the next startup fails outright reading the path back.
#[test]
fn writing_into_a_declared_prefix_is_refused() {
    let store = store();
    let kv = store.kv();

    let err = kv
        .namespace("typed")
        .unwrap()
        .set("port", &"not a number".to_string())
        .unwrap_err();
    insta::assert_snapshot!("kv_write_over_a_declared_field", shape(&err));

    let refused = [
        kv.namespace("typed")
            .unwrap()
            .cell("port", 1u16)
            .map(|_| ())
            .unwrap_err(),
        kv.namespace("typed").unwrap().remove("port").unwrap_err(),
        kv.map::<String, u8>("typed").map(|_| ()).unwrap_err(),
    ];

    for (way, err) in ["cell", "remove", "map"].into_iter().zip(refused) {
        insta::assert_snapshot!(format!("kv_{way}_over_a_declared_field"), shape(&err));
    }
}

#[test]
fn a_path_next_to_a_declared_prefix_is_allowed() {
    let store = store();
    let kv = store.kv();

    kv.namespace("typedish")
        .unwrap()
        .set("port", &1u16)
        .unwrap();
    assert_eq!(
        kv.namespace("typedish")
            .unwrap()
            .get::<u16>("port")
            .unwrap(),
        Some(1)
    );
}

/// Nothing connects a path to a type the way a struct field does, so asking for
/// two types at one path is caught rather than returning garbage.
#[test]
fn the_same_path_cannot_be_two_types() {
    let store = store();
    let kv = store.kv();

    let _width = kv.namespace("ui").unwrap().cell("width", 800u32).unwrap();
    let err = kv
        .namespace("ui")
        .unwrap()
        .cell("width", String::new())
        .unwrap_err();

    insta::assert_snapshot!("kv_asked_for_a_second_type", shape(&err));
}

#[test]
fn asking_for_the_same_path_and_type_twice_is_fine() {
    let store = store();
    let kv = store.kv();

    let a = kv.namespace("ui").unwrap().cell("width", 800u32).unwrap();
    let b = kv.namespace("ui").unwrap().cell("width", 800u32).unwrap();

    a.set(42).unwrap();
    assert_eq!(b.get(), Some(42), "both are views on the same path");
}

#[test]
fn values_survive_a_reopen() {
    let path = unique_path("kv_reopen");

    {
        let store = StoreBuilder::new(&path).build().unwrap();
        store
            .kv()
            .namespace("ui")
            .unwrap()
            .cell("width", 800u32)
            .unwrap()
            .set(1280)
            .unwrap();
        store.save_now().unwrap();
    }

    let store = StoreBuilder::new(&path).build().unwrap();
    assert_eq!(
        store
            .kv()
            .namespace("ui")
            .unwrap()
            .cell("width", 800u32)
            .unwrap()
            .get(),
        Some(1280)
    );
}
