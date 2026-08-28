//! The store holds what the type said, not only the code that opened it.
//!
//! Role and optionality are read off the type at compile time; this is the
//! other half - that they reach the schema snapshot in the file, survive a
//! close, and come back the same. Until they do, only a running program that
//! was built from the same source knows the shape, which is the thing the
//! migration track is trying to stop being true.

#[cfg(feature = "redb")]
use amethystate::migration::fields::Role;
#[cfg(feature = "redb")]
use amethystate::observability::InspectorBackend;
use amethystate::store::builder::StoreBuilder;
#[cfg(feature = "redb")]
use amethystate::store::meta::{SchemaSnapshot, StoredFieldEntry};
use amethystate::{ReactiveMap, amethystate};
use amethystate_core::test_utils::TempPath;

#[amethystate(prefix = "ondisk", version = 1)]
pub struct Recorded {
    #[amestate(default = 8080)]
    pub port: u16,

    #[amestate(default = None)]
    pub note: Option<String>,

    #[amestate(default = {})]
    pub widths: ReactiveMap<String, u64>,

    #[amestate(nested)]
    pub net: NetPart,
}

#[amethystate]
pub struct NetPart {
    #[amestate(default = "127.0.0.1".to_string())]
    pub host: String,

    #[amestate(default = None)]
    pub proxy: Option<String>,
}

/// Recording the shape reads the file's own snapshot back and writes it again,
/// so an engine whose codec cannot carry some part of it fails on the second
/// open - which is how a `Role` that serialises as a variant took `ron` out
/// entirely, since a `ron::value::Value` has nowhere to put one.
///
/// This is the half that runs on every engine. Reading the recorded shape apart
/// needs the inspector, and that is redb-only below.
#[test]
fn the_recorded_shape_survives_this_engines_codec() {
    let path = TempPath::new("shape_codec");

    {
        let store = StoreBuilder::new(path.path()).build().unwrap();
        let _state = Recorded::new_with(&store).unwrap();
        store.save_now().unwrap();
    }

    StoreBuilder::new(path.path())
        .build()
        .expect("the shape this engine wrote must read back through the same codec");
}

/// Written by one store, read by another that only has the file - which is the
/// claim being tested.
#[cfg(feature = "redb")]
fn snapshot_of(path: &TempPath) -> SchemaSnapshot {
    {
        let store = StoreBuilder::new(path.path()).build().unwrap();
        let _state = Recorded::new_with(&store).unwrap();
        store.save_now().unwrap();
    }

    let inspector = amethystate::stores::RedbStore::open(
        amethystate::StoreConfig::new(path.path()),
        Default::default(),
    )
    .unwrap()
    .0;

    inspector
        .get_schema_snapshots()
        .unwrap()
        .into_iter()
        .find(|(prefix, _)| prefix.contains("ondisk"))
        .map(|(_, snapshot)| snapshot)
        .expect("no snapshot was written for the declared prefix")
}

#[cfg(feature = "redb")]
fn field<'a>(fields: &'a [StoredFieldEntry], name: &str) -> &'a StoredFieldEntry {
    fields
        .iter()
        .find(|f| f.name == name)
        .unwrap_or_else(|| panic!("no field named {name} in the snapshot"))
}

#[cfg(feature = "redb")]
#[test]
fn the_snapshot_records_what_each_path_is() {
    let path = TempPath::new("shape_on_disk");
    let snapshot = snapshot_of(&path);

    let port = &field(&snapshot.fields, "port").shape;
    assert_eq!(port.role, Role::Field);
    assert!(!port.optional);

    let note = &field(&snapshot.fields, "note").shape;
    assert_eq!(note.role, Role::Field);
    assert!(note.optional, "a path that may hold nothing says so");

    assert_eq!(field(&snapshot.fields, "widths").shape.role, Role::Map);
}

#[cfg(feature = "redb")]
#[test]
fn a_level_records_what_lives_under_it() {
    let path = TempPath::new("shape_on_disk_nested");
    let snapshot = snapshot_of(&path);

    let net = &field(&snapshot.fields, "net").shape;
    assert_eq!(net.role, Role::Node);
    assert!(!net.optional);

    assert!(
        field(&net.children, "proxy").shape.optional,
        "the shape goes all the way down, not one level"
    );
    assert_eq!(field(&net.children, "host").type_name, "String");
}

/// A leaf writes no `children` key, because most paths are leaves and a
/// document a person reads should not carry an empty list on every one.
#[cfg(feature = "redb")]
#[test]
fn a_leaf_does_not_write_an_empty_list_of_children() {
    let path = TempPath::new("shape_on_disk_leaf");
    let snapshot = snapshot_of(&path);

    let rendered = serde_json::to_string(&field(&snapshot.fields, "port")).unwrap();
    assert_eq!(
        rendered,
        r#"{"name":"port","type_name":"u16","shape":{"role":"field","optional":false}}"#
    );
}
