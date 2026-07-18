//! Compatibility adapter for the [`confy`](https://github.com/rust-cli/confy) crate.
//!
//! This module is a nearly 1-to-1 emulation of the `confy` API (v0.6 and v2.x),
//! rewritten to route all persistence operations directly through `amethystate::Store`.
//!
//! ### Limitations
//! - Support for `yaml_conf` and `basic_toml_conf` was removed because their upstream crates
//!   are unmaintained or abandoned (e.g., `serde_yaml` is archived, and its forks like `yaml-serde`
//!   appear to be stale).
//! - Storage under the hood uses the active `amethystate` backend (`TextStore` or `RedbStore`).
//! ### Behavioral differences from the original `confy`
//! - [`store`] and [`store_path`] update only the **root section** of the configuration file,
//!   leaving any `amethystate`-managed sections intact (sections correspond to the `prefix`
//!   defined via `#[amethystate(prefix = "...")]`, e.g. `[network]`, `[ui]`) —
//!   whereas the original `confy` overwrites the entire file on every call.

#[cfg(feature = "confy-compat-0-6")]
use directories::ProjectDirs;

#[cfg(feature = "confy-compat")]
use etcetera::{
    AppStrategy, AppStrategyArgs, app_strategy::choose_app_strategy,
    app_strategy::choose_native_strategy,
};

use serde::{Serialize, de::DeserializeOwned};
use std::fs::{self, Permissions};
use std::io;
use std::path::{Path, PathBuf};
use thiserror::Error;

use crate::codec::CodecError;
use crate::store::StorageError as RpError;
use crate::{DefaultStore, StoreBuilder};

use crate::store::Store;
#[cfg(feature = "toml")]
use toml_edit::de::Error as TomlDeErr;
#[cfg(feature = "toml")]
use toml_edit::ser::Error as TomlSerErr;

#[cfg(backend = "toml")]
const EXTENSION: &str = "toml";

#[cfg(backend = "json")]
const EXTENSION: &str = "json";

#[cfg(backend = "ron")]
const EXTENSION: &str = "ron";

#[cfg(backend = "redb")]
const EXTENSION: &str = "redb";

#[cfg(backend = "sqlite")]
const EXTENSION: &str = "redb";

const DEFAULT_KEY: &str = ".";

static STRATEGY: std::sync::OnceLock<std::sync::Mutex<ConfigStrategy>> = std::sync::OnceLock::new();
static STORES: std::sync::OnceLock<
    std::sync::Mutex<std::collections::HashMap<PathBuf, std::sync::Arc<DefaultStore>>>,
> = std::sync::OnceLock::new();

fn get_strategy() -> &'static std::sync::Mutex<ConfigStrategy> {
    STRATEGY.get_or_init(|| {
        #[cfg(feature = "confy-compat")]
        let default = ConfigStrategy::App;
        #[cfg(all(not(feature = "confy-compat"), feature = "confy-compat-0-6"))]
        let default = ConfigStrategy::Directories;
        std::sync::Mutex::new(default)
    })
}

fn get_store(path: &Path) -> Result<std::sync::Arc<DefaultStore>, ConfyError> {
    let mut map = STORES
        .get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new()))
        .lock()
        .map_err(|e| ConfyError::GeneralLoadError(io::Error::other(e.to_string())))?;

    if let Some(store) = map.get(path) {
        return Ok(store.clone());
    }

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(ConfyError::DirectoryCreationFailed)?;
    }

    let raw_store = StoreBuilder::new(path).build().map_err(ConfyError::from)?;
    let store = std::sync::Arc::new(raw_store);
    map.insert(path.to_path_buf(), store.clone());

    Ok(store)
}

