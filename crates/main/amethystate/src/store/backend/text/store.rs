use super::document::TextDocument;
use super::error::TextStoreError;
use crate::MigrationReport;
use crate::errors::StorageError;
use crate::migration::engine::{MigrationEngine, StorageProvider};
use crate::migration::set::MigrationSet;
use crate::store::backend::text::migration::TextMigrationBackend;
use crate::store::backend::utils;
use crate::store::backend::utils::Attempted;
use crate::store::config::{FileWritePolicy, StoreConfig};
use crate::store::durable::{Commit, CommitSignal, PersistHealth};
use crate::store::traits::MigrationBackendAdapter;
use crate::store::util::debouncer::Debouncer;
use crate::store::{
    InitState, SchemaAwareStore, StorageResult, StoreBackend, StoreCallback, StoreEvent, StoreOp,
    SubscriptionEntry, SubscriptionId, SubscriptionKind,
};
use amethystate_core::path::StorePath;
use error_stack::ResultExt;
use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use parking_lot::RwLock;
use std::borrow::Cow;
use std::collections::HashMap;
use std::fmt::Debug;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use tempfile::NamedTempFile;
use tracing::{info, warn};

trait InMetaFile: ResultExt {
    fn in_meta(self, what: StorageError, file: &Path) -> StorageResult<Self::Ok>;
}

impl<R: ResultExt> InMetaFile for R {
    fn in_meta(self, what: StorageError, file: &Path) -> StorageResult<Self::Ok> {
        self.change_context(what)
            .attach_with(|| format!("meta file: {}", file.display()))
    }
}

pub struct StoreFile<D> {
    pub path: PathBuf,
    pub backup_path: PathBuf,
    pub doc: Arc<RwLock<D>>,
    pub write_policy: FileWritePolicy,
}

impl<D> Clone for StoreFile<D> {
    fn clone(&self) -> Self {
        Self {
            path: self.path.clone(),
            backup_path: self.backup_path.clone(),
            doc: self.doc.clone(),
            write_policy: self.write_policy,
        }
    }
}

/// The copy a store keeps of one of its own files while rewriting it.
///
/// The whole name is kept and `.bak` added, rather than the extension swapped:
/// swapping it gives `store.bak` for both `store.db` and `store.meta`, so the
/// second copy lands on the first and the data has no backup left. It also
/// names a file the store did not create - a `store.bak` a person put there
/// themselves - and overwrites it.
fn backup_of(path: &Path) -> PathBuf {
    let mut name = match path.file_name() {
        Some(name) => name.to_os_string(),
        None => return path.with_extension("bak"),
    };
    name.push(".bak");
    path.with_file_name(name)
}

/// One record's key in the metadata file, which is flat.
///
/// Reading the data file needs the schema, and the schema is in here - so this
/// file cannot be laid out by a rule that has to be read out of it. Joining
/// once and storing the result whole keeps it readable with no schema at all.
pub(super) fn meta_key(kind: &str, path: &StorePath) -> StorePath {
    StorePath::segment(kind).join(path)
}

impl<D: TextDocument> StoreFile<D> {
    pub fn new(path: PathBuf, initial_doc: D, write_policy: FileWritePolicy) -> Self {
        let backup_path = backup_of(&path);
        Self {
            path,
            backup_path,
            doc: Arc::new(RwLock::new(initial_doc)),
            write_policy,
        }
    }

    pub fn create_backup(&self) -> StorageResult<()> {
        if self.path.exists() {
            std::fs::copy(&self.path, &self.backup_path)
                .map_err(TextStoreError::from)
                .change_context(StorageError::Open)
                .attach_with(|| format!("file: {}", self.path.display()))
                .attach_with(|| format!("backup: {}", self.backup_path.display()))?;
        }
        Ok(())
    }

    pub fn load_or_empty(&self) -> StorageResult<D> {
        if self.path.exists() {
            let content = std::fs::read_to_string(&self.path)
                .map_err(TextStoreError::from)
                .change_context(StorageError::Open)
                .attach_with(|| format!("file: {}", self.path.display()))?;
            D::parse(&content).attach_with(|| format!("file: {}", self.path.display()))
        } else {
            Ok(D::empty())
        }
    }

    pub fn persist(&self) -> StorageResult<()> {
        let content = self
            .doc
            .read()
            .serialize()
            .attach_with(|| format!("file: {}", self.path.display()))?;
        persist_atomic(&self.path, &content, self.write_policy)
            .map_err(TextStoreError::from)
            .change_context(StorageError::Flush)
            .attach_with(|| format!("file: {}", self.path.display()))?;
        Ok(())
    }

