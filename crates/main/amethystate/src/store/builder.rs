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
    /// Whether the extension on the path was spelled by the caller.
    ///
    /// An extension this crate chose belongs to whichever engine is going to
    /// run, and so has to follow [`StoreBuilder::backend`]; one the caller
    /// wrote is theirs, and naming an engine does not overrule it.
    caller_named_extension: bool,
}

impl StoreBuilder {
    /// A store at an explicit path.
    ///
    /// An extension that is given is kept, whatever engine ends up running -
    /// the path is the caller's. Without one the engine's own extension is
    /// used, and it follows [`StoreBuilder::backend`] if that names another
    /// engine later. [`StoreBuilder::for_app`] is the variant that picks a
    /// location as well.
    pub fn new(path: impl Into<PathBuf>) -> Self {
        let mut path: PathBuf = path.into();
        let caller_named_extension = path.extension().is_some();
        if !caller_named_extension {
            path.set_extension(default_backend().extension());
        }
        Self {
            backend: default_backend(),
            config: StoreConfig::new(path),
            migration_builder: MigrationBuilder::default(),
            caller_named_extension,
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
    /// How long a write waits in the buffer before it is flushed.
    ///
    /// Raising this batches more writes into one commit; lowering it narrows
    /// the window a crash can take. Neither affects reads, which see buffered
    /// writes immediately either way.
    pub fn debounce(mut self, every: Duration) -> Self {
        self.config.save_debounce = every;
        self
    }

    /// How often the file watcher polls for changes made outside the process.
    pub fn watch_interval(mut self, every: Duration) -> Self {
        self.config.watch_interval = every;
        self
    }

    /// How long a failed background flush waits before trying again.
    ///
    /// A retry is not a second write: it is the same buffered changes,
    /// tried again. Nothing is lost between attempts.
    pub fn retry_interval(mut self, every: Duration) -> Self {
        self.config.retry_policy.interval = every;
        self
    }

    /// How long a streak of failing flushes may run before the store says so
    /// out loud.
    ///
    /// Not a deadline for giving up: the flush keeps being retried until it
    /// lands or the store is dropped, so a disk someone frees up heals it
    /// without a restart. This bounds how long that goes on quietly before
    /// [`StoreBuilder::on_persist_failure`] is asked what writers should be
    /// told.
    pub fn retry_budget(mut self, within: Duration) -> Self {
        self.config.retry_policy.budget = within;
        self
    }

    /// Runs once per failing streak, with a rendered reason, when a flush
    /// has been failing for longer than the retry budget - after any write
    /// awaiting that flush has been told it failed.
    ///
    /// What it returns decides what writers see from then until a flush
    /// lands: an error each ([`AfterGivingUp::Fail`], the default without a
    /// callback), nothing at all ([`AfterGivingUp::Ignore`]), or a panic
    /// ([`AfterGivingUp::Poison`]).
    ///
    /// [`AfterGivingUp::Fail`]: crate::store::config::AfterGivingUp::Fail
    /// [`AfterGivingUp::Ignore`]: crate::store::config::AfterGivingUp::Ignore
    /// [`AfterGivingUp::Poison`]: crate::store::config::AfterGivingUp::Poison
    pub fn on_persist_failure<F>(mut self, callback: F) -> Self
    where
        F: Fn(&str) -> crate::store::config::AfterGivingUp + Send + Sync + 'static,
    {
        self.config.on_persist_failure = Some(std::sync::Arc::new(callback));
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

    /// Hands a value to every migration step that runs when this store opens.
    ///
    /// A step written with `#[migrate]` is collected at link time as a bare
    /// `fn(&mut MigrationContext)`, so it captures nothing: anything it needs
    /// from the application - a lookup table, a client, the settings it is
    /// porting away from - has no way in except a global. This is that way in.
    ///
    /// One value per type; the step asks for it back with
    /// [`MigrationContext::provided`] or [`MigrationContext::require`].
    ///
    /// ```
    /// # use amethystate::StoreBuilder;
    /// # let path = amethystate_core::test_utils::TempPath::new("doc");
    /// struct LegacyDefaults {
    ///     port: u16,
    /// }
    ///
    /// let store = StoreBuilder::new(&*path)
    ///     .provide(LegacyDefaults { port: 8080 })
    ///     .build()
    ///     .unwrap();
    /// # let _ = store;
    /// ```
    ///
    /// [`MigrationContext::provided`]: crate::MigrationContext::provided
    /// [`MigrationContext::require`]: crate::MigrationContext::require
    pub fn provide<T: std::any::Any>(mut self, value: T) -> Self {
        self.migration_builder.provide(value);
        self
    }

    /// Lets reading a large collection back use more than one core.
    ///
    /// Parsing every stored key and decoding every value is around four
    /// hundred milliseconds of a million-entry open, and dividing them takes
    /// that to about eighty. Off by default: this is a thread pool inside a
    /// state library, and an application that already has one should say
    /// whether it wants a second. While it is off nothing is spawned - the
    /// pool is built on first use.
    ///
    /// Small collections are unaffected either way: below roughly a thousand
    /// entries the handing out costs more than the work, and the split does
    /// not happen.
    pub fn parallel_reads(mut self, yes: bool) -> Self {
        self.config.parallel_reads = yes;
        self
    }

    /// Picks the engine explicitly. Without this the store uses
    /// [`default_backend`].
    ///
    /// An extension this crate chose moves with the engine: a path left
    /// without one by [`StoreBuilder::new`] or [`StoreBuilder::for_app`] is
    /// named for whichever engine actually runs, so a store asked for as
    /// `json` is not opened on a file called `.redb`. An extension the caller
    /// spelled stays as it is.
    pub fn backend(mut self, backend: Backend) -> Self {
        self.backend = backend;
        if !self.caller_named_extension {
            self.config.path.set_extension(backend.extension());
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    /// A path with no extension gets one from the engine that is actually
    /// going to open it, whenever the engine is named. Otherwise a store asked
    /// for as `json` is opened on a file this crate called `.redb`, and the
    /// engine meets another engine's bytes: `for_app` names a config without an
    /// extension, so this is the ordinary way to reach it rather than a corner.
    #[cfg(all(feature = "redb", feature = "json"))]
    #[test]
    fn a_defaulted_extension_follows_the_engine_that_is_named() {
        let builder = StoreBuilder::new("app/settings").backend(Backend::Json);

        assert_eq!(
            builder.config.path.extension().and_then(|e| e.to_str()),
            Some("json"),
            "the engine was named after the path was built, and the path did not follow"
        );
    }

    /// The engine named last is the one that runs, so it is the one the
    /// extension has to agree with.
    #[cfg(all(feature = "redb", feature = "json", feature = "toml"))]
    #[test]
    fn the_last_engine_named_is_the_one_the_path_follows() {
        let builder = StoreBuilder::new("app/settings")
            .backend(Backend::Json)
            .backend(Backend::Toml);

        assert_eq!(
            builder.config.path.extension().and_then(|e| e.to_str()),
            Some("toml")
        );
    }

    /// An extension the caller spelled is the caller's, and naming an engine
    /// does not overrule it - a `.conf` a user's tooling already looks for
    /// stays `.conf`.
    #[cfg(all(feature = "redb", feature = "json"))]
    #[test]
    fn an_extension_the_caller_wrote_is_left_alone() {
        let builder = StoreBuilder::new("app/settings.conf").backend(Backend::Json);

        assert_eq!(
            builder.config.path.extension().and_then(|e| e.to_str()),
            Some("conf")
        );
    }

    /// Without a named engine the default's extension is right, and stays.
    #[test]
    fn an_unnamed_engine_keeps_the_default_extension() {
        let builder = StoreBuilder::new("app/settings");

        assert_eq!(
            builder.config.path.extension().and_then(|e| e.to_str()),
            Some(default_backend().extension())
        );
    }
}
