use amethystate::store::builder::StoreBuilder;
use amethystate::{LocalScope, amethystate};
use amethystate_core::path::StorePath;
use amethystate_core::test_utils::unique_path;
use std::cell::RefCell;
use std::rc::Rc;

mod common;
use common::{per_engine, shape};

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

    let width = kv.namespace("ui").cell("width", 800u32).unwrap();
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

    let ui = kv.namespace("ui");
    ui.set("zoom", &2u8).unwrap();
    ui.set("theme", &"dark".to_string()).unwrap();
    kv.namespace("net")
        .set("host", &"localhost".to_string())
        .unwrap();

    let keys = ui.keys().unwrap();
    assert_eq!(
        keys.iter().map(StorePath::as_str).collect::<Vec<_>>(),
        ["ui.theme", "ui.zoom"]
    );
}

/// A declared path is the schema's. Writing there through Kv would not merely
/// store the wrong thing: the field's subscription fails to decode and keeps
/// its old value, and the next startup fails outright reading the path back.
///
/// The last of the four is the other direction - `typed` is not itself
/// declared, but a map there would take the level `typed.port` lives on.
#[test]
fn writing_into_a_declared_path_is_refused() {
    let store = store();
    let kv = store.kv();

    let err = kv
        .namespace("typed")
        .set("port", &"not a number".to_string())
        .unwrap_err();
    insta::assert_snapshot!("kv_write_over_a_declared_field", shape(&err));

    let refused = [
        kv.namespace("typed")
            .cell("port", 1u16)
            .map(|_| ())
            .unwrap_err(),
        kv.namespace("typed").remove("port").unwrap_err(),
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

    kv.namespace("typedish").set("port", &1u16).unwrap();
    assert_eq!(
        kv.namespace("typedish").get::<u16>("port").unwrap(),
        Some(1)
    );
}

/// A schema owns the paths it declared, not the prefix they sit under. Settings
/// are extended from outside all the time - a plugin, a theme, a person editing
/// the file - and none of that collides with a declared field.
#[test]
fn a_path_beside_a_declared_field_is_allowed() {
    let store = store();
    let typed = store.kv().namespace("typed");

    typed.set("colour", &"blue".to_string()).unwrap();
    typed.namespace("myplugin").set("enabled", &true).unwrap();

    assert_eq!(
        typed.get::<String>("colour").unwrap().as_deref(),
        Some("blue")
    );
    assert_eq!(
        typed.namespace("myplugin").get::<bool>("enabled").unwrap(),
        Some(true)
    );

    assert!(
        typed.set("port", &1u16).is_err(),
        "the declared path itself is still owned"
    );
}

/// Nothing connects a path to a type the way a struct field does, so asking for
/// two types at one path is caught rather than returning garbage - by the read,
/// which is the only thing that can say what is actually stored there. The
/// report names the value, not just the two type names.
#[test]
fn the_same_path_cannot_be_two_types() {
    let store = store();
    let kv = store.kv();

    let _width = kv.namespace("ui").cell("width", 800u32).unwrap();
    let err = kv.namespace("ui").cell("width", String::new()).unwrap_err();

    insta::assert_snapshot!(per_engine("kv_asked_for_a_second_type"), shape(&err));
}

#[test]
fn asking_for_the_same_path_and_type_twice_is_fine() {
    let store = store();
    let kv = store.kv();

    let a = kv.namespace("ui").cell("width", 800u32).unwrap();
    let b = kv.namespace("ui").cell("width", 800u32).unwrap();

    a.set(42).unwrap();
    assert_eq!(b.get(), Some(42), "both are views on the same path");
}

/// The same, for a type that is not a primitive. Worth its own case because a
/// registry of type *names* got this wrong - `alloc::string::String` never
/// matched `String` - and refused the second ask for anything but a primitive.
#[test]
fn the_same_path_and_type_twice_is_fine_for_a_type_that_is_not_a_primitive() {
    let store = store();
    let kv = store.kv();

    kv.cell("text", String::new()).unwrap();
    let again = kv.cell("text", String::new());
    assert!(
        again.is_ok(),
        "`String` was refused the second time: {:?}",
        again.err()
    );
}

#[test]
fn values_survive_a_reopen() {
    let path = unique_path("kv_reopen");

    {
        let store = StoreBuilder::new(&path).build().unwrap();
        store
            .kv()
            .namespace("ui")
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
            .cell("width", 800u32)
            .unwrap()
            .get(),
        Some(1280)
    );
}
