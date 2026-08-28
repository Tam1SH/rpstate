use crate::MigrationReport;
use crate::codec::CodecError;
use crate::migration::engine::{MigrationEngine, StorageProvider};
use crate::migration::set::MigrationSet;
use crate::store::backend::sqlite::migration::SqliteMigrationBackend;
use crate::store::backend::utils;
use crate::store::backend::utils::Attempted;
use crate::store::config::StoreConfig;
use crate::store::durable::{Commit, CommitSignal, PersistHealth};
use crate::store::error::StorageError;
use crate::store::traits::MigrationBackendAdapter;
use crate::store::util::debouncer::Debouncer;
use crate::store::{
    InitState, SchemaAwareStore, StorageResult, StoreBackend, StoreCallback, StoreEvent, StoreOp,
    SubscriptionEntry, SubscriptionId, SubscriptionKind,
};
use amethystate_core::path::StorePath;
use error::SqliteStoreError;
use error_stack::ResultExt;
use parking_lot::{Mutex, RwLock};
use rusqlite::{Connection, OptionalExtension};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use tracing::info;

pub mod error;
mod inspector;
mod migration;

fn at(path: &Path) -> String {
    format!("store: {}", path.display())
}

/// Writes every buffered change through `txn`'s prepared statements.
/// Committing `txn` is the caller's own last step - a synchronous flush that
/// reports its error immediately and a retried background one both walk the
/// same changes the same way, and only differ in what happens after this
/// returns.
fn apply_pending(
    txn: &rusqlite::Transaction,
    changes: &utils::Pending,
    path: &Path,
) -> StorageResult<()> {
    let mut ins = txn
        .prepare_cached("REPLACE INTO data (key, value) VALUES (?, ?)")
        .map_err(SqliteStoreError::from)
        .doing(StorageError::Flush, path)?;
    let mut del = txn
        .prepare_cached("DELETE FROM data WHERE key = ?")
        .map_err(SqliteStoreError::from)
        .doing(StorageError::Flush, path)?;
    let mut mark = txn
        .prepare_cached("REPLACE INTO metadata (key, value) VALUES (?, ?)")
        .map_err(SqliteStoreError::from)
        .doing(StorageError::Flush, path)?;
    let mut unmark = txn
        .prepare_cached("DELETE FROM metadata WHERE key = ?")
        .map_err(SqliteStoreError::from)
        .doing(StorageError::Flush, path)?;

    for (key, op) in changes {
        match op {
            utils::PendingOp::Set(b) => {
                ins.execute(rusqlite::params![&**key, &b[..]])
                    .map_err(SqliteStoreError::from)
                    .doing(StorageError::Flush, path)
                    .attach_with(|| format!("writing key: {key}"))
                    .attach_with(|| format!("value bytes: {}", b.len()))?;
            }
            utils::PendingOp::Delete => {
                del.execute([&**key])
                    .map_err(SqliteStoreError::from)
                    .doing(StorageError::Flush, path)
                    .attach_with(|| format!("deleting key: {key}"))?;
            }
            utils::PendingOp::Init(seeded) => {
                let init_key = utils::init_key(key);
                if *seeded {
                    mark.execute(rusqlite::params![init_key, [] as [u8; 0]])
                } else {
                    unmark.execute(rusqlite::params![init_key])
                }
                .map_err(SqliteStoreError::from)
                .doing(StorageError::Flush, path)
                .attach_with(|| format!("marking namespace: {key}"))?;
            }
        }
    }

    Ok(())
}

struct SqliteStoreInner {
    conn: Arc<Mutex<Connection>>,
    path: PathBuf,
    pending: Arc<Mutex<utils::Pending>>,
    initialized: Arc<Mutex<HashSet<Arc<str>>>>,
    commits: Arc<CommitSignal>,
    health: Arc<PersistHealth>,
    debouncer: Arc<Debouncer>,
    subscriptions: Arc<RwLock<Vec<SubscriptionEntry>>>,
    next_sub_id: Arc<AtomicU64>,
    write_lock: Arc<Mutex<()>>,
}

impl SqliteStoreInner {
    pub fn close(&self) -> StorageResult<()> {
        info!("Closing SqliteStore...");
        self.save_now()?;
        Ok(())
    }

