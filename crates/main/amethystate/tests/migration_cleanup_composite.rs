use amethystate::store::builder::StoreBuilder;
use amethystate::{AmeData, ReactiveMap, migrate};
use amethystate_core::path::StorePath;
use amethystate_core::test_utils::TempPath;
use amethystate_macros::amethystate;

mod dropmap_v1 {
    use super::*;

    #[amethystate(prefix = "dropmap", version = 1)]
    pub struct DropMap {
        #[amestate(default = 1u32)]
        pub kept: u32,

        pub cache: ReactiveMap<String, u32>,
    }
}

#[amethystate(prefix = "dropmap", version = 2)]
pub struct DropMap {
    #[amestate(default = 1u32)]
    pub kept: u32,
}

#[migrate]
fn migrate_dropmap_v1_to_v2(
    old: AmeData<dropmap_v1::DropMap>,
) -> amethystate::MigrationResult<AmeData<DropMap>> {
    Ok(AmeData::<DropMap> { kept: old.kept })
}

/// A `ReactiveMap` field removed in v2: the generated cleanup deletes the
/// map's own path, which holds nothing, so every entry survives.
#[test]
#[ignore = "known: migration cleanup deletes a map's node, not its entries - see TODO.md"]
fn dropping_a_reactive_map_field_removes_its_entries() {
    let path = TempPath::new("dropmap");

    {
        let store = StoreBuilder::new(path.path()).build().unwrap();
        let v1 = dropmap_v1::DropMap::new_with(&store).unwrap();
        v1.cache().insert("alpha".into(), &7u32).unwrap();
        v1.cache().insert("beta".into(), &9u32).unwrap();
        store.flush_prefix(StorePath::root()).unwrap();

        assert_eq!(
            store.get::<u32>(["dropmap", "cache", "alpha"]).unwrap(),
            Some(7)
        );
    }

    let (store, _report) = StoreBuilder::new(path.path())
        .migrations(|m| {
            m.collect_codegen();
        })
        .build_with_report()
        .unwrap();

    let _v2 = DropMap::new_with(&store).unwrap();

    assert_eq!(
        store.get::<u32>(["dropmap", "cache", "alpha"]).unwrap(),
        None,
        "entries of a dropped map are cleaned up"
    );
    assert_eq!(
        store.get::<u32>(["dropmap", "cache", "beta"]).unwrap(),
        None
    );
}

mod dropscalar_v1 {
    use super::*;

    #[amethystate(prefix = "dropscalar", version = 1)]
    pub struct DropScalar {
        #[amestate(default = 1u32)]
        pub kept: u32,

        #[amestate(default = 5u32)]
        pub gone: u32,
    }
}

#[amethystate(prefix = "dropscalar", version = 2)]
pub struct DropScalar {
    #[amestate(default = 1u32)]
    pub kept: u32,
}

#[migrate]
fn migrate_dropscalar_v1_to_v2(
    old: AmeData<dropscalar_v1::DropScalar>,
) -> amethystate::MigrationResult<AmeData<DropScalar>> {
    Ok(AmeData::<DropScalar> { kept: old.kept })
}

/// Control: the same drop of a plain scalar field is cleaned up, so the map
/// case above fails on the map, not on migration cleanup in general.
#[test]
fn dropping_a_scalar_field_removes_its_value() {
    let path = TempPath::new("dropscalar");

    {
        let store = StoreBuilder::new(path.path()).build().unwrap();
        let v1 = dropscalar_v1::DropScalar::new_with(&store).unwrap();
        v1.gone().set(42).unwrap();
        store.flush_prefix(StorePath::root()).unwrap();
        assert_eq!(store.get::<u32>(["dropscalar", "gone"]).unwrap(), Some(42));
    }

    let (store, _report) = StoreBuilder::new(path.path())
        .migrations(|m| {
            m.collect_codegen();
        })
        .build_with_report()
        .unwrap();

    let _v2 = DropScalar::new_with(&store).unwrap();

    assert_eq!(store.get::<u32>(["dropscalar", "gone"]).unwrap(), None);
}

#[amethystate]
pub struct LegacySub {
    #[amestate(default = 3u32)]
    pub inner: u32,
}

mod dropnested_v1 {
    use super::*;

    #[amethystate(prefix = "dropnested", version = 1)]
    pub struct DropNested {
        #[amestate(default = 1u32)]
        pub kept: u32,

        #[amestate(nested)]
        pub legacy: LegacySub,
    }
}

#[amethystate(prefix = "dropnested", version = 2)]
pub struct DropNested {
    #[amestate(default = 1u32)]
    pub kept: u32,
}

#[migrate]
fn migrate_dropnested_v1_to_v2(
    old: AmeData<dropnested_v1::DropNested>,
) -> amethystate::MigrationResult<AmeData<DropNested>> {
    Ok(AmeData::<DropNested> { kept: old.kept })
}

/// A `#[amestate(nested)]` field removed in v2: cleanup deletes the branch
/// path, which on a flat backend holds nothing, so the sub-fields survive.
#[test]
#[ignore = "known: migration cleanup of a nested struct deletes the branch path only - see TODO.md"]
fn dropping_a_nested_struct_field_removes_its_leaves() {
    let path = TempPath::new("dropnested");

    {
        let store = StoreBuilder::new(path.path()).build().unwrap();
        let v1 = dropnested_v1::DropNested::new_with(&store).unwrap();
        v1.legacy().inner().set(77).unwrap();
        store.flush_prefix(StorePath::root()).unwrap();
        assert_eq!(
            store.get::<u32>(["dropnested", "legacy", "inner"]).unwrap(),
            Some(77)
        );
    }

    let (store, _report) = StoreBuilder::new(path.path())
        .migrations(|m| {
            m.collect_codegen();
        })
        .build_with_report()
        .unwrap();

    let _v2 = DropNested::new_with(&store).unwrap();

    assert_eq!(
        store.get::<u32>(["dropnested", "legacy", "inner"]).unwrap(),
        None,
        "leaves of a dropped nested struct are cleaned up"
    );
}