    pub fn restore_from_backup(&self, fallback_to_initial: &D) {
        *self.doc.write() = fallback_to_initial.clone();

        if self.backup_path.exists() {
            let _ = std::fs::copy(&self.backup_path, &self.path);
            let _ = std::fs::remove_file(&self.backup_path);
        } else if self.path.exists() {
            let _ = std::fs::remove_file(&self.path);
        }
    }

    pub fn clean_backup(&self) {
        if self.backup_path.exists() {
            let _ = std::fs::remove_file(&self.backup_path);
        }
    }
}

pub struct StoreFiles<D: TextDocument> {
    pub data: StoreFile<D>,
    pub meta: StoreFile<D>,
}

impl<D: TextDocument> Clone for StoreFiles<D> {
    fn clone(&self) -> Self {
        Self {
            data: self.data.clone(),
            meta: self.meta.clone(),
        }
    }
}

impl<D: TextDocument> StoreFiles<D> {
    pub fn create_backups(&self) -> StorageResult<()> {
        self.data.create_backup().attach("role: the store's data")?;
        self.meta
            .create_backup()
            .attach("role: the store's schema bookkeeping")?;
        Ok(())
    }

    pub fn persist(&self) -> StorageResult<()> {
        self.data.persist().attach("role: the store's data")?;
        self.meta
            .persist()
            .attach("role: the store's schema bookkeeping")?;
        Ok(())
    }

    pub fn clean_backups(&self) {
        self.data.clean_backup();
        self.meta.clean_backup();
    }

    pub fn restore_from_backups(&self, fallback_data: &D, fallback_meta: &D) {
        self.data.restore_from_backup(fallback_data);
        self.meta.restore_from_backup(fallback_meta);
    }
}

pub(crate) struct TextStoreInner<D: TextDocument> {
    pub(crate) files: StoreFiles<D>,
    pub(crate) subscriptions: Arc<RwLock<Vec<SubscriptionEntry>>>,
    pub(crate) next_id: Arc<AtomicU64>,
    pub(crate) debouncer: Arc<Debouncer>,
    pub(crate) commits: Arc<CommitSignal>,
    pub(crate) health: Arc<PersistHealth>,
    /// Bumped by every mutation, and compared against `persisted` to tell
    /// whether the document differs from the file. A flag could not do this:
    /// checking it and acting on it are two steps, and a write landing in
    /// between was either lost or clobbered.
    pub(crate) writes: Arc<AtomicU64>,
    pub(crate) persisted: Arc<AtomicU64>,
    _watch_debouncer: Arc<Debouncer>,
    _watcher: RecommendedWatcher,
}

impl<D: TextDocument> TextStoreInner<D> {
    /// Whether a write may proceed.
    ///
    /// A background flush that has been failing past its budget is an error
    /// the caller can act on, not a reason to take the process down - the
    /// value is refused, what is already buffered keeps being retried, and a
    /// flush that lands clears this. A debouncer thread that is actually dead
    /// is a different thing and still panics: that is a bug here, not a disk.
    pub(crate) fn check_debouncer(&self) -> StorageResult<()> {
        if let Some(reason) = self.health.failure() {
            return Err(error_stack::Report::new(StorageError::CommitFailed)
                .attach(format!("the background flush is not landing: {reason:#}"))
                .attach(
                    "what is already buffered is still being retried, and reads are unaffected",
                ));
        }
        if self.debouncer.is_poisoned() {
            panic!("debouncer thread is dead — store integrity cannot be guaranteed");
        }
        Ok(())
    }
}

impl<D: TextDocument> Drop for TextStoreInner<D> {
    fn drop(&mut self) {
        utils::report_closing_flush(self.save_now(), &self.files.data.path);
    }
}

#[derive(Clone)]
pub struct TextStore<D: TextDocument> {
    pub(crate) inner: Arc<TextStoreInner<D>>,
}

impl<D: TextDocument> PartialEq for TextStore<D> {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.inner, &other.inner)
    }
}
impl<D: TextDocument> Eq for TextStore<D> {}

impl<D: TextDocument> Debug for TextStore<D> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TextStore")
            .field("data_path", &self.inner.files.data.path)
            .field("meta_path", &self.inner.files.meta.path)
            .finish()
    }
}

