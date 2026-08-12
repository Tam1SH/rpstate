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
    SchemaAwareStore, Store, StoreCallback, StoreEvent, StoreOp, SubscriptionEntry, SubscriptionId,
    SubscriptionKind,
};
use error::SqliteStoreError;
use parking_lot::{Mutex, RwLock};
use rusqlite::{Connection, OptionalExtension};
use serde::Serialize;
use serde::de::DeserializeOwned;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use tracing::{info, warn};

pub mod error;
mod inspector;
mod migration;

struct SqliteStoreInner {
    conn: Arc<Mutex<Connection>>,
    pending: Arc<Mutex<HashMap<Arc<str>, Option<Vec<u8>>>>>,
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

    pub fn flush_prefix(&self, prefix: &str) -> StorageResult<()> {
        let _write_guard = self.write_lock.lock();

        let changes = {
            let lock = self.pending.lock();
            utils::pending_prefix(&lock, prefix)
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

                for (path, opt_bytes) in &changes {
                    match opt_bytes {
                        Some(b) => {
                            ins.execute(rusqlite::params![&**path, &b[..]])
                                .map_err(SqliteStoreError::from)?;
                        }
                        None => {
                            del.execute([&**path]).map_err(SqliteStoreError::from)?;
                        }
                    }
                }
            }
            txn.commit().map_err(SqliteStoreError::from)?;
        }

