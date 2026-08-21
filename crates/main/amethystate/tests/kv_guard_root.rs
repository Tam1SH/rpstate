use amethystate::amethystate;
use amethystate::store::builder::StoreBuilder;
use amethystate_core::path::StorePath;
use amethystate_core::test_utils::TempPath;

mod common;
use common::shape;

#[amethystate(as_root)]
pub struct RootConfig {
    #[amestate(default = 1280u32)]
    pub width: u32,
}

#[amethystate(prefix = "guarded")]
pub struct GuardedConfig {
    #[amestate(default = 1280u32)]
    pub width: u32,
}

/// Control: `Kv` refuses a path a prefixed struct owns.
#[test]
fn kv_refuses_a_path_owned_by_a_prefixed_struct() {
    let path = TempPath::new("kv_guard_prefixed");
    let store = StoreBuilder::new(path.path()).build().unwrap();
    let _cfg = GuardedConfig::new_with(&store).unwrap();

    let err = store
        .kv()
        .namespace("guarded")
        .set("width", &"oops".to_string())
        .unwrap_err();

    insta::assert_snapshot!("kv_write_under_a_declared_prefix", shape(&err));
}

/// A root struct's fields sit at bare paths, and the guard only compares
/// against a declared prefix, so `Kv` writes straight over them - including
/// with a type the field cannot decode.
#[test]
#[ignore = "known: Kv::guard does not cover as_root fields - see TODO.md"]
fn kv_refuses_a_path_owned_by_a_root_struct() {
    let path = TempPath::new("kv_guard_root");
    let store = StoreBuilder::new(path.path()).build().unwrap();
    let cfg = RootConfig::new_with(&store).unwrap();

    let err = store.kv().set("width", &"oops".to_string()).unwrap_err();

    insta::assert_snapshot!("kv_write_over_a_root_struct_field", shape(&err));

    assert_eq!(cfg.width().get(), 1280);
}

/// What the missing guard costs: the overwritten path no longer decodes, and
/// the next run fails to load the struct at all.
#[test]
#[ignore = "known: Kv::guard does not cover as_root fields - see TODO.md"]
fn a_root_field_survives_a_kv_write_of_another_type() {
    let path = TempPath::new("kv_guard_root_reopen");

    {
        let store = StoreBuilder::new(path.path()).build().unwrap();
        let _cfg = RootConfig::new_with(&store).unwrap();
        store.kv().set("width", &"oops".to_string()).ok();
        store.flush_prefix(StorePath::root()).unwrap();
    }

    let store = StoreBuilder::new(path.path()).build().unwrap();
    let cfg = RootConfig::new_with(&store);
    assert!(cfg.is_ok(), "the struct still loads after a reopen");
}