impl<D: TextDocument + Send + 'static> TextStore<D> {
    pub fn open(
        config: StoreConfig,
        migration_set: MigrationSet,
    ) -> StorageResult<(Self, MigrationReport)> {
        let path = config.path.clone();
        let meta_path = config.path.with_extension("meta");

        let files = StoreFiles {
            data: StoreFile::new(path, D::empty(), config.file_write),
            meta: StoreFile::new(meta_path, D::empty(), config.file_write),
        };

        files.create_backups()?;

        let initial_data = files
            .data
            .load_or_empty()
            .attach("role: the store's data")?;
        let initial_meta = files
            .meta
            .load_or_empty()
            .attach("role: the store's schema bookkeeping")?;

        *files.data.doc.write() = initial_data.clone();
        *files.meta.doc.write() = initial_meta.clone();

        let store = Self::new(config, files)?;

        match store.run_migrations(migration_set) {
            Ok(report) => {
                store.inner.files.persist()?;
                store.inner.files.clean_backups();
                Ok((store, report))
            }
            Err(e) => {
                store
                    .inner
                    .files
                    .restore_from_backups(&initial_data, &initial_meta);
                Err(e
                    .attach(format!("store: {}", store.inner.files.data.path.display()))
                    .attach("the files were restored from their backups"))
            }
        }
    }

    fn new(config: StoreConfig, files: StoreFiles<D>) -> StorageResult<Self> {
        info!(
            path = %config.path.display(),
            "initializing TextStore"
        );

        let subscriptions = Arc::new(RwLock::new(Vec::<SubscriptionEntry>::new()));
        let writes = Arc::new(AtomicU64::new(0));
        let persisted = Arc::new(AtomicU64::new(0));

        let files_debounce = files.clone();
        let writes_debounce = writes.clone();
        let persisted_debounce = persisted.clone();
        let commits = Arc::new(CommitSignal::default());

        let health = Arc::new(PersistHealth::default());

        let debouncer = Debouncer::new_with_retry(
            config.save_debounce,
            crate::store::util::debouncer::FlushPolicy {
                retry: config.retry_policy.clone(),
                commits: commits.clone(),
                health: health.clone(),
                on_giveup: config.on_persist_failure.clone(),
            },
            move || -> StorageResult<()> {
                // Read the generation before serializing. A write landing during
                // the persist bumps it past this, so it stays pending instead of
                // being marked saved without having been written.
                let saving = writes_debounce.load(Ordering::Acquire);
                files_debounce.persist()?;
                persisted_debounce.store(saving, Ordering::Release);
                Ok(())
            },
        );

        let files_watch = files.clone();
        let watch_subs = subscriptions.clone();
        let writes_watch = writes.clone();
        let persisted_watch = persisted.clone();
        let meta_path = files.meta.path.clone();

        // External edits (e.g. a text editor doing truncate-then-write) fire multiple
        // raw filesystem events in quick succession, and reading the file on the very
        // first one can observe a transient, partially-written state (seen as a
        // spurious delete of every key). Debounce so we only re-read once the file
        // has settled, same as we already do for our own outgoing writes.
        let watch_debouncer = Arc::new(Debouncer::new(config.watch_interval, move || {
            sync_external_changes::<D>(
                &files_watch.data,
                &watch_subs,
                &writes_watch,
                &persisted_watch,
            );

            if let Ok(content) = std::fs::read_to_string(&meta_path)
                && let Ok(on_disk) = D::parse(&content)
            {
                let guard = files_watch.meta.doc.read();
                let current_str = guard.serialize().unwrap_or_default();
                let on_disk_str = on_disk.serialize().unwrap_or_default();
                if current_str != on_disk_str {
                    warn!(
                        "⚠️  External modification of metadata file detected! \
                         Metadata must only be mutated via internal migrations."
                    );
                }
            }
        }));

        let watch_debouncer_trigger = watch_debouncer.clone();
        let watcher = notify::recommended_watcher(move |res: notify::Result<Event>| {
            let Ok(event) = res else { return };

            let is_modify = matches!(event.kind, EventKind::Modify(_) | EventKind::Create(_));
            if !is_modify {
                return;
            }

            watch_debouncer_trigger.schedule();
        })
        .map_err(|e| TextStoreError::Watch(e.to_string()))
        .change_context(StorageError::Open)
        .attach_with(|| format!("file: {}", config.path.display()))?;

        let watch_dir = config.path.parent().unwrap_or(Path::new("."));
        let mut watcher = watcher;
        watcher
            .watch(watch_dir, RecursiveMode::NonRecursive)
            .map_err(|e| TextStoreError::Watch(e.to_string()))
            .change_context(StorageError::Open)
            .attach_with(|| format!("watching: {}", watch_dir.display()))
            .attach_with(|| format!("file: {}", config.path.display()))?;

        let inner = Arc::new(TextStoreInner {
            files,
            subscriptions,
            next_id: Arc::new(AtomicU64::new(1)),
            debouncer: Arc::new(debouncer),
            commits,
            health,
            writes,
            persisted,
            _watch_debouncer: watch_debouncer,
            _watcher: watcher,
        });

        Ok(Self { inner })
    }
}

