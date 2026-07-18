use super::document::TextDocument;
use super::error::TextStoreError;
use crate::MigrationReport;
use crate::codec::CodecError;
use crate::errors::StorageError;
use crate::migration::engine::{MigrationEngine, StorageProvider};
use crate::migration::set::MigrationSet;
use crate::store::StorageResult;
use crate::store::backend::text::migration::TextMigrationBackend;
use crate::store::backend::utils;
use crate::store::config::StoreConfig;
use crate::store::traits::MigrationBackendAdapter;
use crate::store::util::debouncer::Debouncer;
use crate::store::{
    SchemaAwareStore, Store, StoreCallback, StoreEvent, StoreOp, SubscriptionEntry, SubscriptionId,
    SubscriptionKind,
};
use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use parking_lot::RwLock;
use serde::Serialize;
use serde::de::DeserializeOwned;
use std::fmt::Debug;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use tempfile::NamedTempFile;
use tracing::{info, warn};

pub struct StoreFile<D> {
    pub path: PathBuf,
    pub backup_path: PathBuf,
    pub doc: Arc<RwLock<D>>,
}

impl<D> Clone for StoreFile<D> {
    fn clone(&self) -> Self {
        Self {
            path: self.path.clone(),
            backup_path: self.backup_path.clone(),
            doc: self.doc.clone(),
        }
    }
}

impl<D: TextDocument> StoreFile<D> {
    pub fn new(path: PathBuf, initial_doc: D) -> Self {
        let backup_path = path.with_extension("bak");
        Self {
            path,
            backup_path,
            doc: Arc::new(RwLock::new(initial_doc)),
        }
    }

    pub fn create_backup(&self) -> StorageResult<()> {
        if self.path.exists() {
            std::fs::copy(&self.path, &self.backup_path).map_err(TextStoreError::from)?;
        }
        Ok(())
    }

    pub fn load_or_empty(&self) -> StorageResult<D> {
        if self.path.exists() {
            let content = std::fs::read_to_string(&self.path).map_err(TextStoreError::from)?;
            D::parse(&content)
        } else {
            Ok(D::empty())
        }
    }

    pub fn persist(&self) -> StorageResult<()> {
        let content = self.doc.read().serialize()?;
        persist_atomic(&self.path, &content).map_err(TextStoreError::from)?;
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
        self.data.create_backup()?;
        self.meta.create_backup()?;
        Ok(())
    }

    pub fn persist(&self) -> StorageResult<()> {
        self.data.persist()?;
        self.meta.persist()?;
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
    pub(crate) has_pending: Arc<AtomicBool>,
    _watcher: RecommendedWatcher,
}

impl<D: TextDocument> TextStoreInner<D> {
    pub(crate) fn check_debouncer(&self) -> StorageResult<()> {
        if self.debouncer.is_poisoned() {
            panic!("debouncer thread is dead — store integrity cannot be guaranteed");
        }
        Ok(())
    }
}

impl<D: TextDocument> Drop for TextStoreInner<D> {
    fn drop(&mut self) {
        let _ = self.save_now();
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
            data: StoreFile::new(path, D::empty()),
            meta: StoreFile::new(meta_path, D::empty()),
        };

        files.create_backups()?;

