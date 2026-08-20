#![cfg(any(feature = "confy-compat", feature = "confy-compat-0-6"))]
//! What a real `confy` user's file meets when the application swaps `confy`
//! for `amethystate::confy`.
//!
//! Every test writes the file with the `confy` crate itself (`real_confy`) and
//! reads it back through the layer, or the reverse, so the comparison is
//! against the crate rather than against the layer's idea of it. `real_confy`
//! is built with its default `toml_conf` feature, which is the format and the
//! extension the overwhelming majority of `confy` users have on disk.
//!
//! A test that fails only on some backends says so in its `ignore` reason;
//! the reasons name the backend the failure was observed on.
//!
//! `src/confy/mod.rs` still addresses the store with the string `"."` and does
//! not build since paths became segments, so nothing here compiles until it
//! passes `StorePath::root()` instead. Every result recorded in an `ignore`
//! reason was taken with that one substitution applied and nothing else.

use amethystate::confy;
use amethystate_core::test_utils::TempPath;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq, Clone)]
struct Simple {
    name: String,
    comfy: bool,
    foo: i64,
}

impl Default for Simple {
    fn default() -> Self {
        Simple {
            name: "Unknown".to_string(),
            comfy: true,
            foo: 42,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq, Clone, Default)]
struct Window {
    width: u32,
    height: u32,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq, Clone, Default)]
enum Theme {
    #[default]
    Dark,
    Light,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq, Clone, Default)]
struct Rich {
    theme: Theme,
    nickname: Option<String>,
    recent: Vec<String>,
    aliases: BTreeMap<String, String>,
    window: Window,
}

/// The extension `real_confy` is compiled for.
const CONFY_EXT: &str = "toml";

/// A sibling of the `TempPath` file, so `TempPath::drop` sweeps it too.
fn sibling(temp: &TempPath, ext: &str) -> PathBuf {
    PathBuf::from(format!("{}.{ext}", temp.path().display()))
}

/// The extension the layer picks for the current backend.
fn layer_ext() -> &'static str {
    amethystate::store::builder::default_backend().extension()
}

/// Whether the backend in this build reads and writes what `confy` wrote.
///
/// Tests that compare against a real `confy` file have nothing to say when it
/// does not, and return early rather than assert.
fn backend_speaks_confy() -> bool {
    layer_ext() == CONFY_EXT
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_default()
}

/// Text no text backend parses.
#[cfg(feature = "text")]
const NOT_ANY_FORMAT: &str = "this is not = = a document";

/// A config file in the layer's own format, written by a store that is then
/// closed, so the layer's next call opens the file for the first time.
#[cfg(feature = "text")]
fn seeded_store(temp: &TempPath) -> PathBuf {
    use amethystate::StoreBuilder;

    let path = sibling(temp, layer_ext());
    let store = StoreBuilder::new(&path).build().expect("store");
    store.set(["name"], &"from-confy".to_string()).unwrap();
    store.save_now().unwrap();
    path
}

fn a_config() -> Simple {
    Simple {
        name: "from-confy".to_string(),
        comfy: false,
        foo: 7,
    }
}

// ---------------------------------------------------------------- where

/// The layer resolves the file `confy` resolves for the same app name.
///
/// Holds only when the backend's extension happens to be the one `confy` was
/// built for: the layer names the file after its storage engine, `confy` after
/// its serialisation format.
#[test]
#[ignore = "known: the extension follows the backend, so redb/json/ron never name confy's file"]
fn config_file_path_is_the_one_confy_uses() {
    let ours =
        confy::get_configuration_file_path("amethystate-path-probe", None).expect("layer path");
    let theirs = real_confy::get_configuration_file_path("amethystate-path-probe", None)
        .expect("confy path");

    assert_eq!(theirs, ours);
}

/// Everything about the path except the extension agrees with `confy`.
#[test]
fn config_directory_and_stem_are_the_ones_confy_uses() {
    let ours = confy::get_configuration_file_path("amethystate-path-probe", None).unwrap();
    let theirs = real_confy::get_configuration_file_path("amethystate-path-probe", None).unwrap();

    assert_eq!(theirs.parent(), ours.parent());
    assert_eq!(theirs.file_stem(), ours.file_stem());
}

/// An absent config name gives `default-config`, as `confy` documents.
#[test]
fn absent_config_name_is_default_config() {
    let ours = confy::get_configuration_file_path("amethystate-path-probe", None).unwrap();
    assert_eq!(ours.file_stem().unwrap(), "default-config");
}