impl<D: TextDocument + Send + 'static> SchemaAwareStore for TextStore<D> {
    fn run_migrations(&self, mset: MigrationSet) -> StorageResult<MigrationReport> {
        struct TextProvider<D: TextDocument> {
            data_doc: Arc<RwLock<D>>,
            meta_doc: Arc<RwLock<D>>,
        }

        impl<D: TextDocument> StorageProvider for TextProvider<D> {
            fn atomic<F, T>(&self, f: F) -> StorageResult<T>
            where
                F: FnOnce(&mut dyn MigrationBackendAdapter) -> StorageResult<T>,
            {
                let mut data_guard = self.data_doc.write();
                let mut meta_guard = self.meta_doc.write();

                let backup_data = data_guard.clone();
                let backup_meta = meta_guard.clone();

                let mut storage = TextMigrationBackend {
                    data_doc: &mut *data_guard,
                    meta_doc: &mut *meta_guard,
                };

                match f(&mut storage) {
                    Ok(val) => Ok(val),
                    Err(e) => {
                        *data_guard = backup_data;
                        *meta_guard = backup_meta;
                        Err(e)
                    }
                }
            }
        }

        let provider = TextProvider {
            data_doc: self.inner.files.data.doc.clone(),
            meta_doc: self.inner.files.meta.doc.clone(),
        };
        let engine = MigrationEngine::new(&provider);
        engine
            .run(mset)
            .doing(StorageError::Migrate, &self.inner.files.data.path)
            .attach_with(|| format!("meta file: {}", self.inner.files.meta.path.display()))
    }
}

impl<D: TextDocument> TextStoreInner<D> {
    fn get_node_bytes(&self, path: &StorePath) -> StorageResult<Option<Vec<u8>>> {
        let guard = self.files.data.doc.read();
        let levels: Vec<Cow<'_, str>> = path.segments().collect();
        let parts: Vec<&str> = levels.iter().map(Cow::as_ref).collect();
        match guard.get(&parts) {
            Some(node) => Ok(Some(
                D::node_to_bytes(node)
                    .doing(StorageError::Read, &self.files.data.path)
                    .attach_with(|| format!("node: {path}"))?,
            )),
            None => Ok(None),
        }
    }

    fn set_erased_inner(
        &self,
        path: &StorePath,
        value: &dyn erased_serde::Serialize,
        source: Option<uuid::Uuid>,
    ) -> StorageResult<()> {
        self.check_debouncer()?;
        let node = D::serialize_node(value)
            .doing(StorageError::Write, &self.files.data.path)
            .attach_with(|| format!("node: {path}"))?;
        self.set_node(path.clone(), node, source)
    }

    fn save_now(&self) -> StorageResult<()> {
        let saving = self.writes.load(Ordering::Acquire);
        self.files.persist()?;
        self.persisted.store(saving, Ordering::Release);
        Ok(())
    }

    /// Picks up an edit made to the file outside the process before writing our
    /// own, unless we have unsaved changes of our own to lose.
    fn pull_external_changes(&self) {
        sync_external_changes::<D>(
            &self.files.data,
            &self.subscriptions,
            &self.writes,
            &self.persisted,
        );
    }

    fn scan_prefix(&self, prefix: &StorePath) -> StorageResult<Vec<(StorePath, Vec<u8>)>> {
        let guard = self.files.data.doc.read();
        scan_prefix_impl(&*guard, prefix)
            .attach_with(|| format!("file: {}", self.files.data.path.display()))
    }

    fn scan_keys(&self, prefix: &StorePath) -> StorageResult<Vec<StorePath>> {
        let guard = self.files.data.doc.read();
        scan_keys_impl(&*guard, prefix)
            .attach_with(|| format!("file: {}", self.files.data.path.display()))
    }

    fn delete(&self, path: &StorePath, source: Option<uuid::Uuid>) -> StorageResult<()> {
        self.check_debouncer()?;

        self.pull_external_changes();

        let levels: Vec<Cow<'_, str>> = path.segments().collect();
        let parts: Vec<&str> = levels.iter().map(Cow::as_ref).collect();

        let old_bytes = {
            let mut guard = self.files.data.doc.write();
            let old = guard
                .get(&parts)
                .map(|n| D::node_to_bytes(n))
                .transpose()
                .doing(StorageError::Delete, &self.files.data.path)
                .attach_with(|| format!("node: {path}"))?;
            guard
                .delete(&parts)
                .doing(StorageError::Delete, &self.files.data.path)
                .attach_with(|| format!("node: {path}"))?;
            old
        };

        let Some(old_bytes) = old_bytes else {
            return Ok(());
        };

        self.writes.fetch_add(1, Ordering::Release);

        utils::emit_events(
            &self.subscriptions,
            StoreEvent {
                path: Arc::from(path.as_str()),
                op: StoreOp::Delete,
                old: Some(old_bytes),
                new: None,
                source,
            },
        );

        self.debouncer.schedule();
        Ok(())
    }

