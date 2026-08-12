use amethystate::errors::WriteError;
use amethystate::store::Store;
use amethystate::store::builder::StoreBuilder;
use amethystate::{LocalScope, amethystate};
use amethystate_core::test_utils::unique_path;
use std::cell::RefCell;
use std::rc::Rc;

#[amethystate(prefix = "typed")]
pub struct Typed {
    #[amestate(default = 8080u16)]
    pub port: u16,
}

fn store() -> impl Store {
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

    let width = kv.cell("ui.width", 800u32).unwrap();
    assert_eq!(width.get(), 800, "seeded with the default");

    let mut ui = LocalScope::new();
    let seen = Rc::new(RefCell::new(Vec::new()));
    let cap = Rc::clone(&seen);

    width
        .subscription_with()
        .local(&mut ui)
        .register(move |w: &u32| cap.borrow_mut().push(*w));

    width.set(1024).unwrap();
    ui.drain();

    assert_eq!(*seen.borrow(), vec![1024]);
    assert_eq!(store.get::<u32>("ui.width").unwrap(), Some(1024));
}

#[test]
fn a_map_takes_a_key_set_that_is_not_known_up_front() {
    let store = store();
    let kv = store.kv();

    let flags = kv.map::<String, bool>("flags").unwrap();
    flags.set_or_create("beta".into(), &true).unwrap();
    flags.set_or_create("alpha".into(), &false).unwrap();

    assert_eq!(flags.keys().unwrap(), ["alpha", "beta"]);
}

#[test]
fn keys_are_sorted_and_scoped_to_the_prefix() {
    let store = store();
    let kv = store.kv();

    kv.set("ui.zoom", &2u8).unwrap();
    kv.set("ui.theme", &"dark".to_string()).unwrap();
    kv.set("net.host", &"localhost".to_string()).unwrap();

    assert_eq!(kv.keys("ui").unwrap(), ["ui.theme", "ui.zoom"]);
}

/// A declared struct owns its prefix. Writing there through Kv would not merely
/// store the wrong thing: the field's subscription fails to decode and keeps
/// its old value, and the next startup fails outright reading the path back.
#[test]
fn writing_into_a_declared_prefix_is_refused() {
    let store = store();
    let kv = store.kv();

    let err = kv
        .set("typed.port", &"not a number".to_string())
        .unwrap_err();
    assert!(matches!(err, WriteError::SchemaOwned { .. }), "got {err:?}");

    assert!(kv.cell("typed.port", 1u16).is_err());
    assert!(kv.remove("typed.port").is_err());
    assert!(kv.map::<String, u8>("typed").is_err());
}

#[test]
fn a_path_next_to_a_declared_prefix_is_allowed() {
    let store = store();
    let kv = store.kv();

    kv.set("typedish.port", &1u16).unwrap();
    assert_eq!(kv.get::<u16>("typedish.port").unwrap(), Some(1));
}

/// Nothing connects a path to a type the way a struct field does, so asking for
/// two types at one path is caught rather than returning garbage.
#[test]
fn the_same_path_cannot_be_two_types() {
    let store = store();
    let kv = store.kv();

    let _width = kv.cell("ui.width", 800u32).unwrap();
    let err = kv.cell("ui.width", String::new()).unwrap_err();

    assert!(
        matches!(err, WriteError::TypeMismatch { .. }),
        "got {err:?}"
    );
}

#[test]
fn asking_for_the_same_path_and_type_twice_is_fine() {
    let store = store();
    let kv = store.kv();

    let a = kv.cell("ui.width", 800u32).unwrap();
    let b = kv.cell("ui.width", 800u32).unwrap();

    a.set(42).unwrap();
    assert_eq!(b.get(), 42, "both are views on the same path");
}

#[test]
fn values_survive_a_reopen() {
    let path = unique_path("kv_reopen");

    {
        let store = StoreBuilder::new(&path).build().unwrap();
        store
            .kv()
            .cell("ui.width", 800u32)
            .unwrap()
            .set(1280)
            .unwrap();
        store.save_now().unwrap();
    }

    let store = StoreBuilder::new(&path).build().unwrap();
    assert_eq!(store.kv().cell("ui.width", 800u32).unwrap().get(), 1280);
}