    pub fn flush_prefix(&self, prefix: &StorePath) -> StorageResult<()> {
        let _write_guard = self.write_lock.lock();

        let changes = {
            let lock = self.pending.lock();
            utils::pending_prefix(&lock, prefix.as_str())
        };

        {
            let mut conn = self.conn.lock();
            let txn = conn
                .transaction()
                .map_err(SqliteStoreError::from)
                .doing(StorageError::Flush, &self.path)
                .attach_with(|| format!("prefix: {prefix}"))?;

            apply_pending(&txn, &changes, &self.path)?;

            txn.commit()
                .map_err(SqliteStoreError::from)
                .doing(StorageError::Flush, &self.path)
                .attach_with(|| format!("prefix: {prefix}"))
                .attach_with(|| format!("buffered changes: {}", changes.len()))?;
        }

        utils::clear_committed(&mut self.pending.lock(), &changes);
        self.commits.finished(true);
        Ok(())
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

    /// The value a subscriber should see as the old one.
    ///
    /// The buffer wins where it has the key, since it holds the newer value;
    /// otherwise the committed one. Reading the buffer alone reported no old
    /// value once a flush had emptied it, though the key was in the database.
    fn committed_or_buffered(&self, path: &StorePath) -> StorageResult<Option<Vec<u8>>> {
        if let Some(op) = self
            .pending
            .lock()
            .get(path.as_str())
            .filter(|o| o.is_data())
        {
            return Ok(op.value().map(Vec::from));
        }

        let conn = self.conn.lock();
        let mut stmt = conn
            .prepare_cached("SELECT value FROM data WHERE key = ?")
            .map_err(SqliteStoreError::from)
            .doing(StorageError::Read, &self.path)
            .attach_with(|| format!("key: {path}"))?;

        stmt.query_row([path.as_str()], |row| row.get::<_, Vec<u8>>(0))
            .optional()
            .map_err(SqliteStoreError::from)
            .doing(StorageError::Read, &self.path)
            .attach_with(|| format!("key: {path}"))
    }

    fn run_migrations(&self, mset: MigrationSet) -> StorageResult<MigrationReport> {
        struct SqliteProvider<'a> {
            conn: &'a Mutex<Connection>,
        }

        impl<'a> StorageProvider for SqliteProvider<'a> {
            fn atomic<F, T>(&self, f: F) -> StorageResult<T>
            where
                F: FnOnce(&mut dyn MigrationBackendAdapter) -> StorageResult<T>,
            {
                let mut conn = self.conn.lock();
                let txn = conn
                    .transaction()
                    .map_err(SqliteStoreError::from)
                    .change_context(StorageError::Migrate)
                    .attach("opening the migration transaction")?;

                let res = {
                    let mut storage = SqliteMigrationBackend::new(&txn);
                    f(&mut storage)?
                };

                txn.commit()
                    .map_err(SqliteStoreError::from)
                    .change_context(StorageError::Migrate)
                    .attach("committing the migration transaction")?;
                Ok(res)
            }
        }

        let provider = SqliteProvider { conn: &self.conn };
        let engine = MigrationEngine::new(&provider);
        engine.run(mset).attach_with(|| at(&self.path))
    }

    fn get_raw(&self, path: &StorePath) -> StorageResult<Option<Vec<u8>>> {
        {
            let lock = self.pending.lock();
            if let Some(op) = lock.get(path.as_str()).filter(|o| o.is_data()) {
                return Ok(op.value().map(|b| b.to_vec()));
            }
        }

        let conn = self.conn.lock();
        let mut stmt = conn
            .prepare_cached("SELECT value FROM data WHERE key = ?")
            .map_err(SqliteStoreError::from)
            .doing(StorageError::Read, &self.path)
            .attach_with(|| format!("key: {path}"))?;
        let res: Option<Vec<u8>> = stmt
            .query_row([path.as_str()], |row| row.get(0))
            .optional()
            .map_err(SqliteStoreError::from)
            .doing(StorageError::Read, &self.path)
            .attach_with(|| format!("key: {path}"))?;
        Ok(res)
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
        self.check_debouncer()?;
        let vec = sonic_rs::to_vec(&value)
            .map_err(CodecError::from)
            .doing(StorageError::Codec, &self.path)
            .attach_with(|| format!("encoding key: {path}"))?;

        let old_bytes = self
            .committed_or_buffered(&path)
            .change_context(StorageError::Write)
            .attach_with(|| format!("reading the value being replaced: {path}"))?;

        {
            let mut lock = self.pending.lock();
            lock.insert(Arc::from(path.as_str()), utils::PendingOp::Set(vec.clone()));
        }

        utils::emit_events(
            &self.subscriptions,
            StoreEvent {
                path: Arc::from(path.as_str()),
                op: StoreOp::Set,
                old: old_bytes,
                new: Some(vec),
                source,
            },
        );

        self.debouncer.schedule();
        Ok(())
    }