    fn delete_prefix(&self, prefix: &StorePath, source: Option<uuid::Uuid>) -> StorageResult<()> {
        self.check_debouncer()?;

        self.pull_external_changes();

        {
            let levels: Vec<Cow<'_, str>> = prefix.segments().collect();
            let parts: Vec<&str> = levels.iter().map(Cow::as_ref).collect();
            self.files
                .data
                .doc
                .write()
                .delete_subtree(&parts)
                .doing(StorageError::Delete, &self.files.data.path)
                .attach_with(|| format!("prefix: {prefix}"))?;
        }

        self.writes.fetch_add(1, Ordering::Release);

        utils::emit_events(
            &self.subscriptions,
            StoreEvent {
                path: Arc::from(prefix.as_str()),
                op: StoreOp::DeletePrefix,
                old: None,
                new: None,
                source,
            },
        );

        self.debouncer.schedule();
        Ok(())
    }

    fn subscribe(&self, kind: SubscriptionKind, callback: StoreCallback) -> SubscriptionId {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        self.subscriptions
            .write()
            .push(SubscriptionEntry { id, kind, callback });
        id
    }

    fn unsubscribe(&self, id: SubscriptionId) {
        self.subscriptions.write().retain(|s| s.id != id);
    }

    fn init_key(&self, namespace: &str) -> StorageResult<StorePath> {
        let path = StorePath::parse_joined(namespace)
            .in_meta(StorageError::Meta, &self.files.meta.path)
            .attach_with(|| format!("namespace: {namespace}"))?;
        Ok(meta_key("__init", &path))
    }

    fn is_initialized(&self, namespace: &str) -> StorageResult<bool> {
        let key = self.init_key(namespace)?;
        let guard = self.files.meta.doc.read();
        Ok(guard.get(&[key.as_str()]).is_some())
    }

    fn set_initialized(&self, namespace: &str, state: InitState) -> StorageResult<()> {
        let key = self.init_key(namespace)?;
        {
            let mut guard = self.files.meta.doc.write();
            let parts = [key.as_str()];

            match state {
                InitState::Seeded => {
                    let node = D::serialize_node(&true)
                        .in_meta(StorageError::Meta, &self.files.meta.path)
                        .attach_with(|| format!("namespace: {namespace}"))?;
                    guard.set(&parts, node)
                }
                InitState::Fresh => guard.delete(&parts).map(|_| ()),
            }
            .in_meta(StorageError::Meta, &self.files.meta.path)
            .attach_with(|| format!("namespace: {namespace}"))?;
        }

        self.files
            .meta
            .persist()
            .change_context(StorageError::Meta)
            .attach_with(|| format!("namespace: {namespace}"))?;
        Ok(())
    }

    /// Writes `node` at `path_str`, reporting a removal if the document does
    /// not keep it - a format with no way to write nothing answers a `None`
    /// with an absent key.
    pub(crate) fn set_node(
        &self,
        path_str: StorePath,
        node: D::Node,
        source: Option<uuid::Uuid>,
    ) -> StorageResult<()> {
        self.pull_external_changes();

        let levels: Vec<Cow<'_, str>> = path_str.segments().collect();
        let parts: Vec<&str> = levels.iter().map(Cow::as_ref).collect();
        let (old_bytes, new_bytes) = {
            let mut guard = self.files.data.doc.write();
            let old = guard
                .get(&parts)
                .map(|n| D::node_to_bytes(n))
                .transpose()
                .doing(StorageError::Write, &self.files.data.path)
                .attach_with(|| format!("node: {path_str}"))
                .attach("while reading the value being replaced")?;
            guard
                .set(&parts, node)
                .doing(StorageError::Write, &self.files.data.path)
                .attach_with(|| format!("node: {path_str}"))?;
            let new = guard
                .get(&parts)
                .map(|n| D::node_to_bytes(n))
                .transpose()
                .doing(StorageError::Write, &self.files.data.path)
                .attach_with(|| format!("node: {path_str}"))?;
            (old, new)
        };

        self.writes.fetch_add(1, Ordering::Release);

        let event = match new_bytes {
            Some(new) => StoreEvent {
                path: Arc::from(path_str.as_str()),
                op: StoreOp::Set,
                old: old_bytes,
                new: Some(new),
                source,
            },
            None => {
                let Some(old) = old_bytes else {
                    self.debouncer.schedule();
                    return Ok(());
                };
                StoreEvent {
                    path: Arc::from(path_str.as_str()),
                    op: StoreOp::Delete,
                    old: Some(old),
                    new: None,
                    source,
                }
            }
        };

        utils::emit_events(&self.subscriptions, event);

        self.debouncer.schedule();
        Ok(())
    }
}