/// The errors the confy crate can encounter.
#[derive(Debug, Error)]
pub enum ConfyError {
    #[cfg(feature = "toml")]
    #[error("Bad TOML data")]
    BadTomlData(#[source] TomlDeErr),

    #[cfg(feature = "json")]
    #[error("Bad JSON data")]
    BadJsonData(#[source] serde_json::Error),

    #[cfg(feature = "ron")]
    #[error("Bad RON data")]
    BadRonData(#[source] ron::error::Error),

    #[error("Failed to create directory")]
    DirectoryCreationFailed(#[source] std::io::Error),

    #[error("Failed to load configuration file")]
    GeneralLoadError(#[source] std::io::Error),

    #[error("Bad configuration directory: {0}")]
    BadConfigDirectory(String),

    #[cfg(feature = "toml")]
    #[error("Failed to serialize configuration data into TOML")]
    SerializeTomlError(#[source] TomlSerErr),

    #[cfg(feature = "json")]
    #[error("Failed to serialize configuration data into JSON")]
    SerializeJsonError(#[source] serde_json::Error),

    #[cfg(feature = "ron")]
    #[error("Failed to serialize configuration data into RON")]
    SerializeRonError(#[source] ron::error::Error),

    #[error("Failed to write configuration file")]
    WriteConfigurationFileError(#[source] std::io::Error),

    #[error("Failed to read configuration file")]
    ReadConfigurationFileError(#[source] std::io::Error),

    #[error("Failed to open configuration file")]
    OpenConfigurationFileError(#[source] std::io::Error),

    #[error("Failed to set configuration file permissions")]
    SetPermissionsFileError(#[source] std::io::Error),
}

impl From<RpError> for ConfyError {
    fn from(err: RpError) -> Self {
        match err {
            #[cfg(feature = "text")]
            RpError::TextStore(text_err) => {
                use crate::store::backend::text::error::TextStoreError;
                match text_err {
                    TextStoreError::Io(io_err) => ConfyError::GeneralLoadError(io_err),
                    TextStoreError::Codec(codec_err) => match codec_err {
                        #[cfg(feature = "json")]
                        CodecError::Json(e) => ConfyError::BadJsonData(e),
                        #[cfg(feature = "toml")]
                        CodecError::Toml(s) => {
                            #[cfg(feature = "toml")]
                            {
                                ConfyError::BadTomlData(serde::de::Error::custom(s))
                            }
                            #[cfg(not(feature = "toml"))]
                            {
                                panic!(
                                    "TOML codec error encountered but feature `toml` is disabled: {}",
                                    s
                                );
                            }
                        }
                        #[cfg(feature = "ron")]
                        CodecError::Ron(e) => ConfyError::BadRonData(e),
                        _ => panic!(
                            "Unexpected codec error during confy emulation: {:?}",
                            codec_err
                        ),
                    },
                    TextStoreError::RootMustBeObject => {
                        ConfyError::BadConfigDirectory("Root must be an object/mapping".to_string())
                    }
                    TextStoreError::PathSegmentMissing(s) => ConfyError::BadConfigDirectory(s),
                    TextStoreError::Watch(err) => {
                        panic!("Unexpected watch error during confy emulation: {:?}", err);
                    }
                }
            }
            #[cfg(feature = "redb")]
            RpError::RedbStore(redb_err) => {
                panic!(
                    "Unexpected REDB store error during confy emulation: {:?}",
                    redb_err
                );
            }
            #[cfg(feature = "sqlite")]
            RpError::Sqlite(sqlite) => {
                panic!(
                    "Unexpected Sqlite store error during confy emulation: {:?}",
                    sqlite
                );
            }
            RpError::Migration(mig_err) => {
                panic!(
                    "Unexpected migration error during confy emulation: {:?}",
                    mig_err
                );
            }
        }
    }
}

/// Determine what strategy `confy` should use
/// these are based off of [the etcetera crate's strategies](https://docs.rs/etcetera/latest/etcetera/#strategies).
///
/// To change use [`change_config_strategy`] function before calling any load or save functions.
pub enum ConfigStrategy {
    /// The `App` Strategy is the default strategy
    /// this is the traditional XDG strategy and will place the config file in the XDG directories.
    /// See [Etcetera App Strategy](https://docs.rs/etcetera/latest/etcetera/#appstrategy) for more information.
    #[cfg(feature = "confy-compat")]
    App,
    /// The `Native` Strategy is mainly used for GUI applications and places the config directory based on the
    /// host systems determination. See [Etcetera Native Strategy](https://docs.rs/etcetera/latest/etcetera/#native-strategy) for more information.
    #[cfg(feature = "confy-compat")]
    Native,
    /// The legacy directories strategy from confy v0.6.
    #[cfg(feature = "confy-compat-0-6")]
    Directories,
}

/// Changes the strategy to use which places the config file using XDG or the native OS's configuration.
///
/// The default is the App Strategy see [`ConfigStrategy`] for more details on the strategy's affect.
///
/// ```rust,no_run,ignore
/// # use confy::{ConfyError, ConfigStrategy, change_config_strategy};
/// # use serde_derive::{Serialize, Deserialize};
/// # fn main() -> Result<(), ConfyError> {
/// #[derive(Default, Serialize, Deserialize)]
/// struct MyConfig {}
/// // use the native file paths to store the config
/// change_config_strategy(ConfigStrategy::Native);
///
/// let cfg: MyConfig = confy::load("my-app-name", None)?;
/// # Ok(())
/// # }
/// ```
pub fn change_config_strategy(changer: ConfigStrategy) {
    *get_strategy()
        .lock()
        .expect("Error getting lock on Config Strategy") = changer;
}

#[cfg(feature = "confy-compat")]
#[allow(unused)]
enum InternalStrategy {
    App(etcetera::app_strategy::Xdg),
    NativeMac(etcetera::app_strategy::Apple),
    NativeUnix(etcetera::app_strategy::Unix),
    NativeWindows(etcetera::app_strategy::Windows),
}

#[cfg(feature = "confy-compat")]
impl AppStrategy for InternalStrategy {
    fn home_dir(&self) -> &Path {
        unimplemented!()
    }

    fn config_dir(&self) -> PathBuf {
        match self {
            InternalStrategy::App(xdg) => xdg.config_dir(),
            InternalStrategy::NativeMac(mac) => mac.config_dir(),
            InternalStrategy::NativeUnix(unix) => unix.config_dir(),
            InternalStrategy::NativeWindows(windows) => windows.config_dir(),
        }
    }

    fn data_dir(&self) -> PathBuf {
        unimplemented!()
    }

    fn cache_dir(&self) -> PathBuf {
        unimplemented!()
    }

    fn state_dir(&self) -> Option<PathBuf> {
        unimplemented!()
    }

    fn runtime_dir(&self) -> Option<PathBuf> {
        unimplemented!()
    }
}

#[cfg(feature = "confy-compat")]
impl From<etcetera::app_strategy::Xdg> for InternalStrategy {
    fn from(value: etcetera::app_strategy::Xdg) -> Self {
        InternalStrategy::App(value)
    }
}

#[cfg(feature = "confy-compat")]
impl From<etcetera::app_strategy::Apple> for InternalStrategy {
    fn from(value: etcetera::app_strategy::Apple) -> Self {
        InternalStrategy::NativeMac(value)
    }
}

#[cfg(feature = "confy-compat")]
impl From<etcetera::app_strategy::Unix> for InternalStrategy {
    fn from(value: etcetera::app_strategy::Unix) -> Self {
        InternalStrategy::NativeUnix(value)
    }
}

#[cfg(feature = "confy-compat")]
impl From<etcetera::app_strategy::Windows> for InternalStrategy {
    fn from(value: etcetera::app_strategy::Windows) -> Self {
        InternalStrategy::NativeWindows(value)
    }
}

/// Load an application configuration from disk
///
/// A new configuration file is created with default values if none
/// exists.
///
/// Errors that are returned from this function are I/O related,
/// for example if the writing of the new configuration fails
/// or `confy` encounters an operating system or environment
/// that it does not support.
///
/// **Note:** The type of configuration needs to be declared in some way
/// that is inferable by the compiler. Also note that your
/// configuration needs to implement `Default`.
///
/// ```rust,no_run,ignore
/// # use confy::ConfyError;
/// # use serde_derive::{Serialize, Deserialize};
/// # fn main() -> Result<(), ConfyError> {
/// #[derive(Default, Serialize, Deserialize)]
/// struct MyConfig {}
///
/// let cfg: MyConfig = confy::load("my-app-name", None)?;
/// # Ok(())
/// # }
/// ```
pub fn load<'a, T: Serialize + DeserializeOwned + Default>(
    app_name: &str,
    config_name: impl Into<Option<&'a str>>,
) -> Result<T, ConfyError> {
    get_configuration_file_path(app_name, config_name).and_then(load_path)
}

/// Load an application configuration from a specified path.
///
/// A new configuration file is created with default values if none
/// exists.
///
/// This is an alternate version of [`load`] that allows the specification of
/// an arbitrary path instead of a system one.  For more information on errors
/// and behavior, see [`load`]'s documentation.
///
/// [`load`]: fn.load.html
pub fn load_path<T: Serialize + DeserializeOwned + Default>(
    path: impl AsRef<Path>,
) -> Result<T, ConfyError> {
    let path = path.as_ref();
    let file_missing_or_empty =
        !path.exists() || fs::metadata(path).map(|m| m.len() == 0).unwrap_or(true);
    let store = get_store(path)?;

    if file_missing_or_empty {
        let cfg = T::default();
        store.set(DEFAULT_KEY, &cfg).map_err(ConfyError::from)?;
        store.save_now().map_err(ConfyError::from)?;
        return Ok(cfg);
    }

    match store.get::<T>(DEFAULT_KEY) {
        Ok(Some(cfg)) => Ok(cfg),
        Ok(None) => {
            let cfg = T::default();
            store.set(DEFAULT_KEY, &cfg).map_err(ConfyError::from)?;
            store.save_now().map_err(ConfyError::from)?;
            Ok(cfg)
        }
        Err(e) => Err(ConfyError::from(e)),
    }
}

/// Load an application configuration from a specified path.
///
/// A new configuration file is created with `op`'s result if none
/// exists or file content is incorrect.
///
/// This is an alternate version of [`load`] that allows the specification of
/// an arbitrary path instead of a system one.  For more information on errors
/// and behavior, see [`load`]'s documentation.
///
/// [`load`]: fn.load.html
pub fn load_or_else<T, F>(path: impl AsRef<Path>, op: F) -> Result<T, ConfyError>
where
    T: DeserializeOwned + Serialize,
    F: FnOnce() -> T,
{
    let path = path.as_ref();
    let file_missing_or_empty =
        !path.exists() || fs::metadata(path).map(|m| m.len() == 0).unwrap_or(true);

    if file_missing_or_empty {
        let store = get_store(path)?;
        let cfg = op();
        store.set(DEFAULT_KEY, &cfg).map_err(ConfyError::from)?;
        store.save_now().map_err(ConfyError::from)?;
        return Ok(cfg);
    }

    match get_store(path) {
        Ok(store) => match store.get::<T>(DEFAULT_KEY) {
            Ok(Some(cfg)) => Ok(cfg),
            _ => {
                let cfg = op();
                store.set(DEFAULT_KEY, &cfg).map_err(ConfyError::from)?;
                store.save_now().map_err(ConfyError::from)?;
                Ok(cfg)
            }
        },
        Err(_) => {
            let _ = std::fs::remove_file(path);
            let store = get_store(path)?;
            let cfg = op();
            store.set(DEFAULT_KEY, &cfg).map_err(ConfyError::from)?;
            store.save_now().map_err(ConfyError::from)?;
            Ok(cfg)
        }
    }
}

/// Save changes made to a configuration object
///
/// This function will update a configuration,
/// with the provided values, and create a new one,
/// if none exists.
///
/// You can also use this function to create a new configuration
/// with different initial values than which are provided
/// by your `Default` trait implementation, or if your
/// configuration structure _can't_ implement `Default`.
///
/// ```rust,no_run,ignore
/// # use serde_derive::{Serialize, Deserialize};
/// # use confy::ConfyError;
/// # fn main() -> Result<(), ConfyError> {
/// #[derive(Serialize, Deserialize)]
/// struct MyConf {}
///
/// let my_cfg = MyConf {};
/// confy::store("my-app-name", None, my_cfg)?;
/// # Ok(())
/// # }
/// ```
///
/// Errors returned are I/O errors related to not being
/// able to write the configuration file or if `confy`
/// encounters an operating system or environment it does
/// not support.
///
/// **Note:** Unlike the original `confy`, this implementation routes persistence
/// through `amethystate::Store`. Calling `store` updates only the root section of the
/// file, leaving any `amethystate`-managed sections (e.g. `[network]`, `[ui]`) intact.
/// Code that relies on `store` wiping the file clean will behave differently here.
pub fn store<'a, T: Serialize>(
    app_name: &str,
    config_name: impl Into<Option<&'a str>>,
    cfg: T,
) -> Result<(), ConfyError> {
    let path = get_configuration_file_path(app_name, config_name)?;
    store_path(path, cfg)
}

/// Save changes made to a configuration object at a specified path
///
/// This is an alternate version of [`store`] that allows the specification of
/// file permissions that must be set. For more information on errors and
/// behavior, see [`store`]'s documentation.
///
/// [`store`]: fn.store.html
pub fn store_perms<'a, T: Serialize>(
    app_name: &str,
    config_name: impl Into<Option<&'a str>>,
    cfg: T,
    perms: Permissions,
) -> Result<(), ConfyError> {
    let path = get_configuration_file_path(app_name, config_name)?;
    store_path_perms(path, cfg, perms)
}

/// Save changes made to a configuration object at a specified path
///
/// This is an alternate version of [`store`] that allows the specification of
/// an arbitrary path instead of a system one.  For more information on errors
/// and behavior, see [`store`]'s documentation.
///
/// [`store`]: fn.store.html
pub fn store_path<T: Serialize>(path: impl AsRef<Path>, cfg: T) -> Result<(), ConfyError> {
    do_store(path.as_ref(), cfg, None)
}

/// Save changes made to a configuration object at a specified path
///
/// This is an alternate version of [`store_path`] that allows the
/// specification of file permissions that must be set. For more information on
/// errors and behavior, see [`store`]'s documentation.
///
/// [`store_path`]: fn.store_path.html
pub fn store_path_perms<T: Serialize>(
    path: impl AsRef<Path>,
    cfg: T,
    perms: Permissions,
) -> Result<(), ConfyError> {
    do_store(path.as_ref(), cfg, Some(perms))
}

fn do_store<T: Serialize>(
    path: &Path,
    cfg: T,
    perms: Option<Permissions>,
) -> Result<(), ConfyError> {
    let config_dir = path
        .parent()
        .ok_or_else(|| ConfyError::BadConfigDirectory(format!("{path:?} is a root or prefix")))?;
    fs::create_dir_all(config_dir).map_err(ConfyError::DirectoryCreationFailed)?;

    let store = get_store(path)?;
    store.set(DEFAULT_KEY, &cfg).map_err(ConfyError::from)?;
    store.save_now().map_err(ConfyError::from)?;

    if let Some(p) = perms {
        fs::set_permissions(path, p).map_err(ConfyError::SetPermissionsFileError)?;
    }

    Ok(())
}

/// Get the configuration file path used by [`load`] and [`store`]
///
/// This is useful if you want to show where the configuration file is to your user.
///
/// [`load`]: fn.load.html
/// [`store`]: fn.store.html
pub fn get_configuration_file_path<'a>(
    app_name: &str,
    config_name: impl Into<Option<&'a str>>,
) -> Result<PathBuf, ConfyError> {
    let config_name = config_name.into().unwrap_or("default-config");
    let strategy_lock = get_strategy()
        .lock()
        .expect("Error getting lock on config strategy");

    let path = match *strategy_lock {
        #[cfg(feature = "confy-compat")]
        ConfigStrategy::App => {
            let project = choose_app_strategy(AppStrategyArgs {
                top_level_domain: "rs".to_string(),
                author: "".to_string(),
                app_name: app_name.to_string(),
            })
            .map_err(|e| {
                ConfyError::BadConfigDirectory(format!(
                    "could not determine home directory path: {e}"
                ))
            })?;
            let mut p = project.config_dir();
            p.push(format!("{config_name}.{EXTENSION}"));
            p
        }
        #[cfg(feature = "confy-compat")]
        ConfigStrategy::Native => {
            let project = choose_native_strategy(AppStrategyArgs {
                top_level_domain: "rs".to_string(),
                author: "".to_string(),
                app_name: app_name.to_string(),
            })
            .map_err(|e| {
                ConfyError::BadConfigDirectory(format!(
                    "could not determine home directory path: {e}"
                ))
            })?;
            let mut p = project.config_dir();
            p.push(format!("{config_name}.{EXTENSION}"));
            p
        }
        #[cfg(feature = "confy-compat-0-6")]
        ConfigStrategy::Directories => {
            let project = ProjectDirs::from("rs", "", app_name).ok_or_else(|| {
                ConfyError::BadConfigDirectory(
                    "could not determine home directory path".to_string(),
                )
            })?;
            let config_dir_str = project.config_dir().to_str().ok_or_else(|| {
                ConfyError::BadConfigDirectory(format!(
                    "{:?} is not valid Unicode",
                    project.config_dir()
                ))
            })?;

            [config_dir_str, &format!("{config_name}.{EXTENSION}")]
                .iter()
                .collect()
        }
    };

    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Serializer;
    use serde::{Deserialize, Serialize};
    use std::fs::File;
    use std::io::Write;

    #[derive(PartialEq, Default, Debug, Serialize, Deserialize)]
    struct ExampleConfig {
        name: String,
        count: usize,
    }

    /// Run a test function with a temporary config path as fixture.
    fn with_config_path(test_fn: fn(&Path)) {
        let config_dir = tempfile::tempdir().expect("creating test fixture failed");
        // config_path should roughly correspond to the result of `get_configuration_file_path("example-app", "example-config")`
        let config_path = config_dir
            .path()
            .join("example-app")
            .join("example-config")
            .with_extension(EXTENSION);
        test_fn(&config_path);

        if let Some(mutex) = STORES.get()
            && let Ok(mut map) = mutex.lock()
        {
            map.remove(&config_path);
        }

        config_dir.close().expect("removing test fixture failed");
    }

    /// [`load_path`] loads [`ExampleConfig`].
    #[test]
    fn load_path_works() {
        with_config_path(|path| {
            let config: ExampleConfig = load_path(path).expect("load_path failed");
            assert_eq!(config, ExampleConfig::default());
        })
    }

    /// [`load_or_else`] loads [`ExampleConfig`].
    #[allow(clippy::unused_io_amount)]
    #[test]
    fn load_or_else_works() {
        with_config_path(|path| {
            let the_value = || ExampleConfig {
                name: "a".to_string(),
                count: 5,
            };

            let config: ExampleConfig = load_or_else(path, the_value).expect("load_or_else failed");
            assert_eq!(config, the_value());
        });

        with_config_path(|path| {
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            let mut file = File::create(path).expect("creating file failed");
            file.write("some normal text".as_bytes())
                .expect("write to file failed");
            drop(file);

            let the_value = || ExampleConfig {
                name: "a".to_string(),
                count: 5,
            };

            let config: ExampleConfig = load_or_else(path, the_value).expect("load_or_else failed");
            assert_eq!(config, the_value());
        })
    }

    /// [`store_path`] stores [`ExampleConfig`].
    #[test]
    fn test_store_path() {
        with_config_path(|path| {
            let config: ExampleConfig = ExampleConfig {
                name: "Test".to_string(),
                count: 42,
            };
            store_path(path, &config).expect("store_path failed");
            let loaded = load_path(path).expect("load_path failed");
            assert_eq!(config, loaded);
        })
    }

    /// [`store_path_perms`] stores [`ExampleConfig`], with only read permission for owner (UNIX).
    #[test]
    #[cfg(all(feature = "text", not(feature = "redb")))]
    #[cfg(unix)]
    fn test_store_path_perms() {
        with_config_path(|path| {
            let config: ExampleConfig = ExampleConfig {
                name: "Secret".to_string(),
                count: 16549,
            };
            store_path_perms(path, &config, Permissions::from_mode(0o600))
                .expect("store_path_perms failed");
            let loaded = load_path(path).expect("load_path failed");
            assert_eq!(config, loaded);
        })
    }

    /// [`store_path_perms`] stores [`ExampleConfig`], as read-only.
    #[test]
    fn test_store_path_perms_readonly() {
        with_config_path(|path| {
            let config: ExampleConfig = ExampleConfig {
                name: "Soon read-only".to_string(),
                count: 27115,
            };
            store_path(path, &config).expect("store_path failed");

            let metadata = fs::metadata(path).expect("reading metadata failed");
            let mut permissions = metadata.permissions();
            permissions.set_readonly(true);

            store_path_perms(path, &config, permissions).expect("store_path_perms failed");

            assert!(
                fs::metadata(path)
                    .expect("reading metadata failed")
                    .permissions()
                    .readonly()
            );
        })
    }

    /// [`store_path`] fails when given a root path.
    #[test]
    fn test_store_path_root_error() {
        let err = store_path(PathBuf::from("/"), ExampleConfig::default())
            .expect_err("store_path should fail");
        assert_eq!(
            err.to_string(),
            r#"Bad configuration directory: "/" is a root or prefix"#,
        )
    }

    #[allow(unused)]
    struct CannotSerialize;

    impl Serialize for CannotSerialize {
        fn serialize<S>(&self, _serializer: S) -> Result<S::Ok, S::Error>
        where
            S: Serializer,
        {
            use serde::ser::Error;
            Err(S::Error::custom("cannot serialize CannotSerialize"))
        }
    }

    // Verify that [`load_path`] can deserialize into structs with differing names
    // as long as they have the same fields
    #[test]
    fn test_change_struct_name() -> Result<(), ConfyError> {
        with_config_path(|path| {
            #[derive(PartialEq, Default, Debug, Serialize, Deserialize)]
            struct AnotherExampleConfig {
                name: String,
                count: usize,
            }

            store_path(path, ExampleConfig::default()).expect("store_path failed");
            let _: AnotherExampleConfig = load_path(path).expect("load_path failed");
        });

        Ok(())
    }

    #[test]
    fn test_store_path_native() {
        // change the strategy first then the app will always use it
        change_config_strategy(ConfigStrategy::Native);

        with_config_path(|path| {
            let config: ExampleConfig = ExampleConfig {
                name: "Test".to_string(),
                count: 42,
            };

            let file_path = get_configuration_file_path("example-app", "example-config").unwrap();

            if cfg!(target_os = "macos") {
                assert_eq!(
                    file_path,
                    Path::new(&format!(
                        "{}/Library/Preferences/rs.example-app/example-config.{EXTENSION}",
                        std::env::home_dir().unwrap().display()
                    )),
                );
            } else if cfg!(target_os = "linux") {
                assert_eq!(
                    file_path,
                    Path::new(&format!(
                        "{}/.config/example-app/example-config.{EXTENSION}",
                        std::env::home_dir().unwrap().display()
                    ))
                );
            } else {
                //windows
                assert_eq!(
                    file_path,
                    Path::new(&format!(
                        "{}\\AppData\\Roaming\\example-app\\config\\example-config.{EXTENSION}",
                        std::env::home_dir().unwrap().display()
                    )),
                );
            }

            // Make sure it is still the same config file
            store_path(path, &config).expect("store_path failed");
            let loaded = load_path(path).expect("load_path failed");
            assert_eq!(config, loaded);
        })
    }

    #[test]
    fn test_store_path_change() {
        // change the strategy first to native
        change_config_strategy(ConfigStrategy::Native);

        with_config_path(|path| {
            let config: ExampleConfig = ExampleConfig {
                name: "Test".to_string(),
                count: 42,
            };

            let file_path = get_configuration_file_path("example-app", "example-config").unwrap();

            if cfg!(target_os = "macos") {
                assert_eq!(
                    file_path,
                    Path::new(&format!(
                        "{}/Library/Preferences/rs.example-app/example-config.{EXTENSION}",
                        std::env::home_dir().unwrap().display()
                    )),
                );
            } else if cfg!(target_os = "linux") {
                assert_eq!(
                    file_path,
                    Path::new(&format!(
                        "{}/.config/example-app/example-config.{EXTENSION}",
                        std::env::home_dir().unwrap().display()
                    ))
                );
            } else {
                //windows
                assert_eq!(
                    file_path,
                    Path::new(&format!(
                        "{}\\AppData\\Roaming\\example-app\\config\\example-config.{EXTENSION}",
                        std::env::home_dir().unwrap().display()
                    )),
                );
            }

            //change the strategy back to Application style
            change_config_strategy(ConfigStrategy::App);

            let file_path = get_configuration_file_path("example-app", "example-config").unwrap();

            if cfg!(target_os = "macos") || cfg!(target_os = "linux") {
                assert_eq!(
                    file_path,
                    Path::new(&format!(
                        "{}/.config/example-app/example-config.{EXTENSION}",
                        std::env::home_dir().unwrap().display()
                    )),
                );
            } else {
                //windows
                assert_eq!(
                    file_path,
                    Path::new(&format!(
                        "{}\\AppData\\Roaming\\example-app\\config\\example-config.{EXTENSION}",
                        std::env::home_dir().unwrap().display()
                    )),
                );
            }

            // Make sure it is still the same config file
            store_path(path, &config).expect("store_path failed");
            let loaded = load_path(path).expect("load_path failed");
            assert_eq!(config, loaded);
        })
    }
}