        let initial_data = files.data.load_or_empty()?;
        let initial_meta = files.meta.load_or_empty()?;

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
                Err(e)
            }
        }
    }

    fn new(config: StoreConfig, files: StoreFiles<D>) -> StorageResult<Self> {
        info!(
            path = %config.path.display(),
            "initializing TextStore"
        );

        let subscriptions = Arc::new(RwLock::new(Vec::<SubscriptionEntry>::new()));
        let has_pending = Arc::new(AtomicBool::new(false));

        let files_debounce = files.clone();
        let has_pending_debounce = has_pending.clone();
        let debouncer = Debouncer::new(config.save_debounce, move || {
            if let Err(e) = files_debounce.persist() {
                warn!("store persist failed: {e:#}");
            } else {
                has_pending_debounce.store(false, Ordering::Release);
            }
        });

        let files_watch = files.clone();
        let watch_subs = subscriptions.clone();
        let has_pending_watch = has_pending.clone();
        let data_path = files.data.path.clone();
        let meta_path = files.meta.path.clone();

        let watcher = notify::recommended_watcher(move |res: notify::Result<Event>| {
            let Ok(event) = res else { return };

            let is_modify = matches!(event.kind, EventKind::Modify(_) | EventKind::Create(_));
            if !is_modify {
                return;
            }

            if has_pending_watch.load(Ordering::Acquire) {
                return;
            }

            for path in &event.paths {
                if *path == data_path {
                    let Ok(content) = std::fs::read_to_string(path) else {
                        continue;
                    };

                    let Ok(on_disk) = D::parse(&content) else {
                        continue;
                    };

                    let events = {
                        let mut guard = files_watch.data.doc.write();

                        let old_serialized = guard.serialize().unwrap_or_default();
                        let new_serialized = on_disk.serialize().unwrap_or_default();
                        if old_serialized == new_serialized {
                            Vec::new()
                        } else {
                            let old = guard.clone();
                            *guard = on_disk;
                            info!("external store change detected");
                            diff_documents::<D>(&old, &*guard)
                        }
                    };
                    for event in events {
                        utils::emit_events(&watch_subs, event);
                    }
                } else if *path == meta_path {
                    let Ok(content) = std::fs::read_to_string(path) else {
                        continue;
                    };
                    let Ok(on_disk) = D::parse(&content) else {
                        continue;
                    };
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
            }
        })
        .map_err(|e| TextStoreError::Watch(e.to_string()))?;

        let watch_dir = config.path.parent().unwrap_or(Path::new("."));
        let mut watcher = watcher;
        watcher
            .watch(watch_dir, RecursiveMode::NonRecursive)
            .map_err(|e| TextStoreError::Watch(e.to_string()))?;

        let inner = Arc::new(TextStoreInner {
            files,
            subscriptions,
            next_id: Arc::new(AtomicU64::new(1)),
            debouncer: Arc::new(debouncer),
            has_pending,
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
        engine.run(mset)
    }
}

impl<D: TextDocument> TextStoreInner<D> {
    fn get<T: DeserializeOwned>(&self, path: &str) -> StorageResult<Option<T>> {
        let guard = self.files.data.doc.read();
        let parts = split_path(path);
        if let Some(node) = guard.get(&parts) {
            Ok(Some(D::deserialize_node(node)?))
        } else {
            Ok(None)
        }
    }

    fn set<T: Serialize>(
        &self,
        path: &str,
        value: &T,
        source: Option<uuid::Uuid>,
    ) -> StorageResult<()> {
        self.check_debouncer()?;
        let path_str = normalize_path(path)?;
        let node = D::serialize_node(value)?;
        self.set_node(path_str, node, source)
    }

    fn save_now(&self) -> StorageResult<()> {
        self.files.persist()?;
        self.has_pending.store(false, Ordering::Release);
        Ok(())
    }

    fn scan_prefix(&self, prefix: &str) -> StorageResult<Vec<(String, Vec<u8>)>> {
        let guard = self.files.data.doc.read();
        scan_prefix_impl(&*guard, prefix)
    }

    fn delete(&self, path: &str, source: Option<uuid::Uuid>) -> StorageResult<()> {
        self.check_debouncer()?;

        let path_str = normalize_path(path)?;
        let parts = split_path(&path_str);

        let old_bytes = {
            let mut guard = self.files.data.doc.write();
            let old = guard.get(&parts).map(|n| D::node_to_bytes(n)).transpose()?;
            guard.delete(&parts)?;
            old
        };

        self.has_pending.store(true, Ordering::Release);

        utils::emit_events(
            &self.subscriptions,
            StoreEvent {
                path: Arc::from(path_str),
                op: StoreOp::Delete,
                old: old_bytes,
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

    fn decode<T: DeserializeOwned + Default>(&self, bytes: &[u8]) -> StorageResult<T> {
        match D::bytes_to_node(bytes).and_then(|node| D::deserialize_node(&node)) {
            Ok(val) => Ok(val),
            Err(e) => {
                warn!(
                    target: "amethystate",
                    "Failed to decode text field. Data is corrupted or type changed. \
                    Using Default value. Error: {e}"
                );
                Ok(T::default())
            }
        }
    }

    fn is_initialized(&self, namespace: &str) -> StorageResult<bool> {
        let guard = self.files.meta.doc.read();
        let parts = vec!["__init", namespace];
        Ok(guard.get(&parts).is_some())
    }

    fn mark_initialized(&self, namespace: &str) -> StorageResult<()> {
        {
            let mut guard = self.files.meta.doc.write();
            let parts = vec!["__init", namespace];
            let node = D::serialize_node(&true)?;
            guard.set(&parts, node)?;
        }

        self.files.meta.persist()?;
        Ok(())
    }

    pub(crate) fn set_node(
        &self,
        path_str: String,
        node: D::Node,
        source: Option<uuid::Uuid>,
    ) -> StorageResult<()> {
        let parts = split_path(&path_str);
        let (old_bytes, new_bytes) = {
            let mut guard = self.files.data.doc.write();
            let old = guard.get(&parts).map(|n| D::node_to_bytes(n)).transpose()?;
            guard.set(&parts, node)?;
            let new_node = guard.get(&parts).unwrap();
            let new_bytes = D::node_to_bytes(new_node)?;
            (old, new_bytes)
        };

        self.has_pending.store(true, Ordering::Release);

        utils::emit_events(
            &self.subscriptions,
            StoreEvent {
                path: Arc::from(path_str),
                op: StoreOp::Set,
                old: old_bytes,
                new: Some(new_bytes),
                source,
            },
        );

        self.debouncer.schedule();
        Ok(())
    }
}

impl<D: TextDocument + Send + 'static> Store for TextStore<D> {
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
        self.set_with_source(&path, value, source)
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

    fn subscribe(&self, kind: SubscriptionKind, callback: StoreCallback) -> SubscriptionId {
        self.inner.subscribe(kind, callback)
    }

    fn unsubscribe(&self, id: SubscriptionId) {
        self.inner.unsubscribe(id)
    }

    fn decode<T: DeserializeOwned + Default>(&self, bytes: &[u8]) -> StorageResult<T> {
        self.inner.decode(bytes)
    }

    fn flush_prefix(&self, _prefix: &str) -> StorageResult<()> {
        self.save_now()
    }

    fn is_initialized(&self, namespace: &str) -> StorageResult<bool> {
        self.inner.is_initialized(namespace)
    }

    fn mark_initialized(&self, namespace: &str) -> StorageResult<()> {
        self.inner.mark_initialized(namespace)
    }
}

pub fn normalize_path(path: &str) -> StorageResult<String> {
    let trimmed = path.trim();

    if trimmed == "." {
        return Ok(".".to_string());
    }

    let normalized = path
        .split('.')
        .filter(|s| !s.trim().is_empty())
        .collect::<Vec<_>>()
        .join(".");

    if normalized.is_empty() {
        return Err(StorageError::TextStore(TextStoreError::Codec(
            CodecError::Custom("path cannot be empty".into()),
        )));
    }
    Ok(normalized)
}

pub fn split_path(path: &str) -> Vec<&str> {
    if path == "." {
        return vec!["."];
    }
    if path.is_empty() {
        return vec![];
    }
    path.split('.').filter(|s| !s.is_empty()).collect()
}

fn persist_atomic(path: &Path, content: &str) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let dir = path.parent().unwrap_or(Path::new("."));

    let mut attempts = 5;
    loop {
        match NamedTempFile::new_in(dir) {
            Ok(mut tmp) => {
                if let Err(e) = tmp.write_all(content.as_bytes()) {
                    attempts -= 1;
                    if attempts == 0 {
                        return Err(e);
                    }
                    std::thread::sleep(std::time::Duration::from_millis(15));
                    continue;
                }

                match tmp.persist(path) {
                    Ok(_) => return Ok(()),
                    Err(e) => {
                        attempts -= 1;
                        if attempts == 0 {
                            return Err(e.error);
                        }
                        std::thread::sleep(std::time::Duration::from_millis(15));
                    }
                }
            }
            Err(e) => {
                attempts -= 1;
                if attempts == 0 {
                    return Err(e);
                }
                std::thread::sleep(std::time::Duration::from_millis(15));
            }
        }
    }
}

pub(super) fn scan_prefix_impl<D: TextDocument>(
    doc: &D,
    prefix: &str,
) -> StorageResult<Vec<(String, Vec<u8>)>> {
    let parts = split_path(prefix);
    let target_depth = parts.len() + 1;
    let mut raw_nodes = Vec::new();
    scan_prefix_recursive(doc, &parts, prefix, &mut raw_nodes, Some(target_depth));

    let mut results = Vec::new();
    for (k, node) in raw_nodes {
        if k.starts_with(prefix) {
            let bytes = D::node_to_bytes(&node)?;
            results.push((k, bytes));
        }
    }

    Ok(results)
}

pub(super) fn scan_prefix_recursive<D: TextDocument>(
    doc: &D,
    parts: &[&str],
    prefix_str: &str,
    results: &mut Vec<(String, D::Node)>,
    target_depth: Option<usize>,
) {
    let current_depth = parts.len();

    if let Some(target_depth) = target_depth
        && current_depth >= target_depth
    {
        if !prefix_str.is_empty()
            && !prefix_str.ends_with('.')
            && let Some(node) = doc.get(parts)
        {
            results.push((prefix_str.to_string(), node.clone()));
        }
        return;
    }

    let children = doc.scan(parts);
    if children.is_empty() {
        if !prefix_str.is_empty()
            && !prefix_str.ends_with('.')
            && let Some(node) = doc.get(parts)
        {
            results.push((prefix_str.to_string(), node.clone()));
        }
    } else {
        for (full_key, _node) in children {
            let child_parts = split_path(&full_key);
            let grand_children = doc.scan(&child_parts);

            let should_stop = grand_children.is_empty()
                || target_depth.is_some_and(|depth| child_parts.len() >= depth);

            if should_stop {
                if let Some(child_node) = doc.get(&child_parts) {
                    results.push((full_key, child_node.clone()));
                }
            } else {
                scan_prefix_recursive(doc, &child_parts, prefix_str, results, target_depth);
            }
        }
    }
}

fn diff_documents<D: TextDocument>(old: &D, new: &D) -> Vec<StoreEvent> {
    let mut old_nodes = Vec::new();
    scan_prefix_recursive(old, &[], "", &mut old_nodes, None);
    let old_map: std::collections::HashMap<String, D::Node> = old_nodes.into_iter().collect();

    let mut new_nodes = Vec::new();
    scan_prefix_recursive(new, &[], "", &mut new_nodes, None);
    let new_map: std::collections::HashMap<String, D::Node> = new_nodes.into_iter().collect();

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
    events
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    use proptest::string::string_regex;

    fn dirty_path_strategy() -> impl Strategy<Value = String> {
        string_regex("[a-zA-Z0-9_.-]{0,20}").unwrap()
    }

    proptest! {
        #[test]
        fn prop_normalize_path_is_clean(dirty_path in dirty_path_strategy()) {
            match normalize_path(&dirty_path) {
                Ok(norm) => {
                    assert!(!norm.is_empty(), "Normalized path cannot be empty");
                    if norm != "." {
                        assert!(!norm.starts_with('.'), "Should not start with a dot: {}", norm);
                        assert!(!norm.ends_with('.'), "Should not end with a dot: {}", norm);
                    }
                    assert!(!norm.contains(".."), "Should not contain double dots: {}", norm);
                }
                Err(_) => {
                    let only_dots = dirty_path.chars().all(|c| c == '.');
                    assert!(only_dots || dirty_path.trim().is_empty());
                }
            }
        }
    }
}
