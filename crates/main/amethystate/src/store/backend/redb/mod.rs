use crate::store::{
    InitState, SchemaAwareStore, StoreBackend, StoreCallback, StoreEvent, StoreOp,
    SubscriptionEntry, SubscriptionId, SubscriptionKind,
};
use amethystate_core::path::StorePath;
use error_stack::ResultExt;
use migration::RedbMigrationBackend;
use redb::{Database, ReadableDatabase, TableHandle};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::Path;
use tables::{TABLE_DATA, TABLE_DIFF_LOG, TABLE_META, TABLE_MIGRATION_LOG};

use crate::store::config::StoreConfig;
use crate::{
    MigrationReport,
    store::error::{StorageError, StorageResult},
};

use crate::codec::CodecError;
use crate::migration::engine::{MigrationEngine, StorageProvider};
use crate::migration::set::MigrationSet;
use crate::store::backend::redb::tables::TABLE_SCHEMA_SNAPSHOT;
use crate::store::backend::utils;
use crate::store::backend::utils::Attempted;
use crate::store::traits::MigrationBackendAdapter;
use crate::store::util::debouncer::Debouncer;
use parking_lot::{Mutex, RwLock};
use rmp_serde::Serializer;
use rmp_serde::config::BytesMode;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use tracing::info;
use uuid::Uuid;

pub mod error;
mod inspector;
mod migration;
mod recovery;
mod tables;

use recovery::{OpenDatabase, create_database, is_previous_io, reopen};

const BUF_SIZE: usize = 64 * 1024;

#[cfg(test)]
static SIMULATE_WRITE_FAILURE: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

thread_local! {
    static SERIALIZATION_BUFFER: std::cell::RefCell<Vec<u8>> =
        std::cell::RefCell::new(Vec::with_capacity(BUF_SIZE));
}

/// Writes every buffered change into `txn`'s tables. Committing `txn` is the
/// caller's own last step, not this function's - a synchronous flush that
/// reports its error immediately and a retried background one both walk the
/// same changes the same way, and only differ in what happens after this
/// returns.
fn apply_pending(
    txn: &redb::WriteTransaction,
    changes: &utils::Pending,
    path: &Path,
) -> StorageResult<()> {
    let mut table = txn
        .open_table(TABLE_DATA)
        .doing(StorageError::Flush, path)
        .attach_with(|| format!("table: {}", TABLE_DATA.name()))?;
    let mut meta = txn
        .open_table(TABLE_META)
        .doing(StorageError::Flush, path)
        .attach_with(|| format!("table: {}", TABLE_META.name()))?;

    for (key, op) in changes {
        match op {
            utils::PendingOp::Set(b) => {
                table
                    .insert(&**key, &b[..])
                    .doing(StorageError::Flush, path)
                    .attach_with(|| format!("table: {}", TABLE_DATA.name()))
                    .attach_with(|| format!("key: {key}"))
                    .attach_with(|| format!("value: {} bytes", b.len()))?;
            }
            utils::PendingOp::Delete => {
                table
                    .remove(&**key)
                    .doing(StorageError::Flush, path)
                    .attach_with(|| format!("table: {}", TABLE_DATA.name()))
                    .attach_with(|| format!("key: {key}"))?;
            }
            utils::PendingOp::Init(seeded) => {
                let init_key = utils::init_key(key);
                if *seeded {
                    meta.insert(init_key.as_str(), &[][..]).map(|_| ())
                } else {
                    meta.remove(init_key.as_str()).map(|_| ())
                }
                .doing(StorageError::Flush, path)
                .attach_with(|| format!("table: {}", TABLE_META.name()))
                .attach_with(|| format!("namespace: {key}"))?;
            }
        }
    }

    Ok(())
}

struct RedbStoreInner {
    db: OpenDatabase,
    path: Arc<Path>,
    pending: Arc<Mutex<utils::Pending>>,
    initialized: Arc<Mutex<HashSet<Arc<str>>>>,
    commits: Arc<crate::store::durable::CommitSignal>,
    health: Arc<crate::store::durable::PersistHealth>,
    debouncer: Arc<Debouncer>,
    subscriptions: Arc<RwLock<Vec<SubscriptionEntry>>>,
    next_sub_id: Arc<AtomicU64>,
    write_lock: Arc<Mutex<()>>,
}

impl RedbStoreInner {
    pub fn close(&self) -> StorageResult<()> {
        info!("Closing RedbStore...");
        self.save_now().attach("flushing the buffer before close")?;
        Ok(())
    }

    pub fn save_now(&self) -> StorageResult<()> {
        self.flush_prefix(&StorePath::root())
    }

    /// Commits what is buffered under `prefix`, trading the handle in if redb
    /// has stopped touching the disk.
    ///
    /// This is the path a durable write waits on, so it recovers rather than
    /// reporting a failure the caller can do nothing about: a handle that has
    /// seen an I/O error answers everything with `PreviousIo` for good, and
    /// only a fresh one can land the write. One retry, because the second
    /// failure is the disk rather than the handle.
    pub fn flush_prefix(&self, prefix: &StorePath) -> StorageResult<()> {
        let _write_guard = self.write_lock.lock();

        match self.flush_locked(prefix) {
            Err(report) if is_previous_io(&report) => {
                reopen(&self.db, &self.path)?;
                self.flush_locked(prefix)
            }
            other => other,
        }
    }

