use crate::store::{
    SchemaAwareStore, Store, StoreCallback, StoreEvent, StoreOp, SubscriptionEntry, SubscriptionId,
    SubscriptionKind,
};
use error::RedbStoreError;
use migration::RedbMigrationBackend;
use redb::{Database, ReadableDatabase};
use serde::Serialize;
use serde::de::DeserializeOwned;
use std::collections::HashMap;
use tables::{TABLE_DATA, TABLE_DIFF_LOG, TABLE_META, TABLE_MIGRATION_LOG};

use crate::store::config::StoreConfig;
use crate::{MigrationReport, store::error::StorageResult};

use crate::codec::CodecError;
use crate::migration::engine::{MigrationEngine, StorageProvider};
use crate::migration::set::MigrationSet;
use crate::store::backend::redb::tables::TABLE_SCHEMA_SNAPSHOT;
use crate::store::backend::utils;
use crate::store::traits::MigrationBackendAdapter;
use crate::store::util::debouncer::Debouncer;
use parking_lot::{Mutex, RwLock};
use rmp_serde::Serializer;
use rmp_serde::config::BytesMode;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use tracing::{info, warn};
use uuid::Uuid;

pub mod error;
mod inspector;
mod migration;
mod tables;

const BUF_SIZE: usize = 64 * 1024;

#[cfg(test)]
static SIMULATE_WRITE_FAILURE: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

thread_local! {
    static SERIALIZATION_BUFFER: std::cell::RefCell<Vec<u8>> =
        std::cell::RefCell::new(Vec::with_capacity(BUF_SIZE));
}

struct RedbStoreInner {
    db: Arc<Database>,
    pending: Arc<Mutex<HashMap<Arc<str>, Option<Vec<u8>>>>>,
    debouncer: Arc<Debouncer>,
    subscriptions: Arc<RwLock<Vec<SubscriptionEntry>>>,
    next_sub_id: Arc<AtomicU64>,
    write_lock: Arc<Mutex<()>>,
}

impl RedbStoreInner {
    pub fn close(&self) -> StorageResult<()> {
        info!("Closing RedbStore...");
        self.save_now()?;
        Ok(())
    }

    pub fn save_now(&self) -> StorageResult<()> {
        self.flush_prefix("")
    }

    pub fn flush_prefix(&self, prefix: &str) -> StorageResult<()> {
        let _write_guard = self.write_lock.lock();

        let changes = {
            let mut lock = self.pending.lock();
            utils::drain_pending_prefix(&mut *lock, prefix)
        };

        let txn = self.db.begin_write().map_err(RedbStoreError::from)?;
        {
            let mut table = txn.open_table(TABLE_DATA).map_err(RedbStoreError::from)?;
            for (path, opt_bytes) in changes {
                match opt_bytes {
                    Some(b) => {
                        table.insert(&*path, &b[..]).map_err(RedbStoreError::from)?;
                    }
                    None => {
                        table.remove(&*path).map_err(RedbStoreError::from)?;
                    }
                }
            }
        }
        txn.commit().map_err(RedbStoreError::from)?;
        Ok(())
    }

    fn check_debouncer(&self) {
        if self.debouncer.is_poisoned() {
            panic!("debouncer thread is dead — store integrity cannot be guaranteed");
        }
    }
}

impl Drop for RedbStoreInner {
    fn drop(&mut self) {
        let _ = self.close();
    }
}

#[derive(Clone)]
pub struct RedbStore {
    inner: Arc<RedbStoreInner>,
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
        let db = Arc::new(Database::create(&config.path).map_err(RedbStoreError::from)?);

        let write_txn = db.begin_write().map_err(RedbStoreError::from)?;
        {
            let _ = write_txn
                .open_table(TABLE_DATA)
                .map_err(RedbStoreError::from)?;
            let _ = write_txn
                .open_table(TABLE_META)
                .map_err(RedbStoreError::from)?;
            let _ = write_txn
                .open_table(TABLE_DIFF_LOG)
                .map_err(RedbStoreError::from)?;
            let _ = write_txn
                .open_table(TABLE_MIGRATION_LOG)
                .map_err(RedbStoreError::from)?;
            let _ = write_txn
                .open_table(TABLE_SCHEMA_SNAPSHOT)
                .map_err(RedbStoreError::from)?;
        }
        write_txn.commit().map_err(RedbStoreError::from)?;

        let pending = Arc::new(Mutex::new(HashMap::<Arc<str>, Option<Vec<u8>>>::new()));
        let subscriptions = Arc::new(RwLock::new(Vec::new()));