impl<D: TextDocument + Send + 'static> StoreBackend for TextStore<D> {
    fn get_raw(&self, path: &StorePath) -> StorageResult<Option<Vec<u8>>> {
        self.inner.get_node_bytes(path)
    }

    fn get_erased(
        &self,
        path: &StorePath,
        f: &mut dyn FnMut(&mut dyn erased_serde::Deserializer) -> StorageResult<()>,
    ) -> StorageResult<bool> {
        match self.inner.get_node_bytes(path)? {
            Some(bytes) => {
                D::with_bytes_de(&bytes, f)
                    .doing(StorageError::Read, &self.inner.files.data.path)
                    .attach_with(|| format!("node: {path}"))?;
                Ok(true)
            }
            None => Ok(false),
        }
    }

    fn decode_erased(
        &self,
        bytes: &[u8],
        f: &mut dyn FnMut(&mut dyn erased_serde::Deserializer) -> StorageResult<()>,
    ) -> StorageResult<()> {
        D::with_bytes_de(bytes, f)
            .attach_with(|| format!("file: {}", self.inner.files.data.path.display()))
    }

    fn set_erased(
        &self,
        path: &StorePath,
        value: &dyn erased_serde::Serialize,
        source: Option<uuid::Uuid>,
    ) -> StorageResult<()> {
        self.inner.set_erased_inner(path, value, source)
    }

    fn set_owned_erased(
        &self,
        path: StorePath,
        value: &dyn erased_serde::Serialize,
        source: Option<uuid::Uuid>,
    ) -> StorageResult<()> {
        self.inner.set_erased_inner(&path, value, source)
    }

    fn save_now(&self) -> StorageResult<()> {
        self.inner.save_now()
    }

    fn scan_prefix(&self, prefix: &StorePath) -> StorageResult<Vec<(StorePath, Vec<u8>)>> {
        self.inner.scan_prefix(prefix)
    }

    fn scan_keys(&self, prefix: &StorePath) -> StorageResult<Vec<StorePath>> {
        self.inner.scan_keys(prefix)
    }

    fn delete(&self, path: &StorePath) -> StorageResult<()> {
        self.delete_with_source(path, None)
    }

    fn delete_with_source(
        &self,
        path: &StorePath,
        source: Option<uuid::Uuid>,
    ) -> StorageResult<()> {
        self.inner.delete(path, source)
    }

    fn delete_prefix_with_source(
        &self,
        prefix: &StorePath,
        source: Option<uuid::Uuid>,
    ) -> StorageResult<()> {
        self.inner.delete_prefix(prefix, source)
    }

    fn subscribe(&self, kind: SubscriptionKind, callback: StoreCallback) -> SubscriptionId {
        self.inner.subscribe(kind, callback)
    }

    fn unsubscribe(&self, id: SubscriptionId) {
        self.inner.unsubscribe(id)
    }

    fn flush_async(&self) -> Commit {
        let commit = Commit::awaiting(self.inner.commits.clone());
        self.inner.debouncer.flush_now();
        commit
    }

    fn flush_prefix(&self, prefix: &StorePath) -> StorageResult<()> {
        self.save_now().attach_with(|| format!("prefix: {prefix}"))
    }

    fn is_initialized(&self, namespace: &str) -> StorageResult<bool> {
        self.inner.is_initialized(namespace)
    }

    fn set_initialized(&self, namespace: &str, state: InitState) -> StorageResult<()> {
        self.inner.set_initialized(namespace, state)
    }
}

/// Writes `content` where `path` names, so that a reader sees either the whole
/// of it or none.
///
/// The temporary file is made in the target's own directory, because a
/// replacement has to sit on the same volume, and the contents are flushed
/// before the name is moved: otherwise the rename can reach the disk while the
/// bytes are still in the write-back cache, which is how a config file comes
/// back truncated after a power cut. Windows offers no write-through on the
/// replacement itself, so the flush has to be ours.
///
/// A replacement that has to be retried takes the same temporary file back from
/// the failure and tries again with it: the contents are written and flushed
/// already, and only the name is in dispute.
///
/// How long each of the two steps is worth is [`FileWritePolicy`], because what
/// is holding the file is the application's business and not this function's.
fn persist_atomic(path: &Path, content: &str, policy: FileWritePolicy) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let dir = path.parent().unwrap_or(Path::new("."));

    let mut written = None;
    for attempt in 0..policy.write.attempts.max(1) {
        match write_temp(dir, content) {
            Ok(tmp) => {
                written = Some(tmp);
                break;
            }
            Err(e) if attempt + 1 >= policy.write.attempts => return Err(e),
            Err(_) => std::thread::sleep(policy.write.pause),
        }
    }
    let mut tmp = written.expect("the loop above returns rather than falling through");

    for attempt in 0..policy.replace.attempts.max(1) {
        match tmp.persist(path) {
            Ok(_) => return Ok(()),
            Err(e) if attempt + 1 >= policy.replace.attempts => return Err(e.error),
            Err(e) => {
                tmp = e.file;
                std::thread::sleep(policy.replace.pause);
            }
        }
    }
    unreachable!("the loop above returns on its last attempt")
}