    fn save_now(&self) -> StorageResult<()> {
        self.flush_prefix(&StorePath::root())
    }

    fn scan_prefix(&self, prefix: &StorePath) -> StorageResult<Vec<(StorePath, Vec<u8>)>> {
        let bound = utils::subtree_bound(prefix);

        // Ordered by the engine and merged rather than folded into a tree, so
        // a scan costs one pass instead of a tree walk per row. `ORDER BY` is
        // load-bearing now: without it sqlite is free to return the range in
        // any order, and the tree that used to hide that is gone.
        let mut storage_results: Vec<(StorePath, Vec<u8>)> = Vec::new();

        {
            let conn = self.conn.lock();
            let mut stmt = conn
                .prepare_cached(
                    "SELECT key, value FROM data WHERE key >= ? AND key < ? ORDER BY key",
                )
                .map_err(SqliteStoreError::from)
                .doing(StorageError::Scan, &self.path)
                .attach_with(|| format!("prefix: {prefix}"))?;
            let (low, high) = utils::key_range(prefix);
            let range = format!("range: [{low}, {high})");
            let rows = stmt
                .query_map([low, high], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, Vec<u8>>(1)?))
                })
                .map_err(SqliteStoreError::from)
                .doing(StorageError::Scan, &self.path)
                .attach_with(|| format!("prefix: {prefix}"))
                .attach_with(|| range.clone())?;

            for row in rows {
                let (k, v) = row
                    .map_err(SqliteStoreError::from)
                    .doing(StorageError::Scan, &self.path)
                    .attach_with(|| format!("prefix: {prefix}"))
                    .attach_with(|| range.clone())
                    .attach_with(|| format!("rows read: {}", storage_results.len()))?;
                storage_results.push((utils::stored_path(&k)?, v));
            }
        }

        let mut buffered: Vec<(StorePath, Option<Vec<u8>>)> = {
            let lock = self.pending.lock();
            lock.iter()
                .filter(|(k, o)| o.is_data() && utils::is_under(k, prefix.as_str(), &bound))
                .map(|(k, op)| Ok((utils::stored_path(k)?, op.value().map(Vec::from))))
                .collect::<StorageResult<_>>()?
        };
        buffered.sort_unstable_by(|(a, _), (b, _)| a.cmp(b));

        Ok(utils::merge_buffered(storage_results, buffered))
    }

    fn scan_keys(&self, prefix: &StorePath) -> StorageResult<Vec<StorePath>> {
        let bound = utils::subtree_bound(prefix);
        // Values carried as empty vectors so the same merge serves both scans;
        // nothing reads them.
        let mut keys: Vec<(StorePath, Vec<u8>)> = Vec::new();

        {
            let conn = self.conn.lock();
            let mut stmt = conn
                .prepare_cached("SELECT key FROM data WHERE key >= ? AND key < ? ORDER BY key")
                .map_err(SqliteStoreError::from)
                .doing(StorageError::Scan, &self.path)
                .attach_with(|| format!("prefix: {prefix}"))?;
            let (low, high) = utils::key_range(prefix);
            let range = format!("range: [{low}, {high})");
            let rows = stmt
                .query_map([low, high], |row| row.get::<_, String>(0))
                .map_err(SqliteStoreError::from)
                .doing(StorageError::Scan, &self.path)
                .attach_with(|| format!("prefix: {prefix}"))
                .attach_with(|| range.clone())?;

            for row in rows {
                let key = row
                    .map_err(SqliteStoreError::from)
                    .doing(StorageError::Scan, &self.path)
                    .attach_with(|| format!("prefix: {prefix}"))
                    .attach_with(|| range.clone())
                    .attach_with(|| format!("keys read: {}", keys.len()))?;
                keys.push((utils::stored_path(&key)?, Vec::new()));
            }
        }

        let mut buffered: Vec<(StorePath, Option<Vec<u8>>)> = {
            let lock = self.pending.lock();
            lock.iter()
                .filter(|(k, o)| o.is_data() && utils::is_under(k, prefix.as_str(), &bound))
                .map(|(k, op)| Ok((utils::stored_path(k)?, op.value().map(|_| Vec::new()))))
                .collect::<StorageResult<_>>()?
        };
        buffered.sort_unstable_by(|(a, _), (b, _)| a.cmp(b));

        Ok(utils::merge_buffered(keys, buffered)
            .into_iter()
            .map(|(k, _)| k)
            .collect())
    }

    fn delete(&self, path: &StorePath, source: Option<uuid::Uuid>) -> StorageResult<()> {
        self.check_debouncer()?;
        let path_arc: Arc<str> = Arc::from(path.as_str());

        let old_bytes = self
            .committed_or_buffered(path)
            .change_context(StorageError::Delete)
            .attach_with(|| format!("reading the value being deleted: {path}"))?;

        let Some(old_bytes) = old_bytes else {
            return Ok(());
        };

        {
            let mut lock = self.pending.lock();
            lock.insert(path_arc.clone(), utils::PendingOp::Delete);
        }

        utils::emit_events(
            &self.subscriptions,
            StoreEvent {
                path: path_arc,
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

        let keys = self
            .scan_prefix(prefix)
            .change_context(StorageError::Delete)
            .attach_with(|| format!("listing the subtree being deleted: {prefix}"))?;

        {
            let mut lock = self.pending.lock();
            for (path, _) in keys {
                lock.insert(Arc::from(path.as_str()), utils::PendingOp::Delete);
            }
        }

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
        let id = self.next_sub_id.fetch_add(1, Ordering::Relaxed);
        self.subscriptions
            .write()
            .push(SubscriptionEntry { id, kind, callback });
        id
    }

    fn unsubscribe(&self, id: SubscriptionId) {
        self.subscriptions.write().retain(|s| s.id != id);
    }

    fn flush_async(&self) -> Commit {
        let commit = Commit::awaiting(self.commits.clone());
        self.debouncer.flush_now();
        commit
    }

    fn is_initialized(&self, namespace: &str) -> StorageResult<bool> {
        if self.initialized.lock().contains(namespace) {
            return Ok(true);
        }

        let key = utils::init_key(namespace);
        let found = {
            let conn = self.conn.lock();
            let mut stmt = conn
                .prepare_cached("SELECT 1 FROM metadata WHERE key = ?")
                .map_err(SqliteStoreError::from)
                .doing(StorageError::Meta, &self.path)
                .attach_with(|| format!("namespace: {namespace}"))?;
            stmt.exists([key])
                .map_err(SqliteStoreError::from)
                .doing(StorageError::Meta, &self.path)
                .attach_with(|| format!("namespace: {namespace}"))?
        };

        if found {
            self.initialized.lock().insert(Arc::from(namespace));
        }
        Ok(found)
    }

    fn set_initialized(&self, namespace: &str, state: InitState) -> StorageResult<()> {
        if self.initialized.lock().contains(namespace) == state.is_seeded() {
            return Ok(());
        }

        self.check_debouncer()?;
        let key: Arc<str> = Arc::from(namespace);
        self.pending
            .lock()
            .insert(Arc::clone(&key), utils::PendingOp::Init(state.is_seeded()));

        let mut initialized = self.initialized.lock();
        if state.is_seeded() {
            initialized.insert(key);
        } else {
            initialized.remove(&key);
        }
        drop(initialized);

        self.debouncer.schedule();
        Ok(())
    }
}

impl Drop for SqliteStoreInner {
    fn drop(&mut self) {
        utils::report_closing_flush(self.close(), &self.path);
    }
}

#[derive(Clone)]
pub struct SqliteStore {
    inner: Arc<SqliteStoreInner>,
}

impl std::fmt::Debug for SqliteStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SqliteStore").finish_non_exhaustive()
    }
}