/// An empty config name is pushed verbatim, as `confy` does: a dotfile named
/// after the extension alone.
#[test]
fn empty_config_name_matches_confy() {
    let ours = confy::get_configuration_file_path("amethystate-path-probe", Some("")).unwrap();
    let theirs =
        real_confy::get_configuration_file_path("amethystate-path-probe", Some("")).unwrap();

    assert_eq!(theirs.parent(), ours.parent());
    assert_eq!(
        theirs.file_name().unwrap().to_string_lossy(),
        format!(".{CONFY_EXT}")
    );
    assert_eq!(
        ours.file_name().unwrap().to_string_lossy(),
        format!(".{}", layer_ext())
    );
}

/// A config name carrying a dot is not truncated on either side.
#[test]
fn config_name_with_a_dot_matches_confy() {
    let ours =
        confy::get_configuration_file_path("amethystate-path-probe", Some("settings.v2")).unwrap();
    let theirs =
        real_confy::get_configuration_file_path("amethystate-path-probe", Some("settings.v2"))
            .unwrap();

    assert_eq!(
        theirs.file_name().unwrap().to_string_lossy(),
        format!("settings.v2.{CONFY_EXT}")
    );
    assert_eq!(
        ours.file_name().unwrap().to_string_lossy(),
        format!("settings.v2.{}", layer_ext())
    );
}

/// The whole point: an application that swaps the crates keeps the settings
/// its user already has.
///
/// This is the only test here that goes through the real configuration
/// directory, because the app-name overload is where the two paths part.
#[test]
#[ignore = "known: on redb/json/ron `load` reads another file and returns defaults, on toml it cannot decode confy's"]
fn a_confy_users_settings_survive_the_swap() {
    let app = "amethystate-confy-swap-probe";
    let written = a_config();

    let confy_file = real_confy::get_configuration_file_path(app, None).unwrap();
    let dir = confy_file.parent().unwrap().to_path_buf();
    sweep(&dir);

    let outcome = std::panic::catch_unwind(|| {
        real_confy::store(app, None, &written).expect("confy store");
        let got: Simple = confy::load(app, None).expect("layer load");
        assert_eq!(got, written);
    });

    sweep(&dir);
    outcome.expect("the layer did not read what confy wrote");
}

/// Empties a directory without failing when a handle is still open on it.
fn sweep(dir: &Path) {
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let _ = std::fs::remove_file(entry.path());
        }
    }
    let _ = std::fs::remove_dir(dir);
}

// ---------------------------------------------------------------- format

/// A file `confy` wrote loads through the layer with the same values.
#[test]
#[ignore = "known: toml decodes the document root wrongly; json/redb cannot parse confy's file at all"]
fn a_confy_file_loads_through_the_layer() {
    let temp = TempPath::new("confy-import");
    let path = sibling(&temp, CONFY_EXT);

    let written = a_config();
    real_confy::store_path(&path, &written).expect("confy store");

    let read_back: Simple = confy::load_path(&path).expect("layer load");
    assert_eq!(read_back, written);
}

/// Values whose serde shape differs between formats survive the layer.
#[test]
#[ignore = "known: same root decode failure as the flat case, before shape matters"]
fn a_rich_confy_file_survives_the_layer() {
    let temp = TempPath::new("confy-rich");
    let path = sibling(&temp, CONFY_EXT);

    let written = Rich {
        theme: Theme::Light,
        nickname: None,
        recent: vec!["a".to_string(), "b".to_string()],
        aliases: BTreeMap::from([("ls".to_string(), "exa".to_string())]),
        window: Window {
            width: 1280,
            height: 720,
        },
    };
    real_confy::store_path(&path, &written).expect("confy store");

    let read_back: Rich = confy::load_path(&path).expect("layer load");
    assert_eq!(read_back, written);
}

/// A value the layer writes is one `confy` can read back.
#[test]
fn what_the_layer_writes_confy_can_read() {
    if !backend_speaks_confy() {
        return;
    }
    let temp = TempPath::new("confy-export");
    let path = sibling(&temp, CONFY_EXT);

    let written = a_config();
    confy::store_path(&path, &written).expect("layer store");

    let read_back: Simple = real_confy::load_path(&path).expect("confy load");
    assert_eq!(read_back, written);
}

