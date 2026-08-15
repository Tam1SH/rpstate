use crate::store::StorageResult;
use std::path::PathBuf;
use std::time::Duration;

use crate::migration::builder::MigrationBuilder;

use crate::store::config::StoreConfig;
use crate::{MigrationReport, Store};

/// Which engine backs the store. Chosen when the store is built - enabling a
/// feature adds a choice here, it does not take the default away from anyone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Backend {
    #[cfg(feature = "redb")]
    Redb,
    #[cfg(feature = "json")]
    Json,
    #[cfg(feature = "toml")]
    Toml,
    #[cfg(feature = "ron")]
    Ron,
    #[cfg(feature = "sqlite")]
    Sqlite,
}

impl Backend {
    pub const fn extension(self) -> &'static str {
        match self {
            #[cfg(feature = "redb")]
            Backend::Redb => "redb",
            #[cfg(feature = "json")]
            Backend::Json => "json",
            #[cfg(feature = "toml")]
            Backend::Toml => "toml",
            #[cfg(feature = "ron")]
            Backend::Ron => "ron",
            #[cfg(feature = "sqlite")]
            Backend::Sqlite => "db",
        }
    }

    pub fn open_public(
        self,
        config: StoreConfig,
        mset: crate::migration::set::MigrationSet,
    ) -> StorageResult<(Store, MigrationReport)> {
        match self {
            #[cfg(feature = "redb")]
            Backend::Redb => {
                let (s, r) = crate::store::backend::redb::RedbStore::open(config, mset)?;
                Ok((Store::from_arc(std::sync::Arc::new(s)), r))
            }
            #[cfg(feature = "json")]
            Backend::Json => {
                let (s, r) = crate::store::backend::text::JsonStore::open(config, mset)?;
                Ok((Store::from_arc(std::sync::Arc::new(s)), r))
            }
            #[cfg(feature = "toml")]
            Backend::Toml => {
                let (s, r) = crate::store::backend::text::TomlStore::open(config, mset)?;
                Ok((Store::from_arc(std::sync::Arc::new(s)), r))
            }
            #[cfg(feature = "ron")]
            Backend::Ron => {
                let (s, r) = crate::store::backend::text::RonStore::open(config, mset)?;
                Ok((Store::from_arc(std::sync::Arc::new(s)), r))
            }
            #[cfg(feature = "sqlite")]
            Backend::Sqlite => {
                let (s, r) = crate::store::backend::sqlite::SqliteStore::open(config, mset)?;
                Ok((Store::from_arc(std::sync::Arc::new(s)), r))
            }
        }
    }
}

/// Expands a priority list into the `not(...)` cascade that picking the first
/// enabled feature otherwise requires by hand.
macro_rules! first_enabled_backend {
    ($feat:literal => $variant:expr $(, $rest_feat:literal => $rest_variant:expr)* $(,)?) => {
        {
            #[cfg(feature = $feat)]
            { $variant }
            #[cfg(not(feature = $feat))]
            { first_enabled_backend!($($rest_feat => $rest_variant),*) }
        }
    };
    () => {
        compile_error!(
            "amethystate needs at least one storage backend feature: redb, sqlite, json, toml or ron"
        )
    };
}

/// The engine used when the caller does not name one. Priority runs left to
/// right; enabling a feature adds a candidate at its own position and never
/// displaces one ahead of it.
pub const fn default_backend() -> Backend {
    first_enabled_backend! {
        "redb"   => Backend::Redb,
        "sqlite" => Backend::Sqlite,
        "json"   => Backend::Json,
        "toml"   => Backend::Toml,
        "ron"    => Backend::Ron,
    }
}

pub struct StoreBuilder {
    backend: Backend,
    config: StoreConfig,
    migration_builder: MigrationBuilder,
}

impl StoreBuilder {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        let mut path: PathBuf = path.into();
        if path.extension().is_none() {
            path.set_extension(default_backend().extension());
        }
        Self {
            backend: default_backend(),
            config: StoreConfig::new(path),
            migration_builder: MigrationBuilder::default(),
        }
    }

    /// Returns a [`StoreBuilder`] configured to use the platform-appropriate configuration
    /// directory for the given application name.
    ///
    /// The directory strategy depends on the active feature flag:
    /// - `confy-compat-0-6`: uses the [`directories`] crate (legacy `confy` v0.6 behavior)
    /// - default: uses the [`etcetera`] crate (XDG / native OS strategy)
    pub fn for_app(
        app_name: impl AsRef<str>,
        config_name: impl AsRef<str>,
    ) -> std::io::Result<Self> {
        #[cfg(feature = "confy-compat-0-6")]
        {
            Self::for_app_v06(app_name, config_name)
        }
        #[cfg(not(feature = "confy-compat-0-6"))]
        {
            Self::for_app_v2(app_name, config_name)
        }
    }

    pub fn for_app_v2(
        app_name: impl AsRef<str>,
        config_name: impl AsRef<str>,
    ) -> std::io::Result<Self> {
        use etcetera::{AppStrategy, AppStrategyArgs, choose_app_strategy};

        let project = choose_app_strategy(AppStrategyArgs {
            top_level_domain: "rs".to_string(),
            author: "".to_string(),
            app_name: app_name.as_ref().to_string(),
        })
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::NotFound, e.to_string()))?;

        let mut path = project.config_dir();
        path.push(config_name.as_ref());

        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        Ok(Self::new(path))
    }

    #[cfg(feature = "confy-compat-0-6")]
    pub fn for_app_v06(
        app_name: impl AsRef<str>,
        config_name: impl AsRef<str>,
    ) -> std::io::Result<Self> {
        use directories::ProjectDirs;

        let project = ProjectDirs::from("rs", "", app_name.as_ref()).ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "Failed to resolve system application directories",
            )
        })?;

        let mut path = project.config_dir().to_path_buf();
        path.push(config_name.as_ref());

        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        Ok(Self::new(path))
    }
    pub fn debounce(mut self, ms: u64) -> Self {
        self.config.save_debounce = Duration::from_millis(ms);
        self
    }

    pub fn watch_interval(mut self, ms: u64) -> Self {
        self.config.watch_interval = Duration::from_millis(ms);
        self
    }

    pub fn migrations(mut self, configure: impl FnOnce(&mut MigrationBuilder)) -> Self {
        configure(&mut self.migration_builder);
        self
    }

    /// Picks the engine explicitly. Without this the store uses
    /// [`default_backend`].
    pub fn backend(mut self, backend: Backend) -> Self {
        self.backend = backend;
        self
    }

    pub fn build(self) -> StorageResult<Store> {
        let migration_set = self.migration_builder.into_set();
        let (store, _) = self.backend.open_public(self.config, migration_set)?;

        Ok(store)
    }

    pub fn build_with_report(mut self) -> StorageResult<(Store, MigrationReport)> {
        self.migration_builder.collect_codegen();
        let migration_set = self.migration_builder.into_set();
        let (store, report) = self.backend.open_public(self.config, migration_set)?;
        report.log_to_tracing();
        Ok((store, report))
    }
}