    fn flush_locked(&self, prefix: &StorePath) -> StorageResult<()> {
        let changes = {
            let lock = self.pending.lock();
            utils::pending_prefix(&lock, prefix.as_str())
        };
        let txn = self
            .db()?
            .begin_write()
            .doing(StorageError::Flush, &self.path)
            .attach_with(|| format!("prefix: {prefix}"))
            .attach_with(|| format!("buffered entries: {}", changes.len()))?;

        apply_pending(&txn, &changes, &self.path)?;

        txn.commit()
            .doing(StorageError::Flush, &self.path)
            .attach_with(|| format!("prefix: {prefix}"))
            .attach_with(|| format!("buffered entries: {}", changes.len()))?;

        utils::clear_committed(&mut self.pending.lock(), &changes);
        self.commits.finished(true);
        Ok(())
    }

    /// The database to work against, or a failure if it is being replaced.
    ///
    /// A read or a scan calling this during the gap is told so rather than
    /// waiting: a reopen is triggered by a disk that already failed, and a UI
    /// thread blocking on a file operation is what this library avoids
    /// everywhere else.
    ///
    /// A durable write does wait, and needs no code here to do it: it goes
    /// through `flush_prefix`, which takes `write_lock` first, and the reopen
    /// holds that same lock for the whole swap. So a commit either runs before
    /// the reopen or after it, and never during - which is the blocking a
    /// durable write already promises. Keep the two on one lock and that stays
    /// true for free.
    fn db(&self) -> StorageResult<Arc<Database>> {
        self.db.load_full().ok_or_else(|| {
            error_stack::Report::new(StorageError::Read)
                .attach("the database is being reopened after an I/O failure")
                .attach(format!("file: {}", self.path.display()))
        })
    }

    /// Whether a write may proceed.
    ///
    /// A background flush that has been failing past its budget is an error
    /// the caller can act on, not a reason to take the process down - the
    /// value is refused, what is already buffered keeps being retried, and a
    /// flush that lands clears this. A debouncer thread that is actually dead
    /// is a different thing and still panics: that is a bug here, not a disk.
    fn check_debouncer(&self) -> StorageResult<()> {
        if let Some(reason) = self.health.failure() {
            return Err(error_stack::Report::new(StorageError::CommitFailed)
                .attach(format!("the background flush is not landing: {reason}"))
                .attach("what is already buffered is still being retried, and reads are unaffected"));
        }
        if self.debouncer.is_poisoned() {
            panic!("debouncer thread is dead — store integrity cannot be guaranteed");
        }
        Ok(())
    }
}

impl Drop for RedbStoreInner {
    fn drop(&mut self) {
        utils::report_closing_flush(self.close(), &self.path);
    }
}

#[derive(Clone)]
pub struct RedbStore {
    inner: Arc<RedbStoreInner>,
}

impl std::fmt::Debug for RedbStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RedbStore").finish_non_exhaustive()
    }
}

impl PartialEq for RedbStore {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.inner, &other.inner)
    }
}
impl Eq for RedbStore {}