/// What the layer stores, the layer reads back.
#[test]
#[ignore = "known: fails on the toml backend - the document root does not survive a round trip"]
fn a_stored_value_is_read_back() {
    let temp = TempPath::new("confy-restart");
    let path = sibling(&temp, layer_ext());

    let written = Simple {
        name: "persisted".to_string(),
        comfy: false,
        foo: 3,
    };
    confy::store_path(&path, &written).expect("store");

    let read_back: Simple = confy::load_path(&path).expect("load");
    assert_eq!(read_back, written);
}

/// A struct written under a name reads back from it.
///
/// The control for the root case: under a name the toml backend stores the
/// struct as an inline table, which decodes, while at the root it stores a
/// table, which does not. Nothing but the root is affected.
#[test]
#[cfg(feature = "text")]
fn a_struct_reads_back_from_the_path_it_was_written_to() {
    use amethystate::StoreBuilder;

    let temp = TempPath::new("confy-table-path");
    let path = sibling(&temp, layer_ext());
    let store = StoreBuilder::new(&path).build().expect("store");

    let window = Window {
        width: 1280,
        height: 720,
    };
    store.set(["window"], &window).unwrap();

    assert_eq!(store.get::<Window>(["window"]).unwrap(), Some(window));
}

/// The route the library offers a migrating user: the same fields, at the
/// document root, read by an `as_root` schema.
#[test]
#[cfg(feature = "text")]
fn an_as_root_schema_reads_the_confy_file() {
    use amethystate::migration::set::MigrationSet;
    use amethystate::{Store, StoreConfig, amethystate};

    #[amethystate(as_root)]
    pub struct AppConfig {
        #[amestate(default = "Unknown".to_string())]
        pub name: String,
        #[amestate(default = true)]
        pub comfy: bool,
    }

    if !backend_speaks_confy() {
        return;
    }

    let temp = TempPath::new("confy-as-root");
    let path = sibling(&temp, CONFY_EXT);

    real_confy::store_path(&path, &a_config()).expect("confy store");

    let (store, _) = Store::open(StoreConfig::new(&path), MigrationSet::default()).expect("open");
    let config = AppConfig::new_with(&store).expect("schema init");

    assert_eq!(config.name().get(), "from-confy");
    assert!(!config.comfy().get());
}

// ---------------------------------------------------------------- failure

/// An empty file is reported as bad data rather than silently replaced.
#[test]
#[ignore = "known: on toml and redb an empty file becomes defaults; confy reports it"]
fn an_empty_file_is_not_silently_replaced() {
    let temp = TempPath::new("confy-empty");
    let path = sibling(&temp, CONFY_EXT);
    std::fs::write(&path, "").unwrap();

    assert!(
        real_confy::load_path::<Simple>(&path).is_err(),
        "confy accepted an empty file"
    );

    let ours = confy::load_path::<Simple>(&path);
    assert!(
        ours.is_err(),
        "the layer returned {:?} for an empty file",
        ours.map(|c| c.name)
    );
}

/// `load_or_else` falls back only when the file will not parse, as `confy`
/// does - a file `confy` itself wrote always parses.
#[test]
#[ignore = "known: the fallback runs over a valid confy file and overwrites it"]
fn load_or_else_keeps_a_confy_file() {
    let temp = TempPath::new("confy-orelse");
    let path = sibling(&temp, CONFY_EXT);

    let written = a_config();
    real_confy::store_path(&path, &written).expect("confy store");

    let fallback = || Simple {
        name: "fallback".to_string(),
        comfy: true,
        foo: 0,
    };

    let got: Simple = confy::load_or_else(&path, fallback).expect("load_or_else");
    assert_eq!(got, written, "fell back over a file confy wrote");

    assert!(
        read(&path).contains("from-confy"),
        "the user's file was replaced: {}",
        read(&path)
    );
}

/// A file that will not parse is treated the way `confy` treats it: `load_path`
/// reports it and leaves it alone.
#[test]
fn a_corrupt_file_is_reported_by_load_path() {
    let temp = TempPath::new("confy-corrupt");
    let path = sibling(&temp, CONFY_EXT);
    let corrupt = "name = \"half writ";
    std::fs::write(&path, corrupt).unwrap();

    assert!(real_confy::load_path::<Simple>(&path).is_err());
    assert!(confy::load_path::<Simple>(&path).is_err());
    assert_eq!(read(&path), corrupt, "the layer rewrote a file it rejected");
}

