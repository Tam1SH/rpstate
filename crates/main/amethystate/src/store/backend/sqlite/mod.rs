use crate::MigrationReport;
use crate::codec::CodecError;
use crate::migration::engine::{MigrationEngine, StorageProvider};
use crate::migration::set::MigrationSet;
use crate::store::StorageResult;
use crate::store::backend::sqlite::migration::SqliteMigrationBackend;
use crate::store::backend::utils;
use crate::store::config::StoreConfig;
use crate::store::traits::MigrationBackendAdapter;
use crate::store::util::debouncer::Debouncer;
use crate::store::{
    SchemaAwareStore, StoreBackend, StoreCallback, StoreEvent, StoreOp, SubscriptionEntry,
    SubscriptionId, SubscriptionKind,
};
use amethystate_core::path::StorePath;
use error::SqliteStoreError;
use parking_lot::{Mutex, RwLock};
use rusqlite::{Connection, OptionalExtension};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use tracing::info;

pub mod error;
mod inspector;
mod migration;

struct SqliteStoreInner {
    conn: Arc<Mutex<Connection>>,
    pending: Arc<Mutex<utils::Pending>>,
    initialized: Arc<Mutex<HashSet<Arc<str>>>>,
    commits: Arc<crate::store::durable::CommitSignal>,
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
            let txn = conn.transaction().map_err(SqliteStoreError::from)?;
            {
                let mut ins = txn
                    .prepare_cached("REPLACE INTO data (key, value) VALUES (?, ?)")
                    .map_err(SqliteStoreError::from)?;
                let mut del = txn
                    .prepare_cached("DELETE FROM data WHERE key = ?")
                    .map_err(SqliteStoreError::from)?;
                let mut mark = txn
                    .prepare_cached("REPLACE INTO metadata (key, value) VALUES (?, ?)")
                    .map_err(SqliteStoreError::from)?;

                for (path, op) in &changes {
                    match op {
                        utils::PendingOp::Set(b) => {
                            ins.execute(rusqlite::params![&**path, &b[..]])
                                .map_err(SqliteStoreError::from)?;
                        }
                        utils::PendingOp::Delete => {
                            del.execute([&**path]).map_err(SqliteStoreError::from)?;
                        }
                        utils::PendingOp::MarkInit => {
                            mark.execute(rusqlite::params![
                                format!("__init::{path}"),
                                [] as [u8; 0]
                            ])
                            .map_err(SqliteStoreError::from)?;
                        }
                    }
                }
            }
            txn.commit().map_err(SqliteStoreError::from)?;
        }

        utils::clear_committed(&mut self.pending.lock(), &changes);
        self.commits.finished(true);
        Ok(())
    }

    fn check_debouncer(&self) {
        if self.debouncer.is_poisoned() {
            panic!("debouncer thread is dead — store integrity cannot be guaranteed");
        }
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
            .map_err(SqliteStoreError::from)?;

        Ok(stmt
            .query_row([path.as_str()], |row| row.get::<_, Vec<u8>>(0))
            .optional()
            .map_err(SqliteStoreError::from)?)
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
                let txn = conn.transaction().map_err(SqliteStoreError::from)?;

                let res = {
                    let mut storage = SqliteMigrationBackend::new(&txn);
                    f(&mut storage)?
                };

                txn.commit().map_err(SqliteStoreError::from)?;
                Ok(res)
            }
        }

        let provider = SqliteProvider { conn: &self.conn };
        let engine = MigrationEngine::new(&provider);
        engine.run(mset)
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
            .map_err(SqliteStoreError::from)?;
        let res: Option<Vec<u8>> = stmt
            .query_row([path.as_str()], |row| row.get(0))
            .optional()
            .map_err(SqliteStoreError::from)?;
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
        self.check_debouncer();
        let vec = sonic_rs::to_vec(&value)
            .map_err(CodecError::from)
            .map_err(SqliteStoreError::from)?;

        let old_bytes = self.committed_or_buffered(&path)?;

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

    fn scan_prefix(&self, prefix: &StorePath) -> StorageResult<Vec<(String, Vec<u8>)>> {
        let bound = utils::subtree_bound(prefix);
        let mut storage_results = Vec::new();

        {
            let conn = self.conn.lock();
            let mut stmt = conn
                .prepare_cached("SELECT key, value FROM data WHERE key >= ? AND key < ?")
                .map_err(SqliteStoreError::from)?;
            let (low, high) = utils::key_range(prefix);
            let rows = stmt
                .query_map([low, high], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, Vec<u8>>(1)?))
                })
                .map_err(SqliteStoreError::from)?;

            for row in rows {
                let (k, v) = row.map_err(SqliteStoreError::from)?;
                storage_results.push((k, v));
            }
        }

        let mut pending_map = HashMap::new();
        {
            let lock = self.pending.lock();
            for (k, op) in lock.iter().filter(|(_, o)| o.is_data()) {
                if utils::is_under(k, prefix.as_str(), &bound) {
                    pending_map.insert(k.to_string(), op.value().map(Vec::from));
                }
            }
        }

        for (k, opt_v) in pending_map {
            if let Some(v) = opt_v {
                if let Some(pos) = storage_results.iter().position(|(rk, _)| *rk == k) {
                    storage_results[pos].1 = v;
                } else {
                    storage_results.push((k, v));
                }
            } else {
                storage_results.retain(|(rk, _)| *rk != k);
            }
        }

        storage_results.sort_by(|(a, _), (b, _)| a.cmp(b));
        Ok(storage_results)
    }

    fn scan_keys(&self, prefix: &StorePath) -> StorageResult<Vec<String>> {
        let bound = utils::subtree_bound(prefix);
        let mut keys = Vec::new();

        {
            let conn = self.conn.lock();
            let mut stmt = conn
                .prepare_cached("SELECT key FROM data WHERE key >= ? AND key < ?")
                .map_err(SqliteStoreError::from)?;
            let (low, high) = utils::key_range(prefix);
            let rows = stmt
                .query_map([low, high], |row| row.get::<_, String>(0))
                .map_err(SqliteStoreError::from)?;

            for row in rows {
                keys.push(row.map_err(SqliteStoreError::from)?);
            }
        }

        {
            let lock = self.pending.lock();
            for (k, op) in lock.iter().filter(|(_, o)| o.is_data()) {
                if !utils::is_under(k, prefix.as_str(), &bound) {
                    continue;
                }
                match op.value() {
                    Some(_) if !keys.iter().any(|existing| existing == &**k) => {
                        keys.push(k.to_string())
                    }
                    Some(_) => {}
                    None => keys.retain(|existing| existing != &**k),
                }
            }
        }

        keys.sort();
        Ok(keys)
    }

    fn delete(&self, path: &StorePath, source: Option<uuid::Uuid>) -> StorageResult<()> {
        self.check_debouncer();
        let path_arc: Arc<str> = Arc::from(path.as_str());

        let old_bytes = self.committed_or_buffered(path)?;

        {
            let mut lock = self.pending.lock();
            lock.insert(path_arc.clone(), utils::PendingOp::Delete);
        }

        utils::emit_events(
            &self.subscriptions,
            StoreEvent {
                path: path_arc,
                op: StoreOp::Delete,
                old: old_bytes,
                new: None,
                source,
            },
        );

        self.debouncer.schedule();
        Ok(())
    }

    fn delete_prefix(&self, prefix: &StorePath, source: Option<uuid::Uuid>) -> StorageResult<()> {
        self.check_debouncer();

        let keys = self.scan_prefix(prefix)?;

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

    fn flush_async(&self) -> crate::store::durable::Commit {
        let commit = crate::store::durable::Commit::awaiting(self.commits.clone());
        self.debouncer.flush_now();
        commit
    }

    fn is_initialized(&self, namespace: &str) -> StorageResult<bool> {
        if self.initialized.lock().contains(namespace) {
            return Ok(true);
        }

        let key = format!("__init::{namespace}");
        let found = {
            let conn = self.conn.lock();
            let mut stmt = conn
                .prepare_cached("SELECT 1 FROM metadata WHERE key = ?")
                .map_err(SqliteStoreError::from)?;
            stmt.exists([key]).map_err(SqliteStoreError::from)?
        };

        if found {
            self.initialized.lock().insert(Arc::from(namespace));
        }
        Ok(found)
    }

    fn mark_initialized(&self, namespace: &str) -> StorageResult<()> {
        if self.initialized.lock().contains(namespace) {
            return Ok(());
        }

        self.check_debouncer();
        let key: Arc<str> = Arc::from(namespace);
        self.pending
            .lock()
            .insert(Arc::clone(&key), utils::PendingOp::MarkInit);
        self.initialized.lock().insert(key);
        self.debouncer.schedule();
        Ok(())
    }
}

