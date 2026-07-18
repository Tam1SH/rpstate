use crate::store::StorageResult;
use std::path::PathBuf;
use std::time::Duration;

use crate::migration::builder::MigrationBuilder;

use crate::store::config::StoreConfig;
use crate::{DefaultStore, MigrationReport};

#[cfg(backend = "redb")]
const FILE_EXTENSION: &str = "redb";

#[cfg(backend = "json")]
const FILE_EXTENSION: &str = "json";

#[cfg(backend = "toml")]
const FILE_EXTENSION: &str = "toml";

#[cfg(backend = "ron")]
const FILE_EXTENSION: &str = "ron";

#[cfg(backend = "sqlite")]
const FILE_EXTENSION: &str = "db";

pub struct StoreBuilder {
    config: StoreConfig,
    migration_builder: MigrationBuilder,
}

impl StoreBuilder {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        let mut path: PathBuf = path.into();
        if path.extension().is_none() {
            path.set_extension(FILE_EXTENSION);
        }
        Self {
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

    pub fn build(self) -> StorageResult<DefaultStore> {
        let migration_set = self.migration_builder.into_set();
        let (store, _) = DefaultStore::open(self.config, migration_set)?;

        Ok(store)
    }

    pub fn build_with_report(mut self) -> StorageResult<(DefaultStore, MigrationReport)> {
        self.migration_builder.collect_codegen();
        let migration_set = self.migration_builder.into_set();
        let (store, report) = DefaultStore::open(self.config, migration_set)?;
        report.log_to_tracing();
        Ok((store, report))
    }
}