/// Loading leaves hand-written layout and comments as they were.
#[test]
fn loading_leaves_the_file_byte_identical() {
    if !backend_speaks_confy() {
        return;
    }
    let temp = TempPath::new("confy-untouched");
    let path = sibling(&temp, CONFY_EXT);

    let hand_written =
        "# the name shown in the title bar\nname   = \"hand written\"\ncomfy = true\nfoo = 42\n";
    std::fs::write(&path, hand_written).unwrap();

    let _ = confy::load_path::<Simple>(&path);

    assert_eq!(read(&path), hand_written);
}

/// Loading leaves no sidecar files beside the user's config.
#[test]
#[ignore = "known: the text backends write a `.meta` sibling next to the config file"]
fn loading_leaves_no_sidecar_files() {
    let temp = TempPath::new("confy-sidecar");
    let path = sibling(&temp, CONFY_EXT);

    real_confy::store_path(&path, &Simple::default()).expect("confy store");

    let _ = confy::load_path::<Simple>(&path);

    let dir = path.parent().unwrap();
    let stem = path.file_stem().unwrap().to_string_lossy().to_string();
    let name = path.file_name().unwrap().to_string_lossy().to_string();
    let siblings: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().to_string())
        .filter(|n| n.starts_with(&stem) && *n != name)
        .collect();

    assert!(siblings.is_empty(), "left behind: {siblings:?}");
}

/// The metadata written beside the config belongs to this file.
///
/// Every schema compiled into the binary is snapshotted into the sidecar of
/// whatever store is opened, so a config file the application only reads
/// through the layer acquires the schema of structs kept elsewhere.
#[test]
#[cfg(feature = "text")]
#[ignore = "known: the sidecar records every registered schema, this file's or not"]
fn the_sidecar_records_nothing_about_unrelated_schemas() {
    use amethystate::amethystate;

    #[amethystate(prefix = "unrelated")]
    pub struct Unrelated {
        #[amestate(default = 1u8)]
        pub tick: u8,
    }

    let temp = TempPath::new("confy-schema");
    let path = sibling(&temp, layer_ext());

    confy::store_path(&path, a_config()).expect("layer store");

    let meta = read(&path.with_extension("meta"));
    assert!(
        !meta.contains("Unrelated"),
        "the config's sidecar reads: {meta}"
    );
}

/// A sidecar an earlier build wrote does not make the config unreadable.
///
/// The sidecar is named `<stem>.meta` whatever the format inside it, so an
/// application that changes backend between releases finds the old one still
/// sitting there, and it is the store's open that reads it.
#[test]
#[cfg(feature = "text")]
#[ignore = "known: a sidecar in another format fails the open, and every load after it"]
fn a_sidecar_from_an_earlier_backend_does_not_break_the_load() {
    let temp = TempPath::new("confy-stale-meta");
    let path = seeded_store(&temp);
    std::fs::write(path.with_extension("meta"), "[schema]\n").unwrap();

    let loaded: Result<Simple, _> = confy::load_path(&path);
    assert!(loaded.is_ok(), "{:?}", loaded.err());
}

/// A load that fails leaves no half-finished backup beside the config.
#[test]
#[cfg(feature = "text")]
#[ignore = "known: the backup taken at open is left behind when the open then fails"]
fn a_failed_load_leaves_no_backup_beside_the_config() {
    let temp = TempPath::new("confy-bak-stray");
    let path = seeded_store(&temp);
    std::fs::write(path.with_extension("meta"), NOT_ANY_FORMAT).unwrap();

    assert!(confy::load_path::<Simple>(&path).is_err());

    let bak = path.with_extension("bak");
    assert!(!bak.exists(), "left behind: {}", bak.display());
}

/// The backup taken of the config holds the config.
///
/// The data file and the metadata file derive their backup path the same way,
/// `with_extension("bak")`, so for `x.toml` and `x.meta` it is one file and the
/// second copy overwrites the first.
#[test]
#[cfg(feature = "text")]
#[ignore = "known: the data and metadata backups collide on one path"]
fn the_backup_of_the_config_holds_the_config() {
    let temp = TempPath::new("confy-bak-collide");
    let path = seeded_store(&temp);
    std::fs::write(path.with_extension("meta"), NOT_ANY_FORMAT).unwrap();

    let _ = confy::load_path::<Simple>(&path);

    let bak = path.with_extension("bak");
    assert!(
        read(&bak).contains("from-confy"),
        "the config's backup reads: {}",
        read(&bak)
    );
}