        let db_save = db.clone();
        let pending_save = pending.clone();

        let write_lock = Arc::new(Mutex::new(()));
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

            let success = (|| -> Option<bool> {
                #[cfg(test)]
                if SIMULATE_WRITE_FAILURE.load(Ordering::Relaxed) {
                    return None;
                }

                let txn = db_save.begin_write().ok()?;
                {
                    let mut table = txn.open_table(TABLE_DATA).ok()?;
                    for (path, opt_bytes) in &changes {
                        match opt_bytes {
                            Some(b) => {
                                table.insert(&**path, &b[..]).ok()?;
                            }
                            None => {
                                table.remove(&**path).ok()?;
                            }
                        }
                    }
                }
                txn.commit().ok()?;
                Some(true)
            })()
            .unwrap_or(false);

            if success {
                let mut lock = pending_save.lock();
                for (key, committed) in &changes {
                    // Only if the buffer still holds what was just written. A
                    // write landing while the commit was in flight is a
                    // different value, and dropping it here would lose it: it
                    // is not on disk, and nothing would write it later.
                    if lock.get(key) == Some(committed) {
                        lock.remove(key);
                    }
                }
            }
        });

        let inner = Arc::new(RedbStoreInner {
            db,
            pending,
            debouncer: Arc::new(debouncer),
            subscriptions,
            next_sub_id: Arc::new(AtomicU64::new(1)),
            write_lock,
        });

        let store = Self { inner };
        let report = store.run_migrations(migration_set)?;

        Ok((store, report))
    }

    pub fn close(&self) -> StorageResult<()> {
        self.inner.close()
    }
}

impl SchemaAwareStore for RedbStore {
    fn run_migrations(&self, mset: MigrationSet) -> StorageResult<MigrationReport> {
        struct RedbProvider<'a> {
            db: &'a Database,
        }

        impl<'a> StorageProvider for RedbProvider<'a> {
            fn atomic<F, T>(&self, f: F) -> StorageResult<T>
            where
                F: FnOnce(&mut dyn MigrationBackendAdapter) -> StorageResult<T>,
            {
                let write_txn = self.db.begin_write().map_err(RedbStoreError::from)?;

                let res = {
                    let mut storage = RedbMigrationBackend::new(&write_txn);
                    f(&mut storage)?
                };

                write_txn.commit().map_err(RedbStoreError::from)?;
                Ok(res)
            }
        }

        let provider = RedbProvider { db: &self.inner.db };
        let engine = MigrationEngine::new(&provider);
        engine.run(mset)
    }
}

impl Store for RedbStore {
    fn get<T: DeserializeOwned>(&self, path: &str) -> StorageResult<Option<T>> {
        {
            let lock = self.inner.pending.lock();
            if let Some(opt_bytes) = lock.get(path) {
                return match opt_bytes {
                    Some(bytes) => Ok(Some(
                        rmp_serde::from_slice(bytes)
                            .map_err(CodecError::from)
                            .map_err(RedbStoreError::from)?,
                    )),
                    None => Ok(None),
                };
            }
        }

        let read_txn = self.inner.db.begin_read().map_err(RedbStoreError::from)?;
        let table = read_txn
            .open_table(TABLE_DATA)
            .map_err(RedbStoreError::from)?;
        match table.get(path).map_err(RedbStoreError::from)? {
            Some(access_guard) => {
                let bytes = access_guard.value();
                Ok(Some(
                    rmp_serde::from_slice(bytes)
                        .map_err(CodecError::from)
                        .map_err(RedbStoreError::from)?,
                ))
            }
            None => Ok(None),
        }
    }

    fn set<T: Serialize>(&self, path: &str, value: &T) -> StorageResult<()> {
        self.set_with_source(path, value, None)
    }

    fn set_owned<T: Serialize>(&self, path: Arc<str>, value: &T) -> StorageResult<()> {
        self.set_owned_with_source(path, value, None)
    }

