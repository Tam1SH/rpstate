use crate::store::StorageResult;
use std::path::PathBuf;
use std::time::Duration;

use crate::migration::builder::MigrationBuilder;

use crate::store::config::StoreConfig;
use crate::{MigrationReport, Store};

/// Which engine backs a store.
///
/// A variant exists for each backend feature that is enabled. Pass one to
/// [`StoreBuilder::backend`]; without that, [`default_backend`] picks.
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

    /// Opens a store on this engine directly, skipping the builder.
    ///
    /// [`StoreBuilder`] is the ordinary route - it also collects migrations
    /// and settings this does not.
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

/// The engine used when the caller does not name one.
///
/// The first of redb, sqlite, json, toml, ron that is enabled. Naming the
/// engine with [`StoreBuilder::backend`] is worth doing wherever it matters
/// which one runs - the on-disk format differs, and so does what a durable
/// write commits.
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
    /// A store at an explicit path.
    ///
    /// The extension is left as given; [`StoreBuilder::for_app`] is the
    /// variant that picks a location and an extension for you.
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
    /// - `confy-compat-0-6`: uses the `directories` crate (legacy `confy` v0.6 behavior)
    /// - default: uses the `etcetera` crate (XDG / native OS strategy)
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

    /// [`StoreBuilder::for_app`] pinned to the `etcetera` layout, whatever
    /// the feature flags say.
    ///
    /// Worth naming explicitly when a config location must not move because a
    /// dependency elsewhere in the build turned a compatibility feature on.
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
    /// [`StoreBuilder::for_app`] pinned to the layout `confy` 0.6 used, by way
    /// of the `directories` crate.
    ///
    /// For reading configuration an older version of the application wrote;
    /// [`StoreBuilder::for_app_v2`] is the current layout.
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
    /// How long a write waits in the buffer before it is flushed, in
    /// milliseconds.
    ///
    /// Raising this batches more writes into one commit; lowering it narrows
    /// the window a crash can take. Neither affects reads, which see buffered
    /// writes immediately either way.
    pub fn debounce(mut self, ms: u64) -> Self {
        self.config.save_debounce = Duration::from_millis(ms);
        self
    }

    /// How often the file watcher polls for changes made outside the
    /// process, in milliseconds.
    pub fn watch_interval(mut self, ms: u64) -> Self {
        self.config.watch_interval = Duration::from_millis(ms);
        self
    }

    /// Declares migration steps to run when the store opens.
    ///
    /// Steps written with `#[migrate]` are collected automatically by
    /// [`StoreBuilder::build_with_report`]; this is for the ones built by
    /// hand.
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

    /// Opens the store.
    ///
    /// ```
    /// # use amethystate::StoreBuilder;
    /// # let path = amethystate_core::test_utils::TempPath::new("doc");
    /// let store = StoreBuilder::new(&*path).build().unwrap();
    /// store.kv().set("a", &1u8).unwrap();
    /// assert_eq!(store.kv().get::<u8>("a").unwrap(), Some(1));
    /// ```
    pub fn build(self) -> StorageResult<Store> {
        let migration_set = self.migration_builder.into_set();
        let (store, _) = self.backend.open_public(self.config, migration_set)?;

        Ok(store)
    }

    /// Opens the store and returns what the migration pass did.
    ///
    /// This is also the path that collects `#[migrate]` steps, so a store
    /// opened with [`StoreBuilder::build`] runs only the migrations declared
    /// by hand.
    pub fn build_with_report(mut self) -> StorageResult<(Store, MigrationReport)> {
        self.migration_builder.collect_codegen();
        let migration_set = self.migration_builder.into_set();
        let (store, report) = self.backend.open_public(self.config, migration_set)?;
        report.log_to_tracing();
        Ok((store, report))
    }
}