impl PartialEq for SqliteStore {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.inner, &other.inner)
    }
}

impl Eq for SqliteStore {}

impl SqliteStore {
    pub fn open(
        config: StoreConfig,
        migration_set: MigrationSet,
    ) -> StorageResult<(Self, MigrationReport)> {
        let conn = Connection::open(&config.path)
            .map_err(SqliteStoreError::from)
            .doing(StorageError::Open, &config.path)?;

        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA synchronous = NORMAL;
             CREATE TABLE IF NOT EXISTS data (key TEXT PRIMARY KEY, value BLOB);
             CREATE TABLE IF NOT EXISTS metadata (key TEXT PRIMARY KEY, value BLOB);
             CREATE TABLE IF NOT EXISTS schema_snapshot (key TEXT PRIMARY KEY, value BLOB);
             CREATE TABLE IF NOT EXISTS migration_log (key TEXT PRIMARY KEY, value BLOB);",
        )
        .map_err(SqliteStoreError::from)
        .doing(StorageError::Open, &config.path)
        .attach("setting the pragmas and creating the tables")?;

        let conn_arc = Arc::new(Mutex::new(conn));
        let pending = Arc::new(Mutex::new(utils::Pending::new()));
        let initialized = Arc::new(Mutex::new(HashSet::<Arc<str>>::new()));
        let commits = Arc::new(CommitSignal::default());
        let subscriptions = Arc::new(RwLock::new(Vec::new()));
        let next_sub_id = Arc::new(AtomicU64::new(1));
        let write_lock = Arc::new(Mutex::new(()));

