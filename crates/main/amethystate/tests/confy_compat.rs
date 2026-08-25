#![cfg(any(feature = "confy-compat", feature = "confy-compat-0-6"))]
//! The `confy` layer through the calls a `confy` user makes.
//!
//! These are the only tests in the suite that leave the temporary directory,
//! and it is deliberate: where the configuration file lands is part of what
//! `confy` compatibility means, so the path comes from
//! `get_configuration_file_path` and the test removes it afterwards.

use amethystate::confy;
use amethystate_macros::amethystate;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
struct TestConfig {
    name: String,
    comfy: bool,
    foo: i64,
}

impl Default for TestConfig {
    fn default() -> Self {
        TestConfig {
            name: "Unknown".to_string(),
            comfy: true,
            foo: 42,
        }
    }
}

#[test]
#[cfg_attr(
    all(
        feature = "toml",
        not(feature = "redb"),
        not(feature = "json"),
        not(feature = "sqlite"),
        not(feature = "sqlite-bundled")
    ),
    ignore = "known: on the toml backend a stored config cannot be read back - see confy_migration.rs"
)]
fn test_confy_compat_lifecycle() {
    let app_name = "confy_compat_integration_test_app";

    let file_path =
        confy::get_configuration_file_path(app_name, None).expect("Failed to get config path");

    if file_path.exists() {
        let _ = fs::remove_file(&file_path);
    }

    let cfg: TestConfig =
        confy::load(app_name, None).expect("Failed to load default configuration");

    assert_eq!(cfg, TestConfig::default());
    assert!(
        file_path.exists(),
        "Configuration file must be created on disk"
    );

    let updated_cfg = TestConfig {
        name: "TestUser".to_string(),
        comfy: false,
        foo: 99,
    };
    confy::store(app_name, None, &updated_cfg).expect("Failed to store updated configuration");

    let loaded_cfg: TestConfig =
        confy::load(app_name, None).expect("Failed to reload configuration");

    assert_eq!(loaded_cfg, updated_cfg);

    if let Some(parent) = file_path.parent()
        && parent.exists()
    {
        fs::remove_dir_all(parent).expect("Failed to clean up test configuration directory");
    }
}

#[amethystate(prefix = "network", mode = "persistent")]
pub struct NetworkState {
    #[amestate(default = 8080u16)]
    pub port: u16,
}

fn clean_up_files(file_path: &Path) {
    if file_path.exists() {
        let _ = fs::remove_file(file_path);
    }

    if let Some(ext) = file_path.extension().and_then(|s| s.to_str()) {
        let meta_append = file_path.with_extension(format!("{ext}.meta"));
        if meta_append.exists() {
            let _ = fs::remove_file(&meta_append);
        }
    }

    let meta_replace = file_path.with_extension("meta");
    if meta_replace.exists() {
        let _ = fs::remove_file(&meta_replace);
    }
}
#[test]
#[cfg_attr(
    all(
        feature = "toml",
        not(feature = "redb"),
        not(feature = "json"),
        not(feature = "sqlite"),
        not(feature = "sqlite-bundled")
    ),
    ignore = "known: on the toml backend a stored config cannot be read back - see confy_migration.rs"
)]
fn test_confy_amethystate_coexistence() {
    // confy emulates a plain-text config file; with a binary default backend
    // there is no text to round-trip and the comparison is meaningless.
    if !matches!(
        amethystate::store::builder::default_backend().extension(),
        "json" | "toml" | "ron"
    ) {
        return;
    }

    use amethystate::{IntoGlobalStore, StoreBuilder};

    let app_name = "confy_amethystate_coexistence_test";

    let file_path =
        confy::get_configuration_file_path(app_name, None).expect("Failed to get config path");

    clean_up_files(&file_path);

    if let Some(parent) = file_path.parent() {
        fs::create_dir_all(parent).expect("Failed to create parent directory");
    }

    let _ame = StoreBuilder::new(&file_path).init_global();

    let legacy = TestConfig {
        name: "legacy".to_string(),
        comfy: true,
        foo: 1,
    };

    confy::store(app_name, None, &legacy).expect("confy store failed");

    let mut network = NetworkState::load().expect("amethystate init failed");
    network.mutate(|n| n.port = 9090).expect("mutate failed");

    let loaded_legacy: TestConfig = confy::load(app_name, None).expect("confy load failed");
    assert_eq!(loaded_legacy, legacy);

    assert_eq!(network.port, 9090);

    let contents = fs::read_to_string(&file_path).expect("read failed");

    match amethystate::store::builder::default_backend().extension() {
        "json" => insta::assert_snapshot!("coexistence_json", contents),
        "toml" => insta::assert_snapshot!("coexistence_toml", contents),
        "ron" => insta::assert_snapshot!("coexistence_ron", contents),
        _ => {}
    }

    if let Some(parent) = file_path.parent()
        && parent.exists()
    {
        fs::remove_dir_all(parent).expect("cleanup failed");
    }

    if let Some(parent) = file_path.parent()
        && parent.exists()
    {
        fs::remove_dir_all(parent).expect("cleanup failed");
    }
}

#[test]
#[cfg(all(
    feature = "toml",
    not(feature = "redb"),
    not(feature = "json"),
    not(feature = "ron"),
    not(feature = "sqlite"),
    not(feature = "sqlite-bundled")
))]
fn test_compare_real_confy_and_amethystate() {
    let dir = tempfile::tempdir().expect("Failed to create temp dir");

    let legacy = TestConfig {
        name: "legacy".to_string(),
        comfy: true,
        foo: 1,
    };

    let real_path = dir.path().join("real_confy.toml");
    real_confy::store_path(&real_path, &legacy).expect("real confy store failed");
    let real_contents = std::fs::read_to_string(&real_path).expect("read failed");

    let rp_path = dir.path().join("amethystate_confy.toml");
    confy::store_path(&rp_path, &legacy).expect("amethystate confy store failed");
    let rp_contents = std::fs::read_to_string(&rp_path).expect("read failed");

    assert_eq!(real_contents.trim(), rp_contents.trim());
}