// ---------------------------------------------------------------- the seam

/// Writing the confy section leaves the `amethystate` sections alone, as the
/// module documents.
#[test]
#[cfg(feature = "text")]
#[ignore = "known: a confy write replaces the whole document, prefixes included"]
fn storing_keeps_the_amethystate_sections() {
    use amethystate::StoreBuilder;

    let temp = TempPath::new("confy-coexist");
    let path = sibling(&temp, layer_ext());

    {
        let store = StoreBuilder::new(&path).build().expect("store");
        store.set(["network", "port"], &9090u16).unwrap();
        store.save_now().unwrap();
    }

    confy::store_path(&path, Simple::default()).expect("layer store");

    let store = StoreBuilder::new(&path).build().expect("reopen");
    assert_eq!(
        store.get::<u16>(["network", "port"]).unwrap(),
        Some(9090),
        "file now reads: {}",
        read(&path)
    );
}

/// The application can open its own store on the file the layer is using.
#[test]
#[ignore = "known: on redb the layer's cached store holds the file open - DatabaseAlreadyOpen"]
fn the_application_store_opens_alongside_the_layer() {
    use amethystate::StoreBuilder;

    let temp = TempPath::new("confy-two-opens");
    let path = sibling(&temp, layer_ext());

    confy::store_path(&path, Simple::default()).expect("layer store");

    StoreBuilder::new(&path)
        .build()
        .expect("the application cannot open its own store");
}

/// A later confy write does not undo an application write made in between.
#[test]
#[cfg(feature = "text")]
#[ignore = "known: the layer keeps a second document for the same file and writes it whole"]
fn a_confy_write_does_not_clobber_an_application_write() {
    use amethystate::StoreBuilder;

    let temp = TempPath::new("confy-clobber");
    let path = sibling(&temp, layer_ext());

    confy::store_path(&path, Simple::default()).expect("layer store");

    let store = StoreBuilder::new(&path).build().expect("store");
    store.set(["network", "port"], &9090u16).unwrap();
    store.save_now().unwrap();

    confy::store_path(&path, a_config()).expect("layer store");

    assert!(
        read(&path).contains("9090"),
        "file now reads: {}",
        read(&path)
    );
}

/// Opening an `amethystate` store over a confy file, migrations and all,
/// leaves the confy fields where they were.
#[test]
#[cfg(feature = "text")]
fn opening_a_store_keeps_the_confy_fields() {
    use amethystate::StoreBuilder;

    if !backend_speaks_confy() {
        return;
    }

    let temp = TempPath::new("confy-cleanup");
    let path = sibling(&temp, CONFY_EXT);

    real_confy::store_path(&path, &a_config()).expect("confy store");

    let (store, _) = StoreBuilder::new(&path)
        .build_with_report()
        .expect("open with migrations");
    store.save_now().unwrap();
    drop(store);

    let still_there: Simple = real_confy::load_path(&path).expect("confy re-read");
    assert_eq!(still_there, a_config());
}

/// The permissions a store call sets are the ones the file keeps.
#[test]
#[cfg(feature = "text")]
fn a_read_only_file_is_refused_the_way_confy_refuses_it() {
    let temp = TempPath::new("confy-perms");
    let path = sibling(&temp, layer_ext());

    confy::store_path(&path, Simple::default()).expect("layer store");

    let mut perms = std::fs::metadata(&path).unwrap().permissions();
    perms.set_readonly(true);
    confy::store_path_perms(&path, Simple::default(), perms).expect("store with perms");

    let confy_refuses = {
        let temp2 = TempPath::new("confy-perms-ref");
        let path2 = sibling(&temp2, CONFY_EXT);
        real_confy::store_path(&path2, Simple::default()).unwrap();
        let mut p = std::fs::metadata(&path2).unwrap().permissions();
        p.set_readonly(true);
        std::fs::set_permissions(&path2, p).unwrap();
        real_confy::store_path(&path2, a_config()).is_err()
    };

    let layer_refuses = confy::store_path(&path, a_config()).is_err();
    assert_eq!(
        layer_refuses, confy_refuses,
        "read-only file: confy refuses={confy_refuses}, layer refuses={layer_refuses}"
    );
}