        let conn_save = conn_arc.clone();
        let pending_save = pending.clone();
        let write_lock_save = write_lock.clone();
        let path_save = config.path.clone();

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
                let _write_guard = write_lock_save.lock();

                let changes = {
                    let lock = pending_save.lock();
                    if lock.is_empty() {
                        return Ok(());
                    }
                    lock.clone()
                };

                let landed: StorageResult<()> = (|| {
                    let mut conn = conn_save.lock();
                    let txn = conn
                        .transaction()
                        .map_err(SqliteStoreError::from)
                        .doing(StorageError::Flush, &path_save)?;
                    apply_pending(&txn, &changes, &path_save)?;
                    txn.commit()
                        .map_err(SqliteStoreError::from)
                        .doing(StorageError::Flush, &path_save)
                        .attach_with(|| format!("buffered changes: {}", changes.len()))
                })();

                landed?;
                utils::clear_committed(&mut pending_save.lock(), &changes);
                Ok(())
            },
        );

        let inner = Arc::new(SqliteStoreInner {
            conn: conn_arc,
            path: config.path.clone(),
            pending,
            initialized,
            commits,
            health,
            debouncer: Arc::new(debouncer),
            subscriptions,
            next_sub_id,
            write_lock,
        });

        let store = Self { inner };
        let report = store.run_migrations(migration_set)?;

        Ok((store, report))
    }

    pub fn close(&mut self) -> StorageResult<()> {
        self.inner.close()
    }
}

impl SchemaAwareStore for SqliteStore {
    fn run_migrations(&self, mset: MigrationSet) -> StorageResult<MigrationReport> {
        self.inner.run_migrations(mset)
    }
}

impl StoreBackend for SqliteStore {
    fn get_raw(&self, path: &StorePath) -> StorageResult<Option<Vec<u8>>> {
        self.inner.get_raw(path)
    }