impl RedbStore {
    pub fn open(
        config: StoreConfig,
        migration_set: MigrationSet,
    ) -> StorageResult<(Self, MigrationReport)> {
        let path: Arc<Path> = Arc::from(config.path.as_path());

        let opened = Arc::new(create_database(&config.path).doing(StorageError::Open, &path)?);

        let write_txn = opened.begin_write().doing(StorageError::Open, &path)?;
        {
            for table in [
                TABLE_DATA,
                TABLE_META,
                TABLE_DIFF_LOG,
                TABLE_MIGRATION_LOG,
                TABLE_SCHEMA_SNAPSHOT,
            ] {
                let _ = write_txn
                    .open_table(table)
                    .doing(StorageError::Open, &path)
                    .attach_with(|| format!("table: {}", table.name()))?;
            }
        }
        write_txn.commit().doing(StorageError::Open, &path)?;

        let pending = Arc::new(Mutex::new(utils::Pending::new()));
        let initialized = Arc::new(Mutex::new(HashSet::<Arc<str>>::new()));
        let commits = Arc::new(crate::store::durable::CommitSignal::default());
        let subscriptions = Arc::new(RwLock::new(Vec::new()));

        let db: OpenDatabase = Arc::new(arc_swap::ArcSwapOption::from(Some(opened)));

        // The swap, not the database: a clone of the `Database` here would
        // hold redb's file lock for the life of this thread, and a reopen
        // could never take it back.
        let db_save = db.clone();
        let pending_save = pending.clone();
        let path_save = path.clone();

        let write_lock = Arc::new(Mutex::new(()));
        let write_lock_save = write_lock.clone();

        let health = Arc::new(crate::store::durable::PersistHealth::default());

        let debouncer = Debouncer::new_with_retry(
            config.save_debounce,
            crate::store::util::debouncer::FlushPolicy {
                retry: config.retry_policy.clone(),
                commits: commits.clone(),
                health: health.clone(),
                on_giveup: config.on_persist_failure.clone(),
            },
            move || -> Result<(), String> {
                let _write_guard = write_lock_save.lock();

                let changes = {
                    let lock = pending_save.lock();
                    if lock.is_empty() {
                        return Ok(());
                    }
                    lock.clone()
                };

                #[cfg(test)]
                if SIMULATE_WRITE_FAILURE.load(Ordering::Relaxed) {
                    return Err("simulated write failure".to_string());
                }

                let landed: StorageResult<()> = (|| {
                    let db = db_save.load_full().ok_or_else(|| {
                        error_stack::Report::new(StorageError::Flush)
                            .attach("the database is being reopened")
                    })?;
                    let txn = db
                        .begin_write()
                        .doing(StorageError::Flush, &path_save)
                        .attach_with(|| format!("buffered entries: {}", changes.len()))?;
                    apply_pending(&txn, &changes, &path_save)?;
                    txn.commit()
                        .doing(StorageError::Flush, &path_save)
                        .attach_with(|| format!("buffered entries: {}", changes.len()))
                })();

                match landed {
                    Ok(()) => {
                        utils::clear_committed(&mut pending_save.lock(), &changes);
                        Ok(())
                    }
                    Err(report) => {
                        // Retrying against a handle that answers `PreviousIo`
                        // is spinning: it has stopped going near the disk at
                        // all. Trading it for a fresh one is the only thing
                        // that turns a recovered disk back into a working
                        // store, and the next attempt of this same retry loop
                        // is the one that lands.
                        if is_previous_io(&report)
                            && let Err(failed) = reopen(&db_save, &path_save)
                        {
                            return Err(format!("{report:#}; and {failed:#}"));
                        }
                        Err(format!("{report:#}"))
                    }
                }
            },
        );

        let inner = Arc::new(RedbStoreInner {
            db,
            path,
            pending,
            initialized,
            commits,
            health,
            debouncer: Arc::new(debouncer),
            subscriptions,
            next_sub_id: Arc::new(AtomicU64::new(1)),
            write_lock,
        });

        let store = Self { inner };
        let report = store
            .run_migrations(migration_set)
            .attach_with(|| format!("store: {}", store.inner.path.display()))
            .attach("opening the store")?;

        Ok((store, report))
    }

    pub fn close(&self) -> StorageResult<()> {
        self.inner.close()
    }

    /// The value a subscriber should see as the old one.
    ///
    /// The buffer wins where it has the key, since it holds the newer value;
    /// otherwise the committed one. Reading the buffer alone reported no old
    /// value once a flush had emptied it, though the key was on disk.
    fn committed_or_buffered(&self, path: &StorePath) -> StorageResult<Option<Vec<u8>>> {
        let path = path.as_str();
        if let Some(op) = self.inner.pending.lock().get(path).filter(|o| o.is_data()) {
            return Ok(op.value().map(Vec::from));
        }

        let read_txn = self
            .inner
            .db()?
            .begin_read()
            .doing(StorageError::Read, &self.inner.path)
            .attach_with(|| format!("key: {path}"))?;
        let table = read_txn
            .open_table(TABLE_DATA)
            .doing(StorageError::Read, &self.inner.path)
            .attach_with(|| format!("table: {}", TABLE_DATA.name()))?;

        Ok(table
            .get(path)
            .doing(StorageError::Read, &self.inner.path)
            .attach_with(|| format!("key: {path}"))?
            .map(|v| Vec::from(&v.value()[..])))
    }
}

impl SchemaAwareStore for RedbStore {
    fn run_migrations(&self, mset: MigrationSet) -> StorageResult<MigrationReport> {
        struct RedbProvider<'a> {
            db: &'a Database,
            path: &'a Path,
        }

        impl<'a> StorageProvider for RedbProvider<'a> {
            fn atomic<F, T>(&self, f: F) -> StorageResult<T>
            where
                F: FnOnce(&mut dyn MigrationBackendAdapter) -> StorageResult<T>,
            {
                let write_txn = self
                    .db
                    .begin_write()
                    .doing(StorageError::Migrate, self.path)?;

                let res = {
                    let mut storage = RedbMigrationBackend::new(&write_txn, self.path);
                    f(&mut storage)?
                };

                write_txn.commit().doing(StorageError::Migrate, self.path)?;
                Ok(res)
            }
        }

        let db = self.inner.db()?;
        let provider = RedbProvider {
            db: &db,
            path: &self.inner.path,
        };
        let engine = MigrationEngine::new(&provider);
        engine.run(mset)
    }
}