/// The contents in a file of their own, beside the target and already on the
/// disk.
fn write_temp(dir: &Path, content: &str) -> io::Result<NamedTempFile> {
    let mut tmp = NamedTempFile::new_in(dir)?;
    tmp.write_all(content.as_bytes())?;
    tmp.as_file().sync_all()?;
    Ok(tmp)
}

pub(super) fn scan_prefix_impl<D: TextDocument>(
    doc: &D,
    prefix: &StorePath,
) -> StorageResult<Vec<(StorePath, Vec<u8>)>> {
    let levels: Vec<Cow<'_, str>> = prefix.segments().collect();
    let parts: Vec<&str> = levels.iter().map(Cow::as_ref).collect();
    let target_depth = parts.len() + 1;
    let mut raw_nodes = Vec::new();
    scan_prefix_recursive(
        doc,
        &parts,
        prefix.as_str(),
        &mut raw_nodes,
        Some(target_depth),
    )?;

    let mut results = Vec::new();
    for (k, node) in raw_nodes {
        if k.starts_with(prefix.as_str()) {
            let bytes = D::node_to_bytes(&node)
                .change_context(StorageError::Scan)
                .attach_with(|| format!("prefix: {prefix}"))
                .attach_with(|| format!("node: {k}"))?;
            results.push((utils::stored_path(&k)?, bytes));
        }
    }

    results.sort_by(|(a, _), (b, _)| a.as_str().cmp(b.as_str()));
    Ok(results)
}

pub(super) fn scan_prefix_recursive<D: TextDocument>(
    doc: &D,
    parts: &[&str],
    prefix_str: &str,
    results: &mut Vec<(String, D::Node)>,
    target_depth: Option<usize>,
) -> StorageResult<()> {
    let current_depth = parts.len();

    if let Some(target_depth) = target_depth
        && current_depth >= target_depth
    {
        if !prefix_str.is_empty()
            && let Some(node) = doc.get(parts)
        {
            results.push((prefix_str.to_string(), node.clone()));
        }
        return Ok(());
    }

    let children = doc.scan(parts)?;
    if children.is_empty() {
        if !prefix_str.is_empty()
            && let Some(node) = doc.get(parts)
        {
            results.push((prefix_str.to_string(), node.clone()));
        }
    } else {
        for (full_key, _node) in children {
            let child_path = StorePath::parse_joined(&full_key)
                .change_context(StorageError::Scan)
                .attach_with(|| format!("stored key: {full_key}"))
                .attach("the document holds a key this library could not have written")?;
            let child_levels: Vec<Cow<'_, str>> = child_path.segments().collect();
            let child_parts: Vec<&str> = child_levels.iter().map(Cow::as_ref).collect();
            let grand_children = doc.scan(&child_parts)?;

            let should_stop = grand_children.is_empty()
                || target_depth.is_some_and(|depth| child_parts.len() >= depth);

            if should_stop {
                if let Some(child_node) = doc.get(&child_parts) {
                    results.push((full_key, child_node.clone()));
                }
            } else {
                scan_prefix_recursive(doc, &child_parts, prefix_str, results, target_depth)?;
            }
        }
    }

    Ok(())
}

fn sync_external_changes<D: TextDocument>(
    file: &StoreFile<D>,
    subscriptions: &Arc<RwLock<Vec<SubscriptionEntry>>>,
    writes: &AtomicU64,
    persisted: &AtomicU64,
) {
    let Ok(content) = std::fs::read_to_string(&file.path) else {
        return;
    };
    let Ok(on_disk) = D::parse(&content) else {
        return;
    };

    let events = {
        let mut guard = file.doc.write();

        // Under the same guard a write takes, so this cannot be overtaken:
        // either the write landed first and is seen here, or it lands after
        // and applies on top. Checking before taking the guard let a write
        // slip into the gap and be overwritten with what was read from disk.
        if writes.load(Ordering::Acquire) != persisted.load(Ordering::Acquire) {
            return;
        }

        let old_serialized = guard.serialize().unwrap_or_default();
        let new_serialized = on_disk.serialize().unwrap_or_default();
        if old_serialized == new_serialized {
            Vec::new()
        } else {
            let old = guard.clone();
            *guard = on_disk;
            info!("external store change detected");
            match diff_documents::<D>(&old, &*guard) {
                Ok(events) => events,
                Err(e) => {
                    tracing::error!(
                        "an external edit could not be read, so nobody was told about it: {e:?}"
                    );
                    return;
                }
            }
        }
    };
    for event in events {
        utils::emit_events(subscriptions, event);
    }
}

