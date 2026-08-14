use crate::migration::set::MigrationSet;
use crate::store::backend::text::json::json_doc::JsonDocument;
use crate::store::backend::text::store::TextStore;
use crate::store::config::StoreConfig;
use crate::store::{Store, StoreCallback, SubscriptionId, SubscriptionKind};
use crate::{MigrationReport, StorageResult};
use serde::Serialize;
use serde::de::DeserializeOwned;
use std::sync::Arc;
use uuid::Uuid;

#[derive(PartialEq, Eq, Clone, Debug)]
pub struct JsonStore(pub TextStore<JsonDocument>);

impl JsonStore {
    pub fn open(
        config: StoreConfig,
        migration_set: MigrationSet,
    ) -> StorageResult<(Self, MigrationReport)> {
        let (store, report) = TextStore::open(config, migration_set)?;
        Ok((JsonStore(store), report))
    }
}

impl Store for JsonStore {
    fn get<T: DeserializeOwned>(&self, path: &str) -> StorageResult<Option<T>> {
        self.0.get(path)
    }
    fn set<T: Serialize>(&self, path: &str, value: &T) -> StorageResult<()> {
        self.0.set(path, value)
    }

    fn set_with_source<T: Serialize>(
        &self,
        path: &str,
        value: &T,
        source: Option<Uuid>,
    ) -> StorageResult<()> {
        self.0.set_with_source(path, value, source)
    }

    fn set_owned_with_source<T: Serialize>(
        &self,
        path: Arc<str>,
        value: &T,
        source: Option<Uuid>,
    ) -> StorageResult<()> {
        self.0.set_owned_with_source(path, value, source)
    }

    fn delete_with_source(&self, path: &str, source: Option<Uuid>) -> StorageResult<()> {
        self.0.delete_with_source(path, source)
    }

    fn delete_prefix_with_source(&self, prefix: &str, source: Option<Uuid>) -> StorageResult<()> {
        self.0.delete_prefix_with_source(prefix, source)
    }
    fn delete(&self, path: &str) -> StorageResult<()> {
        self.0.delete(path)
    }

    fn scan_prefix(&self, prefix: &str) -> StorageResult<Vec<(String, Vec<u8>)>> {
        self.0.scan_prefix(prefix)
    }

    fn scan_keys(&self, prefix: &str) -> StorageResult<Vec<String>> {
        self.0.scan_keys(prefix)
    }

    fn save_now(&self) -> StorageResult<()> {
        self.0.save_now()
    }
    fn subscribe(&self, kind: SubscriptionKind, callback: StoreCallback) -> SubscriptionId {
        self.0.subscribe(kind, callback)
    }
    fn unsubscribe(&self, id: SubscriptionId) {
        self.0.unsubscribe(id)
    }
    fn decode<T: DeserializeOwned + Default>(&self, bytes: &[u8]) -> StorageResult<T> {
        self.0.decode(bytes)
    }
    fn flush_prefix(&self, prefix: &str) -> StorageResult<()> {
        self.0.flush_prefix(prefix)
    }
    fn flush_async(&self) -> crate::store::durable::Commit {
        self.0.flush_async()
    }
    fn is_initialized(&self, namespace: &str) -> StorageResult<bool> {
        self.0.is_initialized(namespace)
    }
    fn mark_initialized(&self, namespace: &str) -> StorageResult<()> {
        self.0.mark_initialized(namespace)
    }
}

crate::define_store_test_suite!(
    JsonStore,
    "json",
    r#"{"amethystate": {"watch_interval_ms": 50}, "ui": {"theme": {"dark": false}}}"#,
    r#"{"amethystate": {"watch_interval_ms": 50}, "ui": {"theme": {"dark": true}}}"#,
    r#"{"amethystate": {"watch_interval_ms": 50}, "ui": {}}"#
);