impl Drop for SqliteStoreInner {
    fn drop(&mut self) {
        let _ = self.close();
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
        let conn = Connection::open(&config.path).map_err(SqliteStoreError::from)?;

        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA synchronous = NORMAL;
             CREATE TABLE IF NOT EXISTS data (key TEXT PRIMARY KEY, value BLOB);
             CREATE TABLE IF NOT EXISTS metadata (key TEXT PRIMARY KEY, value BLOB);
             CREATE TABLE IF NOT EXISTS schema_snapshot (key TEXT PRIMARY KEY, value BLOB);
             CREATE TABLE IF NOT EXISTS migration_log (key TEXT PRIMARY KEY, value BLOB);",
        )
        .map_err(SqliteStoreError::from)?;

        let conn_arc = Arc::new(Mutex::new(conn));
        let pending = Arc::new(Mutex::new(utils::Pending::new()));
        let initialized = Arc::new(Mutex::new(HashSet::<Arc<str>>::new()));
        let commits = Arc::new(crate::store::durable::CommitSignal::default());
        let subscriptions = Arc::new(RwLock::new(Vec::new()));
        let next_sub_id = Arc::new(AtomicU64::new(1));
        let write_lock = Arc::new(Mutex::new(()));

        let conn_save = conn_arc.clone();
        let commits_save = commits.clone();
        let pending_save = pending.clone();
        let write_lock_save = write_lock.clone();