    fn get_erased(
        &self,
        path: &StorePath,
        f: &mut dyn FnMut(&mut dyn erased_serde::Deserializer) -> StorageResult<()>,
    ) -> StorageResult<bool> {
        match self.inner.get_raw(path)? {
            Some(bytes) => {
                self.decode_erased(&bytes, f)
                    .doing(StorageError::Read, &self.inner.path)
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
        let mut de = sonic_rs::Deserializer::from_slice(bytes);
        let mut erased = <dyn erased_serde::Deserializer>::erase(&mut de);
        f(&mut erased).attach_with(|| format!("decoding {} bytes of json", bytes.len()))
    }

    fn set_erased(
        &self,
        path: &StorePath,
        value: &dyn erased_serde::Serialize,
        source: Option<uuid::Uuid>,
    ) -> StorageResult<()> {
        self.inner.set_erased(path, value, source)
    }

    fn set_owned_erased(
        &self,
        path: StorePath,
        value: &dyn erased_serde::Serialize,
        source: Option<uuid::Uuid>,
    ) -> StorageResult<()> {
        self.inner.set_owned_erased(path, value, source)
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

    fn flush_prefix(&self, prefix: &StorePath) -> StorageResult<()> {
        self.inner.flush_prefix(prefix)
    }

    fn flush_async(&self) -> Commit {
        self.inner.flush_async()
    }

    fn is_initialized(&self, namespace: &str) -> StorageResult<bool> {
        self.inner.is_initialized(namespace)
    }

    fn set_initialized(&self, namespace: &str, state: InitState) -> StorageResult<()> {
        self.inner.set_initialized(namespace, state)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::store::StoreExt;
    use amethystate_core::test_utils::unique_path;
    use serial_test::serial;
    use std::thread;
    use std::time::Duration;

    #[test]
    #[serial]
    fn test_debouncer_persistence() {
        let path = unique_path("debounce");

        let mut config = StoreConfig::new(path);
        config.save_debounce = Duration::from_millis(50);

        let (store, _) = SqliteStore::open(config, MigrationSet::default()).unwrap();

        store.set(["config", "port"], &8080u16).unwrap();

        {
            let conn = store.inner.conn.lock();
            let mut stmt = conn
                .prepare("SELECT 1 FROM data WHERE key = 'config.port'")
                .unwrap();
            assert!(!stmt.exists([]).unwrap());
        }

        thread::sleep(Duration::from_millis(500));

        {
            let conn = store.inner.conn.lock();
            let mut stmt = conn
                .prepare("SELECT 1 FROM data WHERE key = 'config.port'")
                .unwrap();
            assert!(stmt.exists([]).unwrap());
        }
    }

    #[test]
    fn test_delete_flow() {
        let path = unique_path("delete");
        let (store, _) =
            SqliteStore::open(StoreConfig::new(path), MigrationSet::default()).unwrap();

        store.set(["temp", "key"], &1).unwrap();

        store.save_now().unwrap();
        store
            .delete(&StorePath::from_segments(["temp", "key"]))
            .unwrap();
        assert_eq!(store.get::<i32>(["temp", "key"]).unwrap(), None);

        store.save_now().unwrap();

        let conn = store.inner.conn.lock();
        let mut stmt = conn
            .prepare("SELECT 1 FROM data WHERE key = 'temp.key'")
            .unwrap();
        assert!(!stmt.exists([]).unwrap());
    }

    #[test]
    fn test_close_saves_pending_data() {
        let path = unique_path("save_on_close");
        let mut config = StoreConfig::new(&path);
        config.save_debounce = Duration::from_secs(3600);

        {
            let (mut store, _) = SqliteStore::open(config, MigrationSet::default()).unwrap();
            store.set(["urgent", "data"], &true).unwrap();
            store.close().unwrap();
        }

        let (store, _) =
            SqliteStore::open(StoreConfig::new(&path), MigrationSet::default()).unwrap();
        assert_eq!(store.get::<bool>(["urgent", "data"]).unwrap(), Some(true));
    }

    #[test]
    fn test_granular_flush_prefix_drains_buffer() {
        let path = unique_path("granular_flush");
        let mut config = StoreConfig::new(&path);

        config.save_debounce = Duration::from_secs(3600);

        let (store, _) = SqliteStore::open(config, MigrationSet::default()).unwrap();

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
            let conn = store.inner.conn.lock();
            let mut stmt = conn.prepare("SELECT 1 FROM data WHERE key = ?").unwrap();
            assert!(!stmt.exists(["net.host"]).unwrap());
            assert!(!stmt.exists(["ui.theme"]).unwrap());
        }

        store
            .flush_prefix(&StorePath::from_segments(["net"]))
            .unwrap();

        {
            let conn = store.inner.conn.lock();
            let mut stmt = conn
                .prepare("SELECT value FROM data WHERE key = ?")
                .unwrap();
            let host_bytes: Vec<u8> = stmt.query_row(["net.host"], |r| r.get(0)).unwrap();
            assert_eq!(store.decode::<String>(&host_bytes).unwrap(), "127.0.0.1");

            let port_bytes: Vec<u8> = stmt.query_row(["net.port"], |r| r.get(0)).unwrap();
            assert_eq!(store.decode::<u16>(&port_bytes).unwrap(), 8080);

            assert!(
                !stmt.exists(["ui.theme"]).unwrap(),
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
            let conn = store.inner.conn.lock();
            let mut stmt = conn
                .prepare("SELECT 1 FROM data WHERE key = 'ui.theme'")
                .unwrap();
            assert!(
                stmt.exists([]).unwrap(),
                "UI should now be persisted on disk"
            );
        }
    }
}
