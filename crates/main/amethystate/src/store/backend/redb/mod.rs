use crate::store::{
    SchemaAwareStore, StoreBackend, StoreCallback, StoreEvent, StoreOp, SubscriptionEntry,
    SubscriptionId, SubscriptionKind,
};
use amethystate_core::path::StorePath;
use error_stack::ResultExt;
use migration::RedbMigrationBackend;
use redb::{Database, ReadableDatabase, TableHandle};
use std::collections::{HashMap, HashSet};
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
    path: Arc<Path>,
    pending: Arc<Mutex<utils::Pending>>,
    initialized: Arc<Mutex<HashSet<Arc<str>>>>,
    commits: Arc<crate::store::durable::CommitSignal>,
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

    pub fn flush_prefix(&self, prefix: &StorePath) -> StorageResult<()> {
        let _write_guard = self.write_lock.lock();

        let changes = {
            let lock = self.pending.lock();
            utils::pending_prefix(&lock, prefix.as_str())
        };
        let txn = self
            .db
            .begin_write()
            .change_context(StorageError::Flush)
            .attach_with(|| format!("store: {}", self.path.display()))
            .attach_with(|| format!("prefix: {prefix}"))
            .attach_with(|| format!("buffered entries: {}", changes.len()))?;
        {
            let mut table = txn
                .open_table(TABLE_DATA)
                .change_context(StorageError::Flush)
                .attach_with(|| format!("store: {}", self.path.display()))
                .attach_with(|| format!("table: {}", TABLE_DATA.name()))?;
            let mut meta = txn
                .open_table(TABLE_META)
                .change_context(StorageError::Flush)
                .attach_with(|| format!("store: {}", self.path.display()))
                .attach_with(|| format!("table: {}", TABLE_META.name()))?;
            for (path, op) in &changes {
                match op {
                    utils::PendingOp::Set(b) => {
                        table
                            .insert(&**path, &b[..])
                            .change_context(StorageError::Flush)
                            .attach_with(|| format!("store: {}", self.path.display()))
                            .attach_with(|| format!("table: {}", TABLE_DATA.name()))
                            .attach_with(|| format!("key: {path}"))
                            .attach_with(|| format!("value: {} bytes", b.len()))?;
                    }
                    utils::PendingOp::Delete => {
                        table
                            .remove(&**path)
                            .change_context(StorageError::Flush)
                            .attach_with(|| format!("store: {}", self.path.display()))
                            .attach_with(|| format!("table: {}", TABLE_DATA.name()))
                            .attach_with(|| format!("key: {path}"))?;
                    }
                    utils::PendingOp::MarkInit => {
                        meta.insert(format!("__init::{path}").as_str(), &[][..])
                            .change_context(StorageError::Flush)
                            .attach_with(|| format!("store: {}", self.path.display()))
                            .attach_with(|| format!("table: {}", TABLE_META.name()))
                            .attach_with(|| format!("namespace: {path}"))?;
                    }
                }
            }
        }
        txn.commit()
            .change_context(StorageError::Flush)
            .attach_with(|| format!("store: {}", self.path.display()))
            .attach_with(|| format!("prefix: {prefix}"))
            .attach_with(|| format!("buffered entries: {}", changes.len()))?;

        utils::clear_committed(&mut self.pending.lock(), &changes);
        self.commits.finished(true);
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

        let db = Arc::new(
            Database::create(&config.path)
                .change_context(StorageError::Open)
                .attach_with(|| format!("store: {}", path.display()))?,
        );

        let write_txn = db
            .begin_write()
            .change_context(StorageError::Open)
            .attach_with(|| format!("store: {}", path.display()))?;
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
                    .change_context(StorageError::Open)
                    .attach_with(|| format!("store: {}", path.display()))
                    .attach_with(|| format!("table: {}", table.name()))?;
            }
        }
        write_txn
            .commit()
            .change_context(StorageError::Open)
            .attach_with(|| format!("store: {}", path.display()))?;

        let pending = Arc::new(Mutex::new(utils::Pending::new()));
        let initialized = Arc::new(Mutex::new(HashSet::<Arc<str>>::new()));
        let commits = Arc::new(crate::store::durable::CommitSignal::default());
        let subscriptions = Arc::new(RwLock::new(Vec::new()));

        let db_save = db.clone();
        let pending_save = pending.clone();
        let commits_save = commits.clone();

        let write_lock = Arc::new(Mutex::new(()));
        let write_lock_save = write_lock.clone();

        let debouncer = Debouncer::new(config.save_debounce, move || {
            let _write_guard = write_lock_save.lock();

            let changes = {
                let lock = pending_save.lock();
                lock.clone()
            };
            if changes.is_empty() {
                return;
            }

            let success = (|| -> Option<bool> {
                #[cfg(test)]
                if SIMULATE_WRITE_FAILURE.load(Ordering::Relaxed) {
                    return None;
                }

                let txn = db_save.begin_write().ok()?;
                {
                    let mut table = txn.open_table(TABLE_DATA).ok()?;
                    let mut meta = txn.open_table(TABLE_META).ok()?;
                    for (path, op) in &changes {
                        match op {
                            utils::PendingOp::Set(b) => {
                                table.insert(&**path, &b[..]).ok()?;
                            }
                            utils::PendingOp::Delete => {
                                table.remove(&**path).ok()?;
                            }
                            utils::PendingOp::MarkInit => {
                                meta.insert(format!("__init::{path}").as_str(), &[][..])
                                    .ok()?;
                            }
                        }
                    }
                }
                txn.commit().ok()?;
                Some(true)
            })()
            .unwrap_or(false);

            if success {
                utils::clear_committed(&mut pending_save.lock(), &changes);
            }
            commits_save.finished(success);
        });

        let inner = Arc::new(RedbStoreInner {
            db,
            path,
            pending,
            initialized,
            commits,
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
            .db
            .begin_read()
            .change_context(StorageError::Read)
            .attach_with(|| format!("store: {}", self.inner.path.display()))
            .attach_with(|| format!("key: {path}"))?;
        let table = read_txn
            .open_table(TABLE_DATA)
            .change_context(StorageError::Read)
            .attach_with(|| format!("store: {}", self.inner.path.display()))
            .attach_with(|| format!("table: {}", TABLE_DATA.name()))?;

        Ok(table
            .get(path)
            .change_context(StorageError::Read)
            .attach_with(|| format!("store: {}", self.inner.path.display()))
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
                    .change_context(StorageError::Migrate)
                    .attach_with(|| format!("store: {}", self.path.display()))?;

                let res = {
                    let mut storage = RedbMigrationBackend::new(&write_txn, self.path);
                    f(&mut storage)?
                };

                write_txn
                    .commit()
                    .change_context(StorageError::Migrate)
                    .attach_with(|| format!("store: {}", self.path.display()))?;
                Ok(res)
            }
        }

        let provider = RedbProvider {
            db: &self.inner.db,
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
            .db
            .begin_read()
            .change_context(StorageError::Read)
            .attach_with(|| format!("store: {}", self.inner.path.display()))
            .attach_with(|| format!("key: {path}"))?;
        let table = read_txn
            .open_table(TABLE_DATA)
            .change_context(StorageError::Read)
            .attach_with(|| format!("store: {}", self.inner.path.display()))
            .attach_with(|| format!("table: {}", TABLE_DATA.name()))?;
        match table
            .get(path)
            .change_context(StorageError::Read)
            .attach_with(|| format!("store: {}", self.inner.path.display()))
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
                            .attach_with(|| format!("key: {path} (unflushed)"))?;
                        Ok(true)
                    }
                    None => Ok(false),
                };
            }
        }

        let read_txn = self
            .inner
            .db
            .begin_read()
            .change_context(StorageError::Read)
            .attach_with(|| format!("store: {}", self.inner.path.display()))
            .attach_with(|| format!("key: {path}"))?;
        let table = read_txn
            .open_table(TABLE_DATA)
            .change_context(StorageError::Read)
            .attach_with(|| format!("store: {}", self.inner.path.display()))
            .attach_with(|| format!("table: {}", TABLE_DATA.name()))?;
        match table
            .get(path)
            .change_context(StorageError::Read)
            .attach_with(|| format!("store: {}", self.inner.path.display()))
            .attach_with(|| format!("key: {path}"))?
        {
            Some(access_guard) => {
                self.decode_erased(access_guard.value(), f)
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
        self.inner.check_debouncer();
        let bytes = SERIALIZATION_BUFFER
            .with(|buf| {
                let mut b = buf.borrow_mut();
                b.clear();
                let mut ser = Serializer::new(&mut *b).with_bytes(BytesMode::ForceAll);
                erased_serde::serialize(value, &mut ser).map_err(CodecError::from)?;

                Ok::<Vec<u8>, CodecError>(Vec::from(&b[..]))
            })
            .change_context(StorageError::Codec)
            .attach_with(|| format!("store: {}", self.inner.path.display()))
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

    fn scan_prefix(&self, prefix: &StorePath) -> StorageResult<Vec<(String, Vec<u8>)>> {
        let bound = utils::subtree_bound(prefix);
        let prefix = prefix.as_str();
        let mut results = Vec::new();

        let read_txn = self
            .inner
            .db
            .begin_read()
            .change_context(StorageError::Scan)
            .attach_with(|| format!("store: {}", self.inner.path.display()))
            .attach_with(|| format!("prefix: {prefix}"))?;
        let table = read_txn
            .open_table(TABLE_DATA)
            .change_context(StorageError::Scan)
            .attach_with(|| format!("store: {}", self.inner.path.display()))
            .attach_with(|| format!("table: {}", TABLE_DATA.name()))?;

        let range = prefix..;
        let entries = table
            .range(range)
            .change_context(StorageError::Scan)
            .attach_with(|| format!("store: {}", self.inner.path.display()))
            .attach_with(|| format!("prefix: {prefix}"))?;
        for result in entries {
            let (k, v) = result
                .change_context(StorageError::Scan)
                .attach_with(|| format!("store: {}", self.inner.path.display()))
                .attach_with(|| format!("prefix: {prefix}"))
                .attach_with(|| format!("entries read so far: {}", results.len()))?;
            let key_str = k.value();
            if utils::is_under(key_str, prefix, &bound) {
                results.push((key_str.to_string(), Vec::from(&v.value()[..])));
            } else if !key_str.starts_with(prefix) {
                break;
            }
        }

        let mut pending_map = HashMap::new();
        {
            let lock = self.inner.pending.lock();
            for (k, op) in lock.iter().filter(|(_, o)| o.is_data()) {
                if utils::is_under(k, prefix, &bound) {
                    pending_map.insert(k.to_string(), op.value().map(Vec::from));
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

    fn scan_keys(&self, prefix: &StorePath) -> StorageResult<Vec<String>> {
        let bound = utils::subtree_bound(prefix);
        let prefix = prefix.as_str();
        let mut keys = Vec::new();

        let read_txn = self
            .inner
            .db
            .begin_read()
            .change_context(StorageError::Scan)
            .attach_with(|| format!("store: {}", self.inner.path.display()))
            .attach_with(|| format!("prefix: {prefix}"))?;
        let table = read_txn
            .open_table(TABLE_DATA)
            .change_context(StorageError::Scan)
            .attach_with(|| format!("store: {}", self.inner.path.display()))
            .attach_with(|| format!("table: {}", TABLE_DATA.name()))?;

        let entries = table
            .range(prefix..)
            .change_context(StorageError::Scan)
            .attach_with(|| format!("store: {}", self.inner.path.display()))
            .attach_with(|| format!("prefix: {prefix}"))?;
        for result in entries {
            let (k, _) = result
                .change_context(StorageError::Scan)
                .attach_with(|| format!("store: {}", self.inner.path.display()))
                .attach_with(|| format!("prefix: {prefix}"))
                .attach_with(|| format!("keys read so far: {}", keys.len()))?;
            let key = k.value();
            if !utils::is_under(key, prefix, &bound) {
                if !key.starts_with(prefix) {
                    break;
                }
                continue;
            }
            keys.push(key.to_string());
        }

        {
            let lock = self.inner.pending.lock();
            for (k, op) in lock.iter().filter(|(_, o)| o.is_data()) {
                if !utils::is_under(k, prefix, &bound) {
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

    fn delete_with_source(&self, path: &StorePath, source: Option<Uuid>) -> StorageResult<()> {
        self.inner.check_debouncer();
        let path_arc: Arc<str> = Arc::from(path.as_str());

        let old_bytes = self
            .committed_or_buffered(path)
            .change_context(StorageError::Delete)
            .attach_with(|| format!("key: {path}"))
            .attach("reading the value a subscriber should see as the old one")?;

        {
            let mut lock = self.inner.pending.lock();
            lock.insert(path_arc.clone(), utils::PendingOp::Delete);
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

    fn delete_prefix_with_source(
        &self,
        prefix: &StorePath,
        source: Option<Uuid>,
    ) -> StorageResult<()> {
        self.inner.check_debouncer();

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

        let key = format!("__init::{namespace}");
        let read_txn = self
            .inner
            .db
            .begin_read()
            .change_context(StorageError::Meta)
            .attach_with(|| format!("store: {}", self.inner.path.display()))
            .attach_with(|| format!("namespace: {namespace}"))?;
        let table = read_txn
            .open_table(TABLE_META)
            .change_context(StorageError::Meta)
            .attach_with(|| format!("store: {}", self.inner.path.display()))
            .attach_with(|| format!("table: {}", TABLE_META.name()))?;
        let found = table
            .get(key.as_str())
            .change_context(StorageError::Meta)
            .attach_with(|| format!("store: {}", self.inner.path.display()))
            .attach_with(|| format!("key: {key}"))?
            .is_some();

        if found {
            self.inner.initialized.lock().insert(Arc::from(namespace));
        }
        Ok(found)
    }

    fn mark_initialized(&self, namespace: &str) -> StorageResult<()> {
        if self.inner.initialized.lock().contains(namespace) {
            return Ok(());
        }

        self.inner.check_debouncer();
        let key: Arc<str> = Arc::from(namespace);
        self.inner
            .pending
            .lock()
            .insert(Arc::clone(&key), utils::PendingOp::MarkInit);
        self.inner.initialized.lock().insert(key);
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
    use amethystate_core::test_utils::unique_path;
    use serial_test::serial;
    use std::thread;
    use std::time::Duration;

    const EMPTY_FIELDS: &[FieldDescriptor] = &[];

    #[test]
    fn test_set_get_immediate() {
        let path = unique_path("immediate");
        let (store, _) = RedbStore::open(StoreConfig::new(path), MigrationSet::default()).unwrap();

        store.set(["user", "name"], &"Alice".to_string()).unwrap();

        let val: Option<String> = store.get(["user", "name"]).unwrap();
        assert_eq!(val, Some("Alice".to_string()));
    }

    #[test]
    #[serial]
    fn test_debouncer_persistence() {
        let path = unique_path("debounce");

        let mut config = StoreConfig::new(path);
        config.save_debounce = Duration::from_millis(50);

        let (store, _) = RedbStore::open(config, MigrationSet::default()).unwrap();

        store.set(["config", "port"], &8080u16).unwrap();

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

        store.set(["ui", "theme"], &"dark".to_string()).unwrap();

        assert!(*hit.lock());
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

        let read_txn = store.inner.db.begin_read().unwrap();
        let table = read_txn.open_table(TABLE_DATA).unwrap();
        assert!(table.get("temp.key").unwrap().is_none());
    }

    #[test]
    fn bytes_that_are_not_the_type_are_an_error_rather_than_a_default() {
        let path = unique_path("recovery");
        let (store, _) = RedbStore::open(StoreConfig::new(path), MigrationSet::default()).unwrap();
        let garbage = vec![0x00, 0x01, 0x02];

        let err = store.decode::<String>(&garbage).unwrap_err();

        assert_eq!(err.current_context(), &StorageError::Codec);
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
            let read_txn = store.inner.db.begin_read().unwrap();
            let table = read_txn.open_table(TABLE_DATA).unwrap();
            assert!(table.get("net.host").unwrap().is_none());
            assert!(table.get("ui.theme").unwrap().is_none());
        }

        store
            .flush_prefix(&StorePath::from_segments(["net"]))
            .unwrap();

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

        store.flush_prefix(&StorePath::root()).unwrap();
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
}