        utils::clear_committed(&mut self.pending.lock(), &changes);
        Ok(())
    }

    fn check_debouncer(&self) {
        if self.debouncer.is_poisoned() {
            panic!("debouncer thread is dead — store integrity cannot be guaranteed");
        }
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

    fn get<T: DeserializeOwned>(&self, path: &str) -> StorageResult<Option<T>> {
        {
            let lock = self.pending.lock();
            if let Some(opt_bytes) = lock.get(path) {
                return match opt_bytes {
                    Some(bytes) => Ok(Some(
                        sonic_rs::from_slice(bytes)
                            .map_err(CodecError::from)
                            .map_err(SqliteStoreError::from)?,
                    )),
                    None => Ok(None),
                };
            }
        }

        let conn = self.conn.lock();
        let mut stmt = conn
            .prepare_cached("SELECT value FROM data WHERE key = ?")
            .map_err(SqliteStoreError::from)?;
        let res: Option<Vec<u8>> = stmt
            .query_row([path], |row| row.get(0))
            .optional()
            .map_err(SqliteStoreError::from)?;

        match res {
            Some(bytes) => Ok(Some(
                sonic_rs::from_slice(&bytes)
                    .map_err(CodecError::from)
                    .map_err(SqliteStoreError::from)?,
            )),
            None => Ok(None),
        }
    }

    fn set<T: Serialize>(
        &self,
        path: &str,
        value: &T,
        source: Option<uuid::Uuid>,
    ) -> StorageResult<()> {
        self.set_owned_with_source(Arc::from(path), value, source)
    }

    fn set_owned_with_source<T: Serialize>(
        &self,
        path: Arc<str>,
        value: &T,
        source: Option<uuid::Uuid>,
    ) -> StorageResult<()> {
        self.check_debouncer();
        let vec = sonic_rs::to_vec(value)
            .map_err(CodecError::from)
            .map_err(SqliteStoreError::from)?;

        let old_bytes = {
            let lock = self.pending.lock();
            lock.get(&*path).cloned().flatten()
        };

        {
            let mut lock = self.pending.lock();
            lock.insert(path.clone(), Some(vec.clone()));
        }

        utils::emit_events(
            &self.subscriptions,
            StoreEvent {
                path,
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
        self.flush_prefix("")
    }

    fn scan_prefix(&self, prefix: &str) -> StorageResult<Vec<(String, Vec<u8>)>> {
        let mut storage_results = Vec::new();

        {
            let conn = self.conn.lock();
            let mut stmt = conn
                .prepare_cached("SELECT key, value FROM data WHERE key GLOB ?")
                .map_err(SqliteStoreError::from)?;
            let pattern = format!("{}*", prefix);
            let rows = stmt
                .query_map([pattern], |row| {
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
            for (k, opt_v) in lock.iter() {
                if k.starts_with(prefix) {
                    pending_map.insert(k.to_string(), opt_v.clone());
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

    fn delete(&self, path: &str, source: Option<uuid::Uuid>) -> StorageResult<()> {
        self.check_debouncer();
        let path_arc: Arc<str> = Arc::from(path);

        let old_bytes = {
            let lock = self.pending.lock();
            if let Some(p) = lock.get(path) {
                p.clone()
            } else {
                let conn = self.conn.lock();
                let mut stmt = conn
                    .prepare_cached("SELECT value FROM data WHERE key = ?")
                    .map_err(SqliteStoreError::from)?;
                stmt.query_row([path], |row| row.get::<_, Vec<u8>>(0))
                    .optional()
                    .map_err(SqliteStoreError::from)?
            }
        };

        {
            let mut lock = self.pending.lock();
            lock.insert(path_arc.clone(), None);
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

    fn delete_prefix(&self, prefix: &str, source: Option<uuid::Uuid>) -> StorageResult<()> {
        self.check_debouncer();

        let keys = self.scan_prefix(prefix)?;

        {
            let mut lock = self.pending.lock();
            for (path, _) in keys {
                lock.insert(Arc::from(path.as_str()), None);
            }
        }

        utils::emit_events(
            &self.subscriptions,
            StoreEvent {
                path: Arc::from(prefix),
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

    fn decode<T: DeserializeOwned + Default>(&self, bytes: &[u8]) -> StorageResult<T> {
        match sonic_rs::from_slice(bytes) {
            Ok(val) => Ok(val),
            Err(e) => {
                warn!("Failed to decode SQLite field. Using Default. Error: {e}");
                Ok(T::default())
            }
        }
    }

    fn is_initialized(&self, namespace: &str) -> StorageResult<bool> {
        let key = format!("__init::{namespace}");
        let conn = self.conn.lock();
        let mut stmt = conn
            .prepare_cached("SELECT 1 FROM metadata WHERE key = ?")
            .map_err(SqliteStoreError::from)?;
        Ok(stmt.exists([key]).map_err(SqliteStoreError::from)?)
    }

    fn mark_initialized(&self, namespace: &str) -> StorageResult<()> {
        let key = format!("__init::{namespace}");
        let conn = self.conn.lock();
        conn.execute(
            "REPLACE INTO metadata (key, value) VALUES (?, ?)",
            rusqlite::params![key, [] as [u8; 0]],
        )
        .map_err(SqliteStoreError::from)?;
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
        let pending = Arc::new(Mutex::new(HashMap::<Arc<str>, Option<Vec<u8>>>::new()));
        let subscriptions = Arc::new(RwLock::new(Vec::new()));
        let next_sub_id = Arc::new(AtomicU64::new(1));
        let write_lock = Arc::new(Mutex::new(()));

        let conn_save = conn_arc.clone();
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

                    for (path, opt_bytes) in &changes {
                        match opt_bytes {
                            Some(b) => {
                                if ins.execute(rusqlite::params![&**path, &b[..]]).is_err() {
                                    success = false;
                                    break;
                                }
                            }
                            None => {
                                if del.execute([&**path]).is_err() {
                                    success = false;
                                    break;
                                }
                            }
                        }
                    }
                }
                if success && txn.commit().is_ok() {
                    utils::clear_committed(&mut pending_save.lock(), &changes);
                }
            }
        });

        let inner = Arc::new(SqliteStoreInner {
            conn: conn_arc,
            pending,
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

impl Store for SqliteStore {
    fn get<T: DeserializeOwned>(&self, path: &str) -> StorageResult<Option<T>> {
        self.inner.get(path)
    }

    fn set<T: Serialize>(&self, path: &str, value: &T) -> StorageResult<()> {
        self.set_with_source(path, value, None)
    }

    fn set_with_source<T: Serialize>(
        &self,
        path: &str,
        value: &T,
        source: Option<uuid::Uuid>,
    ) -> StorageResult<()> {
        self.inner.set(path, value, source)
    }

    fn set_owned<T: Serialize>(&self, path: Arc<str>, value: &T) -> StorageResult<()> {
        self.set_owned_with_source(path, value, None)
    }

    fn set_owned_with_source<T: Serialize>(
        &self,
        path: Arc<str>,
        value: &T,
        source: Option<uuid::Uuid>,
    ) -> StorageResult<()> {
        self.inner.set_owned_with_source(path, value, source)
    }

    fn save_now(&self) -> StorageResult<()> {
        self.inner.save_now()
    }

    fn scan_prefix(&self, prefix: &str) -> StorageResult<Vec<(String, Vec<u8>)>> {
        self.inner.scan_prefix(prefix)
    }

    fn delete(&self, path: &str) -> StorageResult<()> {
        self.delete_with_source(path, None)
    }

    fn delete_with_source(&self, path: &str, source: Option<uuid::Uuid>) -> StorageResult<()> {
        self.inner.delete(path, source)
    }

    fn delete_prefix_with_source(
        &self,
        prefix: &str,
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

    fn decode<T: DeserializeOwned + Default>(&self, bytes: &[u8]) -> StorageResult<T> {
        self.inner.decode(bytes)
    }

    fn flush_prefix(&self, prefix: &str) -> StorageResult<()> {
        self.inner.flush_prefix(prefix)
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

        store.set("config.port", &8080u16).unwrap();

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

        store.set("temp.key", &1).unwrap();

        store.save_now().unwrap();
        store.delete("temp.key").unwrap();
        assert_eq!(store.get::<i32>("temp.key").unwrap(), None);

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
            store.set("urgent.data", &true).unwrap();
            store.close().unwrap();
        }

        let (store, _) =
            SqliteStore::open(StoreConfig::new(&path), MigrationSet::default()).unwrap();
        assert_eq!(store.get::<bool>("urgent.data").unwrap(), Some(true));
    }

    #[test]
    fn test_granular_flush_prefix_drains_buffer() {
        let path = unique_path("granular_flush");
        let mut config = StoreConfig::new(&path);

        config.save_debounce = Duration::from_secs(3600);

        let (store, _) = SqliteStore::open(config, MigrationSet::default()).unwrap();

        store.set("net.host", &"127.0.0.1".to_string()).unwrap();
        store.set("net.port", &8080u16).unwrap();
        store.set("ui.theme", &"dark".to_string()).unwrap();

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

        store.flush_prefix("net").unwrap();

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

        store.flush_prefix("").unwrap();
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