    fn set_with_source<T: Serialize>(
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
        self.inner.check_debouncer();
        let bytes = SERIALIZATION_BUFFER.with(|buf| {
            let mut b = buf.borrow_mut();
            b.clear();
            let mut ser = Serializer::new(&mut *b).with_bytes(BytesMode::ForceAll);
            value.serialize(&mut ser).map_err(CodecError::from)?;

            Ok::<Vec<u8>, RedbStoreError>(Vec::from(&b[..]))
        })?;

        let old_bytes = {
            let lock = self.inner.pending.lock();
            lock.get(&*path).cloned().flatten()
        };

        {
            let mut lock = self.inner.pending.lock();
            lock.insert(path.clone(), Some(bytes.clone()));
        }

        utils::emit_events(
            &self.inner.subscriptions,
            StoreEvent {
                path: path.clone(),
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

    fn scan_prefix(&self, prefix: &str) -> StorageResult<Vec<(String, Vec<u8>)>> {
        let mut results = Vec::new();

        let read_txn = self.inner.db.begin_read().map_err(RedbStoreError::from)?;
        let table = read_txn
            .open_table(TABLE_DATA)
            .map_err(RedbStoreError::from)?;

        let range = prefix..;
        for result in table.range(range).map_err(RedbStoreError::from)? {
            let (k, v) = result.map_err(RedbStoreError::from)?;
            let key_str = k.value();
            if key_str.starts_with(prefix) {
                results.push((key_str.to_string(), Vec::from(&v.value()[..])));
            } else {
                break;
            }
        }

        let mut pending_map = HashMap::new();
        {
            let lock = self.inner.pending.lock();
            for (k, opt_v) in lock.iter() {
                if k.starts_with(prefix) {
                    pending_map.insert(k.to_string(), opt_v.clone());
                }
            }
        }

        for (k, opt_v) in pending_map {
            if let Some(v) = opt_v {
                if let Some(pos) = results.iter().position(|(rk, _)| *rk == k) {
                    results[pos].1 = v;
                } else {
                    results.push((k, v));
                }
            } else {
                results.retain(|(rk, _)| *rk != k);
            }
        }

        results.sort_by(|(a, _), (b, _)| a.cmp(b));
        Ok(results)
    }

    fn delete_with_source(&self, path: &str, source: Option<Uuid>) -> StorageResult<()> {
        self.inner.check_debouncer();
        let path_arc: Arc<str> = Arc::from(path);

        let old_bytes = {
            let lock = self.inner.pending.lock();
            if let Some(p) = lock.get(path) {
                p.clone()
            } else {
                let read_txn = self.inner.db.begin_read().map_err(RedbStoreError::from)?;
                let table = read_txn
                    .open_table(TABLE_DATA)
                    .map_err(RedbStoreError::from)?;
                table
                    .get(path)
                    .map_err(RedbStoreError::from)?
                    .map(|v| Vec::from(&v.value()[..]))
            }
        };

        {
            let mut lock = self.inner.pending.lock();
            lock.insert(path_arc.clone(), None);
        }

        utils::emit_events(
            &self.inner.subscriptions,
            StoreEvent {
                path: path_arc,
                op: StoreOp::Delete,
                old: old_bytes,
                new: None,
                source,
            },
        );

        self.inner.debouncer.schedule();
        Ok(())
    }

    fn delete_prefix_with_source(&self, prefix: &str, source: Option<Uuid>) -> StorageResult<()> {
        self.inner.check_debouncer();

        let keys = self.scan_prefix(prefix)?;

        {
            let mut lock = self.inner.pending.lock();
            for (path, _) in keys {
                lock.insert(Arc::from(path.as_str()), None);
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

    fn delete(&self, path: &str) -> StorageResult<()> {
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

    fn decode<T: DeserializeOwned + Default>(&self, bytes: &[u8]) -> StorageResult<T> {
        match rmp_serde::from_slice(bytes) {
            Ok(val) => Ok(val),
            Err(e) => {
                warn!(
                    target: "amethystate",
                    "Failed to decode field. Data is corrupted or type changed. \
                    Using Default value. Error: {e}"
                );
                Ok(T::default())
            }
        }
    }

    fn flush_prefix(&self, prefix: &str) -> StorageResult<()> {
        self.inner.flush_prefix(prefix)
    }

    fn is_initialized(&self, namespace: &str) -> StorageResult<bool> {
        let key = format!("__init::{namespace}");
        let read_txn = self.inner.db.begin_read().map_err(RedbStoreError::from)?;
        let table = read_txn
            .open_table(TABLE_META)
            .map_err(RedbStoreError::from)?;
        Ok(table
            .get(key.as_str())
            .map_err(RedbStoreError::from)?
            .is_some())
    }

    fn mark_initialized(&self, namespace: &str) -> StorageResult<()> {
        let key = format!("__init::{namespace}");
        let write_txn = self.inner.db.begin_write().map_err(RedbStoreError::from)?;
        {
            let mut table = write_txn
                .open_table(TABLE_META)
                .map_err(RedbStoreError::from)?;
            table
                .insert(key.as_str(), &[][..])
                .map_err(RedbStoreError::from)?;
        }
        write_txn.commit().map_err(RedbStoreError::from)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::migration::fields::FieldDescriptor;
    use crate::migration::{MigrationError, MigrationPlan};
    use amethystate_core::test_utils::unique_path;
    use serial_test::serial;
    use std::thread;
    use std::time::Duration;

    const EMPTY_FIELDS: &[FieldDescriptor] = &[];

    #[test]
    fn test_set_get_immediate() {
        let path = unique_path("immediate");
        let (store, _) = RedbStore::open(StoreConfig::new(path), MigrationSet::default()).unwrap();

        store.set("user.name", &"Alice".to_string()).unwrap();

        let val: Option<String> = store.get("user.name").unwrap();
        assert_eq!(val, Some("Alice".to_string()));
    }

    #[test]
    #[serial]
    fn test_debouncer_persistence() {
        let path = unique_path("debounce");

        let mut config = StoreConfig::new(path);
        config.save_debounce = Duration::from_millis(50);

        let (store, _) = RedbStore::open(config, MigrationSet::default()).unwrap();

        store.set("config.port", &8080u16).unwrap();

        {
            let read_txn = store.inner.db.begin_read().unwrap();
            let table = read_txn.open_table(TABLE_DATA).unwrap();
            assert!(table.get("config.port").unwrap().is_none());
        }

        thread::sleep(Duration::from_millis(500));

        {
            let read_txn = store.inner.db.begin_read().unwrap();
            let table = read_txn.open_table(TABLE_DATA).unwrap();
            assert!(table.get("config.port").unwrap().is_some());
        }
    }

    #[test]
    fn test_local_reactivity() {
        let path = unique_path("reactivity");
        let (store, _) = RedbStore::open(StoreConfig::new(path), MigrationSet::default()).unwrap();

        let hit = Arc::new(Mutex::new(false));
        let hit_inner = hit.clone();

        store.subscribe(
            SubscriptionKind::ExactPath(Arc::from("ui.theme")),
            Arc::new(move |_| {
                let mut guard = hit_inner.lock();
                *guard = true;
            }),
        );

        store.set("ui.theme", &"dark".to_string()).unwrap();

        assert!(*hit.lock());
    }

    #[test]
    fn test_delete_flow() {
        let path = unique_path("delete");
        let (store, _) = RedbStore::open(StoreConfig::new(path), MigrationSet::default()).unwrap();

        store.set("temp.key", &1).unwrap();

        store.save_now().unwrap();
        store.delete("temp.key").unwrap();
        assert_eq!(store.get::<i32>("temp.key").unwrap(), None);

        store.save_now().unwrap();

        let read_txn = store.inner.db.begin_read().unwrap();
        let table = read_txn.open_table(TABLE_DATA).unwrap();
        assert!(table.get("temp.key").unwrap().is_none());
    }

    #[test]
    fn test_smart_recovery_decode() {
        let path = unique_path("recovery");
        let (store, _) = RedbStore::open(StoreConfig::new(path), MigrationSet::default()).unwrap();
        let garbage = vec![0x00, 0x01, 0x02];

        let storage_result: String = store.decode(&garbage).unwrap();
        assert_eq!(storage_result, String::default());
    }

    #[test]
    fn test_deterministic_closure_and_reopen() {
        let path = unique_path("closure");
        {
            let (store, _) =
                RedbStore::open(StoreConfig::new(&path), MigrationSet::default()).unwrap();
            store.set("test.key", &"hello".to_string()).unwrap();
            store.close().expect("Explicit close failed");
        }

        let (store_reopened, _) = RedbStore::open(StoreConfig::new(&path), MigrationSet::default())
            .expect("Database should be available immediately after close");

        let val: Option<String> = store_reopened.get("test.key").unwrap();
        assert_eq!(val, Some("hello".to_string()));
    }

    #[test]
    fn test_drop_behavior_is_deterministic() {
        let path = unique_path("drop_logic");
        {
            let (store, _) =
                RedbStore::open(StoreConfig::new(&path), MigrationSet::default()).unwrap();
            store.set("drop.test", &42u32).unwrap();
        }

        let (store_reopened, _) = RedbStore::open(StoreConfig::new(&path), MigrationSet::default())
            .expect("Drop must release file lock deterministically");

        assert_eq!(store_reopened.get::<u32>("drop.test").unwrap(), Some(42));
    }

    #[test]
    fn test_close_saves_pending_data() {
        let path = unique_path("save_on_close");
        let mut config = StoreConfig::new(&path);
        config.save_debounce = Duration::from_secs(3600);

        {
            let (store, _) = RedbStore::open(config, MigrationSet::default()).unwrap();
            store.set("urgent.data", &true).unwrap();
            store.close().unwrap();
        }

        let (store, _) = RedbStore::open(StoreConfig::new(&path), MigrationSet::default()).unwrap();
        assert_eq!(store.get::<bool>("urgent.data").unwrap(), Some(true));
    }

    #[test]
    fn test_granular_flush_prefix_drains_buffer() {
        let path = unique_path("granular_flush");
        let mut config = StoreConfig::new(&path);

        config.save_debounce = Duration::from_secs(3600);

        let (store, _) = RedbStore::open(config, MigrationSet::default()).unwrap();

        store.set("net.host", &"127.0.0.1".to_string()).unwrap();
        store.set("net.port", &8080u16).unwrap();
        store.set("ui.theme", &"dark".to_string()).unwrap();

        {
            let pending = store.inner.pending.lock();
            assert_eq!(pending.len(), 3);
        }
        {
            let read_txn = store.inner.db.begin_read().unwrap();
            let table = read_txn.open_table(TABLE_DATA).unwrap();
            assert!(table.get("net.host").unwrap().is_none());
            assert!(table.get("ui.theme").unwrap().is_none());
        }

        store.flush_prefix("net").unwrap();

        {
            let read_txn = store.inner.db.begin_read().unwrap();
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

        store.flush_prefix("").unwrap();
        {
            let pending = store.inner.pending.lock();
            assert!(
                pending.is_empty(),
                "Pending buffer should be completely empty"
            );
        }
        {
            let read_txn = store.inner.db.begin_read().unwrap();
            let table = read_txn.open_table(TABLE_DATA).unwrap();
            assert!(
                table.get("ui.theme").unwrap().is_some(),
                "UI should now be persisted on disk"
            );
        }
    }

    #[test]
    fn test_is_initialized_false_on_fresh_store() {
        let path = unique_path("init_fresh");
        let (store, _) = RedbStore::open(StoreConfig::new(path), MigrationSet::default()).unwrap();
        assert!(!store.is_initialized("settings").unwrap());
    }

    #[test]
    fn test_mark_and_is_initialized() {
        let path = unique_path("init_mark");
        let (store, _) = RedbStore::open(StoreConfig::new(path), MigrationSet::default()).unwrap();
        assert!(!store.is_initialized("settings").unwrap());
        store.mark_initialized("settings").unwrap();
        assert!(store.is_initialized("settings").unwrap());
    }

    #[test]
    fn test_initialized_namespaces_are_independent() {
        let path = unique_path("init_namespaces");
        let (store, _) = RedbStore::open(StoreConfig::new(path), MigrationSet::default()).unwrap();
        store.mark_initialized("settings").unwrap();
        assert!(store.is_initialized("settings").unwrap());
        assert!(!store.is_initialized("other").unwrap());
    }

    #[test]
    fn test_component_atomic_rollback() {
        let path = unique_path("rollback");
        let mut cfg = StoreConfig::new(&path);
        cfg.save_debounce = Duration::from_millis(50);
        {
            let (store, _) = RedbStore::open(cfg, MigrationSet::default()).unwrap();
            store.set("net.ip", &"1.1.1.1".to_string()).unwrap();
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
                    Err(MigrationError::Custom("crash".into()).into())
                }),
                0,
                EMPTY_FIELDS,
                &["net"],
            );

        let (store, report) = RedbStore::open(StoreConfig::new(&path), mset).unwrap();
        assert!(report.has_failures());

        let val: String = store.get("net.ip").unwrap().unwrap();
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

        let test_key = "system.critical_update";
        let test_value = "payload_data".to_string();
        store.set(test_key, &test_value).unwrap();

        {
            let pending = store.inner.pending.lock();
            assert!(pending.contains_key(test_key));
        }

        thread::sleep(Duration::from_millis(150));

        SIMULATE_WRITE_FAILURE.store(false, Ordering::Relaxed);

        {
            let pending = store.inner.pending.lock();
            assert!(
                pending.contains_key(test_key),
                "The pending changes buffer should not be cleared when a transaction fails!"
            );
        }

        let retrieved: Option<String> = store.get(test_key).unwrap();
        assert_eq!(retrieved, Some(test_value));
    }
}