        let debouncer = Debouncer::new(config.save_debounce, move || {
            let _write_guard = write_lock_save.lock();

            let changes = {
                let lock = pending_save.lock();
                if lock.is_empty() {
                    return;
                }
                lock.clone()
            };

            let mut conn = conn_save.lock();
            if let Ok(txn) = conn.transaction() {
                let mut success = true;
                {
                    let mut ins = match txn.prepare("REPLACE INTO data (key, value) VALUES (?, ?)")
                    {
                        Ok(s) => s,
                        Err(_) => {
                            return;
                        }
                    };
                    let mut del = match txn.prepare("DELETE FROM data WHERE key = ?") {
                        Ok(s) => s,
                        Err(_) => {
                            return;
                        }
                    };
                    let mut mark =
                        match txn.prepare("REPLACE INTO metadata (key, value) VALUES (?, ?)") {
                            Ok(s) => s,
                            Err(_) => {
                                return;
                            }
                        };

                    for (path, op) in &changes {
                        match op {
                            utils::PendingOp::Set(b) => {
                                if ins.execute(rusqlite::params![&**path, &b[..]]).is_err() {
                                    success = false;
                                    break;
                                }
                            }
                            utils::PendingOp::Delete => {
                                if del.execute([&**path]).is_err() {
                                    success = false;
                                    break;
                                }
                            }
                            utils::PendingOp::MarkInit => {
                                if mark
                                    .execute(rusqlite::params![
                                        format!("__init::{path}"),
                                        [] as [u8; 0]
                                    ])
                                    .is_err()
                                {
                                    success = false;
                                    break;
                                }
                            }
                        }
                    }
                }
                let landed = success && txn.commit().is_ok();
                if landed {
                    utils::clear_committed(&mut pending_save.lock(), &changes);
                }
                commits_save.finished(landed);
            }
        });

        let inner = Arc::new(SqliteStoreInner {
            conn: conn_arc,
            pending,
            initialized,
            commits,
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
                self.decode_erased(&bytes, f)?;
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
        f(&mut erased)
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

    fn scan_prefix(&self, prefix: &StorePath) -> StorageResult<Vec<(String, Vec<u8>)>> {
        self.inner.scan_prefix(prefix)
    }

    fn scan_keys(&self, prefix: &StorePath) -> StorageResult<Vec<String>> {
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

    fn flush_async(&self) -> crate::store::durable::Commit {
        self.inner.flush_async()
    }

    fn is_initialized(&self, namespace: &str) -> StorageResult<bool> {
        self.inner.is_initialized(namespace)
    }

    fn mark_initialized(&self, namespace: &str) -> StorageResult<()> {
        self.inner.mark_initialized(namespace)
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