impl StoreBackend for RedbStore {
    fn get_raw(&self, path: &StorePath) -> StorageResult<Option<Vec<u8>>> {
        let path = path.as_str();
        {
            let lock = self.inner.pending.lock();
            if let Some(op) = lock.get(path).filter(|o| o.is_data()) {
                return Ok(op.value().map(|b| b.to_vec()));
            }
        }

        let read_txn = self
            .inner
            .db()?
            .begin_read()
            .doing(StorageError::Read, &self.inner.path)
            .attach_with(|| format!("key: {path}"))?;
        let table = read_txn
            .open_table(TABLE_DATA)
            .doing(StorageError::Read, &self.inner.path)
            .attach_with(|| format!("table: {}", TABLE_DATA.name()))?;
        match table
            .get(path)
            .doing(StorageError::Read, &self.inner.path)
            .attach_with(|| format!("key: {path}"))?
        {
            Some(access_guard) => Ok(Some(access_guard.value().to_vec())),
            None => Ok(None),
        }
    }

    fn get_erased(
        &self,
        path: &StorePath,
        f: &mut dyn FnMut(&mut dyn erased_serde::Deserializer) -> StorageResult<()>,
    ) -> StorageResult<bool> {
        let path = path.as_str();
        {
            let lock = self.inner.pending.lock();
            if let Some(op) = lock.get(path).filter(|o| o.is_data()) {
                return match op.value() {
                    Some(bytes) => {
                        self.decode_erased(bytes, f)
                            .change_context(StorageError::Read)
                            .attach_with(|| format!("store: {}", self.inner.path.display()))
                            .attach_with(|| format!("key: {path} (unflushed)"))?;
                        Ok(true)
                    }
                    None => Ok(false),
                };
            }
        }

        let read_txn = self
            .inner
            .db()?
            .begin_read()
            .doing(StorageError::Read, &self.inner.path)
            .attach_with(|| format!("key: {path}"))?;
        let table = read_txn
            .open_table(TABLE_DATA)
            .doing(StorageError::Read, &self.inner.path)
            .attach_with(|| format!("table: {}", TABLE_DATA.name()))?;
        match table
            .get(path)
            .doing(StorageError::Read, &self.inner.path)
            .attach_with(|| format!("key: {path}"))?
        {
            Some(access_guard) => {
                self.decode_erased(access_guard.value(), f)
                    .change_context(StorageError::Read)
                    .attach_with(|| format!("store: {}", self.inner.path.display()))
                    .attach_with(|| format!("key: {path}"))?;
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
        let mut de = rmp_serde::Deserializer::from_read_ref(bytes);
        let mut erased = <dyn erased_serde::Deserializer>::erase(&mut de);
        f(&mut erased).attach_with(|| format!("decoding {} bytes of messagepack", bytes.len()))
    }

    fn set_erased(
        &self,
        path: &StorePath,
        value: &dyn erased_serde::Serialize,
        source: Option<uuid::Uuid>,
    ) -> StorageResult<()> {
        self.set_owned_erased(path.clone(), value, source)
    }

    fn set_owned_erased(
        &self,
        path: StorePath,
        value: &dyn erased_serde::Serialize,
        source: Option<uuid::Uuid>,
    ) -> StorageResult<()> {
        self.inner.check_debouncer()?;
        let bytes = SERIALIZATION_BUFFER
            .with(|buf| {
                let mut b = buf.borrow_mut();
                b.clear();
                let mut ser = Serializer::new(&mut *b).with_bytes(BytesMode::ForceAll);
                erased_serde::serialize(value, &mut ser).map_err(CodecError::from)?;

                Ok::<Vec<u8>, CodecError>(Vec::from(&b[..]))
            })
            .doing(StorageError::Codec, &self.inner.path)
            .attach_with(|| format!("key: {path}"))?;

        let old_bytes = self
            .committed_or_buffered(&path)
            .change_context(StorageError::Write)
            .attach_with(|| format!("key: {path}"))
            .attach("reading the value a subscriber should see as the old one")?;

        {
            let mut lock = self.inner.pending.lock();
            lock.insert(
                Arc::from(path.as_str()),
                utils::PendingOp::Set(bytes.clone()),
            );
        }

        utils::emit_events(
            &self.inner.subscriptions,
            StoreEvent {
                path: Arc::from(path.as_str()),
                op: StoreOp::Set,
                old: old_bytes,
                new: Some(bytes),
                source,
            },
        );

        self.inner.debouncer.schedule();
        Ok(())
    }

    fn save_now(&self) -> StorageResult<()> {
        self.inner.save_now()
    }

    fn scan_prefix(&self, prefix: &StorePath) -> StorageResult<Vec<(StorePath, Vec<u8>)>> {
        let bound = utils::subtree_bound(prefix);
        let prefix = prefix.as_str();
        let mut results: BTreeMap<StorePath, Vec<u8>> = BTreeMap::new();

        let read_txn = self
            .inner
            .db()?
            .begin_read()
            .doing(StorageError::Scan, &self.inner.path)
            .attach_with(|| format!("prefix: {prefix}"))?;
        let table = read_txn
            .open_table(TABLE_DATA)
            .doing(StorageError::Scan, &self.inner.path)
            .attach_with(|| format!("table: {}", TABLE_DATA.name()))?;

        let range = prefix..;
        let entries = table
            .range(range)
            .doing(StorageError::Scan, &self.inner.path)
            .attach_with(|| format!("prefix: {prefix}"))?;
        for result in entries {
            let (k, v) = result
                .doing(StorageError::Scan, &self.inner.path)
                .attach_with(|| format!("prefix: {prefix}"))
                .attach_with(|| format!("entries read so far: {}", results.len()))?;
            let key_str = k.value();
            if utils::is_under(key_str, prefix, &bound) {
                results.insert(utils::stored_path(key_str)?, Vec::from(&v.value()[..]));
            } else if !key_str.starts_with(prefix) {
                break;
            }
        }

        let mut pending_map = HashMap::new();
        {
            let lock = self.inner.pending.lock();
            for (k, op) in lock.iter().filter(|(_, o)| o.is_data()) {
                if utils::is_under(k, prefix, &bound) {
                    pending_map.insert(utils::stored_path(k)?, op.value().map(Vec::from));
                }
            }
        }

        for (k, opt_v) in pending_map {
            match opt_v {
                Some(v) => results.insert(k, v),
                None => results.remove(&k),
            };
        }

        Ok(results.into_iter().collect())
    }

    fn scan_keys(&self, prefix: &StorePath) -> StorageResult<Vec<StorePath>> {
        let bound = utils::subtree_bound(prefix);
        let prefix = prefix.as_str();
        let mut keys: BTreeSet<StorePath> = BTreeSet::new();

        let read_txn = self
            .inner
            .db()?
            .begin_read()
            .doing(StorageError::Scan, &self.inner.path)
            .attach_with(|| format!("prefix: {prefix}"))?;

        let table = read_txn
            .open_table(TABLE_DATA)
            .doing(StorageError::Scan, &self.inner.path)
            .attach_with(|| format!("table: {}", TABLE_DATA.name()))?;

        let entries = table
            .range(prefix..)
            .doing(StorageError::Scan, &self.inner.path)
            .attach_with(|| format!("prefix: {prefix}"))?;

        for result in entries {
            let (k, _) = result
                .doing(StorageError::Scan, &self.inner.path)
                .attach_with(|| format!("prefix: {prefix}"))
                .attach_with(|| format!("keys read so far: {}", keys.len()))?;
            let key = k.value();
            if !utils::is_under(key, prefix, &bound) {
                if !key.starts_with(prefix) {
                    break;
                }
                continue;
            }
            keys.insert(utils::stored_path(key)?);
        }

        {
            let lock = self.inner.pending.lock();
            for (k, op) in lock.iter().filter(|(_, o)| o.is_data()) {
                if !utils::is_under(k, prefix, &bound) {
                    continue;
                }
                let key = utils::stored_path(k)?;
                match op.value() {
                    Some(_) => keys.insert(key),
                    None => keys.remove(&key),
                };
            }
        }

        Ok(keys.into_iter().collect())
    }

    fn delete_with_source(&self, path: &StorePath, source: Option<Uuid>) -> StorageResult<()> {
        self.inner.check_debouncer()?;
        let path_arc: Arc<str> = Arc::from(path.as_str());

        let old_bytes = self
            .committed_or_buffered(path)
            .change_context(StorageError::Delete)
            .attach_with(|| format!("key: {path}"))
            .attach("reading the value a subscriber should see as the old one")?;

        let Some(old_bytes) = old_bytes else {
            return Ok(());
        };

        {
            let mut lock = self.inner.pending.lock();
            lock.insert(path_arc.clone(), utils::PendingOp::Delete);
        }

        utils::emit_events(
            &self.inner.subscriptions,
            StoreEvent {
                path: path_arc,
                op: StoreOp::Delete,
                old: Some(old_bytes),
                new: None,
                source,
            },
        );

        self.inner.debouncer.schedule();
        Ok(())
    }

    fn delete_prefix_with_source(
        &self,
        prefix: &StorePath,
        source: Option<Uuid>,
    ) -> StorageResult<()> {
        self.inner.check_debouncer()?;

        let keys = self
            .scan_prefix(prefix)
            .change_context(StorageError::Delete)
            .attach_with(|| format!("prefix: {prefix}"))
            .attach("listing the subtree to be removed")?;
        let prefix = prefix.as_str();

        {
            let mut lock = self.inner.pending.lock();
            for (path, _) in keys {
                lock.insert(Arc::from(path.as_str()), utils::PendingOp::Delete);
            }
        }

        utils::emit_events(
            &self.inner.subscriptions,
            StoreEvent {
                path: Arc::from(prefix),
                op: StoreOp::DeletePrefix,
                old: None,
                new: None,
                source,
            },
        );

        self.inner.debouncer.schedule();
        Ok(())
    }

    fn delete(&self, path: &StorePath) -> StorageResult<()> {
        self.delete_with_source(path, None)
    }

    fn subscribe(&self, kind: SubscriptionKind, callback: StoreCallback) -> SubscriptionId {
        let id = self.inner.next_sub_id.fetch_add(1, Ordering::Relaxed);
        self.inner
            .subscriptions
            .write()
            .push(SubscriptionEntry { id, kind, callback });
        id
    }

    fn unsubscribe(&self, id: SubscriptionId) {
        self.inner.subscriptions.write().retain(|s| s.id != id);
    }

    fn flush_prefix(&self, prefix: &StorePath) -> StorageResult<()> {
        self.inner.flush_prefix(prefix)
    }

    fn flush_async(&self) -> crate::store::durable::Commit {
        let commit = crate::store::durable::Commit::awaiting(self.inner.commits.clone());
        self.inner.debouncer.flush_now();
        commit
    }

    fn is_initialized(&self, namespace: &str) -> StorageResult<bool> {
        if self.inner.initialized.lock().contains(namespace) {
            return Ok(true);
        }

        let key = utils::init_key(namespace);
        let read_txn = self
            .inner
            .db()?
            .begin_read()
            .doing(StorageError::Meta, &self.inner.path)
            .attach_with(|| format!("namespace: {namespace}"))?;
        let table = read_txn
            .open_table(TABLE_META)
            .doing(StorageError::Meta, &self.inner.path)
            .attach_with(|| format!("table: {}", TABLE_META.name()))?;
        let found = table
            .get(key.as_str())
            .doing(StorageError::Meta, &self.inner.path)
            .attach_with(|| format!("key: {key}"))?
            .is_some();

        if found {
            self.inner.initialized.lock().insert(Arc::from(namespace));
        }
        Ok(found)
    }

    fn set_initialized(&self, namespace: &str, state: InitState) -> StorageResult<()> {
        if self.inner.initialized.lock().contains(namespace) == state.is_seeded() {
            return Ok(());
        }

        self.inner.check_debouncer()?;
        let key: Arc<str> = Arc::from(namespace);
        self.inner
            .pending
            .lock()
            .insert(Arc::clone(&key), utils::PendingOp::Init(state.is_seeded()));

        let mut initialized = self.inner.initialized.lock();
        if state.is_seeded() {
            initialized.insert(key);
        } else {
            initialized.remove(&key);
        }
        drop(initialized);

        self.inner.debouncer.schedule();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::migration::fields::FieldDescriptor;
    use crate::migration::{MigrationError, MigrationPlan};
    use crate::store::IntoStorageReport;
    use crate::store::StoreExt;
    use crate::store::config::AfterGivingUp;
    use amethystate_core::test_utils::unique_path;
    use serial_test::serial;
    use std::thread;
    use std::time::Duration;

    const EMPTY_FIELDS: &[FieldDescriptor] = &[];

    #[test]
    #[serial]
    fn test_debouncer_persistence() {
        let path = unique_path("debounce");

        let mut config = StoreConfig::new(path);
        config.save_debounce = Duration::from_millis(50);

        let (store, _) = RedbStore::open(config, MigrationSet::default()).unwrap();

        store.set(["config", "port"], &8080u16).unwrap();

        {
            let read_txn = store.inner.db().unwrap().begin_read().unwrap();
            let table = read_txn.open_table(TABLE_DATA).unwrap();
            assert!(table.get("config.port").unwrap().is_none());
        }

        thread::sleep(Duration::from_millis(500));

        {
            let read_txn = store.inner.db().unwrap().begin_read().unwrap();
            let table = read_txn.open_table(TABLE_DATA).unwrap();
            assert!(table.get("config.port").unwrap().is_some());
        }
    }

    #[test]
    fn test_delete_flow() {
        let path = unique_path("delete");
        let (store, _) = RedbStore::open(StoreConfig::new(path), MigrationSet::default()).unwrap();

        store.set(["temp", "key"], &1).unwrap();

        store.save_now().unwrap();
        store
            .delete(&StorePath::from_segments(["temp", "key"]))
            .unwrap();
        assert_eq!(store.get::<i32>(["temp", "key"]).unwrap(), None);

        store.save_now().unwrap();

        let read_txn = store.inner.db().unwrap().begin_read().unwrap();
        let table = read_txn.open_table(TABLE_DATA).unwrap();
        assert!(table.get("temp.key").unwrap().is_none());
    }

    #[test]
    fn test_deterministic_closure_and_reopen() {
        let path = unique_path("closure");
        {
            let (store, _) =
                RedbStore::open(StoreConfig::new(&path), MigrationSet::default()).unwrap();
            store.set(["test", "key"], &"hello".to_string()).unwrap();
            store.close().expect("Explicit close failed");
        }

        let (store_reopened, _) = RedbStore::open(StoreConfig::new(&path), MigrationSet::default())
            .expect("Database should be available immediately after close");

        let val: Option<String> = store_reopened.get(["test", "key"]).unwrap();
        assert_eq!(val, Some("hello".to_string()));
    }

    #[test]
    fn test_drop_behavior_is_deterministic() {
        let path = unique_path("drop_logic");
        {
            let (store, _) =
                RedbStore::open(StoreConfig::new(&path), MigrationSet::default()).unwrap();
            store.set(["drop", "test"], &42u32).unwrap();
        }

        let (store_reopened, _) = RedbStore::open(StoreConfig::new(&path), MigrationSet::default())
            .expect("Drop must release file lock deterministically");

        assert_eq!(
            store_reopened.get::<u32>(["drop", "test"]).unwrap(),
            Some(42)
        );
    }

    #[test]
    fn test_close_saves_pending_data() {
        let path = unique_path("save_on_close");
        let mut config = StoreConfig::new(&path);
        config.save_debounce = Duration::from_secs(3600);

        {
            let (store, _) = RedbStore::open(config, MigrationSet::default()).unwrap();
            store.set(["urgent", "data"], &true).unwrap();
            store.close().unwrap();
        }

        let (store, _) = RedbStore::open(StoreConfig::new(&path), MigrationSet::default()).unwrap();
        assert_eq!(store.get::<bool>(["urgent", "data"]).unwrap(), Some(true));
    }

    #[test]
    fn test_granular_flush_prefix_drains_buffer() {
        let path = unique_path("granular_flush");
        let mut config = StoreConfig::new(&path);

        config.save_debounce = Duration::from_secs(3600);

        let (store, _) = RedbStore::open(config, MigrationSet::default()).unwrap();

        store
            .set(["net", "host"], &"127.0.0.1".to_string())
            .unwrap();
        store.set(["net", "port"], &8080u16).unwrap();
        store.set(["ui", "theme"], &"dark".to_string()).unwrap();

        {
            let pending = store.inner.pending.lock();
            assert_eq!(pending.len(), 3);
        }
        {
            let read_txn = store.inner.db().unwrap().begin_read().unwrap();
            let table = read_txn.open_table(TABLE_DATA).unwrap();
            assert!(table.get("net.host").unwrap().is_none());
            assert!(table.get("ui.theme").unwrap().is_none());
        }

        store
            .flush_prefix(&StorePath::from_segments(["net"]))
            .unwrap();

        {
            let read_txn = store.inner.db().unwrap().begin_read().unwrap();
            let table = read_txn.open_table(TABLE_DATA).unwrap();
            assert_eq!(
                store
                    .decode::<String>(table.get("net.host").unwrap().unwrap().value())
                    .unwrap(),
                "127.0.0.1"
            );
            assert_eq!(
                store
                    .decode::<u16>(table.get("net.port").unwrap().unwrap().value())
                    .unwrap(),
                8080
            );
            assert!(
                table.get("ui.theme").unwrap().is_none(),
                "UI should remain in the RAM buffer"
            );
        }

        {
            let pending = store.inner.pending.lock();
            assert_eq!(
                pending.len(),
                1,
                "Only ui.theme should remain in the buffer"
            );
            assert!(pending.contains_key("ui.theme"));
            assert!(!pending.contains_key("net.host"));
            assert!(!pending.contains_key("net.port"));
        }

        store.flush_prefix(&StorePath::root()).unwrap();
        {
            let pending = store.inner.pending.lock();
            assert!(
                pending.is_empty(),
                "Pending buffer should be completely empty"
            );
        }
        {
            let read_txn = store.inner.db().unwrap().begin_read().unwrap();
            let table = read_txn.open_table(TABLE_DATA).unwrap();
            assert!(
                table.get("ui.theme").unwrap().is_some(),
                "UI should now be persisted on disk"
            );
        }
    }

    #[test]
    fn test_component_atomic_rollback() {
        let path = unique_path("rollback");
        let mut cfg = StoreConfig::new(&path);
        cfg.save_debounce = Duration::from_millis(50);
        {
            let (store, _) = RedbStore::open(cfg, MigrationSet::default()).unwrap();
            store.set(["net", "ip"], &"1.1.1.1".to_string()).unwrap();
            store.save_now().unwrap();
        }

        let mset = MigrationSet::default()
            .add(
                "net",
                MigrationPlan::new().step(1, "ok", |ctx| ctx.set("ip", &"8.8.8.8".to_string())),
                0,
                EMPTY_FIELDS,
                &[],
            )
            .add(
                "ui",
                MigrationPlan::new().step(1, "fail", |_| {
                    Err(MigrationError::Custom("crash".into()).into_report())
                }),
                0,
                EMPTY_FIELDS,
                &["net"],
            );

        let (store, report) = RedbStore::open(StoreConfig::new(&path), mset).unwrap();
        assert!(report.has_failures());

        let val: String = store.get(["net", "ip"]).unwrap().unwrap();
        assert_eq!(val, "1.1.1.1");
    }
    #[test]
    #[serial]
    fn test_debouncer_retains_buffer_on_simulated_transaction_failure() {
        let path = unique_path("debouncer_simulated_fail");

        let mut config = StoreConfig::new(&path);
        config.save_debounce = Duration::from_millis(50);

        SIMULATE_WRITE_FAILURE.store(true, Ordering::Relaxed);

        let (store, _) = RedbStore::open(config, MigrationSet::default()).unwrap();

        let test_key = StorePath::from_segments(["system", "critical_update"]);
        let test_value = "payload_data".to_string();
        store.set(&test_key, &test_value).unwrap();

        {
            let pending = store.inner.pending.lock();
            assert!(pending.contains_key(test_key.as_str()));
        }

        thread::sleep(Duration::from_millis(150));

        SIMULATE_WRITE_FAILURE.store(false, Ordering::Relaxed);

        {
            let pending = store.inner.pending.lock();
            assert!(
                pending.contains_key(test_key.as_str()),
                "The pending changes buffer should not be cleared when a transaction fails!"
            );
        }

        let retrieved: Option<String> = store.get(&test_key).unwrap();
        assert_eq!(retrieved, Some(test_value));
    }

    fn failing_store(tag: &str, decision: AfterGivingUp) -> (RedbStore, Arc<Mutex<Vec<String>>>) {
        let mut config = StoreConfig::new(unique_path(tag));
        config.save_debounce = Duration::from_millis(10);
        config.retry_policy = crate::store::config::RetryPolicy {
            interval: Duration::from_millis(10),
            budget: Duration::from_millis(50),
        };

        let heard: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let heard_write = heard.clone();
        config.on_persist_failure = Some(Arc::new(move |reason: &str| {
            heard_write.lock().push(reason.to_string());
            decision
        }));

        let (store, _) = RedbStore::open(config, MigrationSet::default()).unwrap();
        (store, heard)
    }

    /// A flush that keeps failing tells writers so, once its streak has
    /// outlived the budget - an error they can act on, not a dead process.
    /// A full disk is somebody about to delete something, and taking the
    /// application down with it is the store's least useful reaction.
    #[test]
    #[serial]
    fn a_flush_that_keeps_failing_fails_the_next_write_rather_than_the_process() {
        SIMULATE_WRITE_FAILURE.store(true, Ordering::Relaxed);
        let (store, heard) = failing_store("debouncer_fails_writes", AfterGivingUp::Fail);

        store
            .set(StorePath::from_segments(["doomed"]), &1u32)
            .unwrap();
        thread::sleep(Duration::from_millis(200));

        assert!(
            !heard.lock().is_empty(),
            "on_persist_failure never ran once the streak outlived the budget"
        );
        assert!(
            !store.inner.debouncer.is_poisoned(),
            "a disk that will not take a write is not a reason to poison the writer"
        );

        let refused = store.set(StorePath::from_segments(["another"]), &2u32);
        assert!(
            refused.is_err(),
            "a write while the flush is not landing should say so, not queue quietly"
        );

        // The reads the store already had are untouched by any of it.
        assert_eq!(
            store.get::<u32>(StorePath::from_segments(["doomed"])).unwrap(),
            Some(1)
        );

        SIMULATE_WRITE_FAILURE.store(false, Ordering::Relaxed);
    }

    /// And it heals: the disk comes back, the next flush lands, and writes
    /// work again with nothing restarted.
    #[test]
    #[serial]
    fn a_disk_that_comes_back_heals_the_store() {
        SIMULATE_WRITE_FAILURE.store(true, Ordering::Relaxed);
        let (store, _) = failing_store("debouncer_heals", AfterGivingUp::Fail);

        store
            .set(StorePath::from_segments(["waiting"]), &1u32)
            .unwrap();
        thread::sleep(Duration::from_millis(200));
        assert!(
            store.set(StorePath::from_segments(["nope"]), &2u32).is_err(),
            "the store should be refusing writes before the disk comes back"
        );

        SIMULATE_WRITE_FAILURE.store(false, Ordering::Relaxed);

        // The retry loop is still running, so the next attempt lands on its
        // own - nothing here asks it to.
        thread::sleep(Duration::from_millis(200));

        store
            .set(StorePath::from_segments(["fine"]), &3u32)
            .expect("writes should work again once a flush has landed");
    }

    /// The application that would rather stop than run on with state it
    /// cannot persist can still say so.
    #[test]
    #[serial]
    fn poison_is_available_for_an_application_that_asks_for_it() {
        SIMULATE_WRITE_FAILURE.store(true, Ordering::Relaxed);
        let (store, _) = failing_store("debouncer_poisons", AfterGivingUp::Poison);

        store
            .set(StorePath::from_segments(["doomed"]), &1u32)
            .unwrap();
        thread::sleep(Duration::from_millis(200));

        assert!(
            store.inner.debouncer.is_poisoned(),
            "AfterGivingUp::Poison should have taken the writer down"
        );

        let poisoned_write = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            store.set(StorePath::from_segments(["another"]), &2u32)
        }));
        assert!(
            poisoned_write.is_err(),
            "a write after a poison should panic in the caller's own stack"
        );

        SIMULATE_WRITE_FAILURE.store(false, Ordering::Relaxed);
    }

    /// The flush a short-lived process depends on is the one nobody is left to
    /// ask about: `Drop` has no caller to hand an error to. It leaves a line
    /// instead, and this is what fails if that line ever goes away.
    #[test]
    #[serial]
    #[tracing_test::traced_test]
    fn a_closing_flush_that_fails_leaves_a_trace() {
        let path = unique_path("redb_closing_flush");
        let _disk = recovery::arm_failing_disk(&path);

        let mut config = StoreConfig::new(&path);
        config.save_debounce = Duration::from_secs(60);
        let (store, _) = RedbStore::open(config, MigrationSet::default()).unwrap();

        store
            .set(StorePath::from_segments(["lost"]), &1u32)
            .unwrap();

        recovery::WRITES_LEFT.store(0, Ordering::SeqCst);
        drop(store);

        assert!(
            logs_contain("the store's closing flush failed"),
            "a store that could not write on the way out said nothing"
        );
    }
}
