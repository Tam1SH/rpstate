use amethystate::migration::fields::Role;
use amethystate::store::InspectorBackend;
use amethystate::store::builder::{Backend, StoreBuilder};
use amethystate::{ReactiveMap, amethystate};
use amethystate_core::test_utils::TempPath;
use amethystate_test_macros::backends;
use std::path::Path;

#[amethystate]
pub struct Instance {
    #[amestate(default = 8080u16)]
    pub port: u16,

    #[amestate(default = None)]
    pub note: Option<String>,

    #[amestate(default = {})]
    pub widths: ReactiveMap<String, u64>,
}

fn inspecting(backend: Backend, at: &Path) -> Box<dyn InspectorBackend> {
    let config = amethystate::StoreConfig::new(at);

    match backend {
        #[cfg(feature = "redb")]
        Backend::Redb => Box::new(
            amethystate::stores::RedbStore::open(config, Default::default())
                .unwrap()
                .0,
        ),
        #[cfg(feature = "sqlite")]
        Backend::Sqlite => Box::new(
            amethystate::stores::SqliteStore::open(config, Default::default())
                .unwrap()
                .0,
        ),
        #[cfg(feature = "json")]
        Backend::Json => Box::new(
            amethystate::stores::JsonStore::open(config, Default::default())
                .unwrap()
                .0,
        ),
        #[cfg(feature = "toml")]
        Backend::Toml => Box::new(
            amethystate::stores::TomlStore::open(config, Default::default())
                .unwrap()
                .0,
        ),
        #[cfg(feature = "ron")]
        Backend::Ron => Box::new(
            amethystate::stores::RonStore::open(config, Default::default())
                .unwrap()
                .0,
        ),
    }
}

#[backends(all)]
fn the_places_it_claims_are_recorded_where_it_was_built(backend: Backend) {
    let path = TempPath::new("runtime_namespace_schema");

    {
        let store = StoreBuilder::new(path.path())
            .backend(backend)
            .build()
            .unwrap();

        let _one = Instance::new(&store, ["instances", "a"]).unwrap();
        store.save_now().unwrap();
    }

    let recorded = inspecting(backend, path.path())
        .get_schema_snapshots()
        .unwrap();

    let (_, snapshot) = recorded
        .iter()
        .find(|(prefix, _)| prefix == "instances.a")
        .expect("nothing recorded the places a struct claimed at a runtime namespace");

    assert_eq!(snapshot.struct_name.as_deref(), Some("Instance"));

    let mut named: Vec<&str> = snapshot.fields.iter().map(|f| f.name.as_str()).collect();
    named.sort();
    assert_eq!(named, ["note", "port", "widths"]);

    let field = |name: &str| {
        &snapshot
            .fields
            .iter()
            .find(|f| f.name.as_str() == name)
            .unwrap()
            .shape
    };

    assert_eq!(field("port").role, Role::Field);
    assert!(field("note").optional);
    assert_eq!(field("widths").role, Role::Map);
}

#[amethystate(prefix = "holder")]
pub struct Holder {
    #[amestate(nested, flatten)]
    pub flat: Part,

    #[amestate(nested)]
    pub under: Part,
}

#[amethystate]
pub struct Part {
    #[amestate(default = 1u32)]
    pub thing: u32,
}

#[backends(all)]
fn a_nested_part_records_nothing_of_its_own(backend: Backend) {
    let path = TempPath::new("runtime_namespace_nested");

    {
        let store = StoreBuilder::new(path.path())
            .backend(backend)
            .build()
            .unwrap();

        let _held = Holder::new_with(&store).unwrap();
        store.save_now().unwrap();
    }

    let recorded = inspecting(backend, path.path())
        .get_schema_snapshots()
        .unwrap();

    let (_, snapshot) = recorded
        .iter()
        .find(|(prefix, _)| prefix == "holder")
        .expect("the holder records its own places");

    assert_eq!(
        snapshot.struct_name.as_deref(),
        Some("Holder"),
        "the flattened part is built at the holder's own path, and must not \
         record its places there under its own name"
    );

    assert!(
        !recorded.iter().any(|(prefix, _)| prefix == "holder.under"),
        "a nested part's places are already in its holder's record: {recorded:?}"
    );
}

#[backends(all)]
fn two_namespaces_are_recorded_apart(backend: Backend) {
    let path = TempPath::new("runtime_namespace_two");

    {
        let store = StoreBuilder::new(path.path())
            .backend(backend)
            .build()
            .unwrap();

        let _one = Instance::new(&store, ["instances", "a"]).unwrap();
        let _other = Instance::new(&store, ["instances", "b"]).unwrap();
        store.save_now().unwrap();
    }

    let recorded = inspecting(backend, path.path())
        .get_schema_snapshots()
        .unwrap();

    let under: Vec<&str> = recorded
        .iter()
        .map(|(prefix, _)| prefix.as_str())
        .filter(|prefix| prefix.starts_with("instances."))
        .collect();

    assert_eq!(under, ["instances.a", "instances.b"]);
}