fn diff_documents<D: TextDocument>(old: &D, new: &D) -> StorageResult<Vec<StoreEvent>> {
    let mut old_nodes = Vec::new();
    scan_prefix_recursive(old, &[], "", &mut old_nodes, None)
        .attach("reading the document as it was before the edit")?;
    let old_map: HashMap<String, D::Node> = old_nodes.into_iter().collect();

    let mut new_nodes = Vec::new();
    scan_prefix_recursive(new, &[], "", &mut new_nodes, None)
        .attach("reading the document as it is on disk")?;
    let new_map: HashMap<String, D::Node> = new_nodes.into_iter().collect();

    let mut events = Vec::new();

    let mut all_keys: std::collections::BTreeSet<String> = old_map.keys().cloned().collect();
    all_keys.extend(new_map.keys().cloned());

    for key in all_keys {
        let old_node = old_map.get(&key);
        let new_node = new_map.get(&key);

        match (old_node, new_node) {
            (Some(o), Some(n)) => {
                let old_bytes = D::node_to_bytes(o).ok();
                let new_bytes = D::node_to_bytes(n).ok();
                if old_bytes != new_bytes {
                    events.push(StoreEvent {
                        path: Arc::from(key),
                        op: StoreOp::Set,
                        old: old_bytes,
                        new: new_bytes,
                        source: None,
                    });
                }
            }
            (Some(o), None) => {
                let old_bytes = D::node_to_bytes(o).ok();
                events.push(StoreEvent {
                    path: Arc::from(key),
                    op: StoreOp::Delete,
                    old: old_bytes,
                    new: None,
                    source: None,
                });
            }
            (None, Some(n)) => {
                let new_bytes = D::node_to_bytes(n).ok();
                events.push(StoreEvent {
                    path: Arc::from(key),
                    op: StoreOp::Set,
                    old: None,
                    new: new_bytes,
                    source: None,
                });
            }
            (None, None) => {}
        }
    }

    Ok(events)
}

pub(super) fn scan_keys_impl<D: TextDocument>(
    doc: &D,
    prefix: &StorePath,
) -> StorageResult<Vec<StorePath>> {
    let levels: Vec<Cow<'_, str>> = prefix.segments().collect();
    let parts: Vec<&str> = levels.iter().map(Cow::as_ref).collect();
    let target_depth = parts.len() + 1;
    let mut keys = Vec::new();
    scan_keys_recursive(doc, &parts, prefix.as_str(), &mut keys, Some(target_depth))?;

    keys.retain(|k| k.starts_with(prefix.as_str()));
    keys.sort();

    keys.iter().map(|k| utils::stored_path(k)).collect()
}

fn scan_keys_recursive<D: TextDocument>(
    doc: &D,
    parts: &[&str],
    prefix_str: &str,
    keys: &mut Vec<String>,
    target_depth: Option<usize>,
) -> StorageResult<()> {
    let current_depth = parts.len();

    if let Some(target_depth) = target_depth
        && current_depth >= target_depth
    {
        if !prefix_str.is_empty() && doc.get(parts).is_some() {
            keys.push(prefix_str.to_string());
        }
        return Ok(());
    }

    let children = doc.scan(parts)?;
    if children.is_empty() {
        if !prefix_str.is_empty() && doc.get(parts).is_some() {
            keys.push(prefix_str.to_string());
        }
    } else {
        for (full_key, _node) in children {
            let child_path = StorePath::parse_joined(&full_key)
                .change_context(StorageError::Scan)
                .attach_with(|| format!("stored key: {full_key}"))
                .attach("the document holds a key this library could not have written")?;
            let child_levels: Vec<Cow<'_, str>> = child_path.segments().collect();
            let child_parts: Vec<&str> = child_levels.iter().map(Cow::as_ref).collect();
            let grand_children = doc.scan(&child_parts)?;

            let should_stop = grand_children.is_empty()
                || target_depth.is_some_and(|depth| child_parts.len() >= depth);

            if should_stop {
                if doc.get(&child_parts).is_some() {
                    keys.push(full_key);
                }
            } else {
                scan_keys_recursive(doc, &child_parts, prefix_str, keys, target_depth)?;
            }
        }
    }

    Ok(())
}
