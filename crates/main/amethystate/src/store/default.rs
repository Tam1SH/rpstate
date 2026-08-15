use crate::store::backend;

#[cfg(feature = "json")]
pub use backend::text::JsonStore;

#[cfg(feature = "sqlite")]
pub use backend::sqlite::SqliteStore;

#[cfg(feature = "redb")]
pub use backend::redb::RedbStore;

#[cfg(feature = "toml")]
pub use backend::text::TomlStore;

#[cfg(feature = "ron")]
pub use backend::text::RonStore;

use crate::MigrationReport;
use crate::store::config::StoreConfig;
use crate::store::{StorageResult, StoreBackend};
use std::sync::Arc;

/// The store as everything downstream sees it: one type, whatever the enabled
/// backends. The engine is chosen when the store is built, not by which
/// features happened to unify across the dependency graph.
#[derive(Clone)]
pub struct Store(Arc<dyn StoreBackend>);

impl Store {
    pub fn from_arc(inner: Arc<dyn StoreBackend>) -> Self {
        Self(inner)
    }

    pub fn as_dyn(&self) -> &Arc<dyn StoreBackend> {
        &self.0
    }

    /// Opens the store with [`crate::store::builder::default_backend`].
    pub fn open(
        config: StoreConfig,
        mset: crate::migration::set::MigrationSet,
    ) -> StorageResult<(Self, MigrationReport)> {
        crate::store::builder::default_backend().open_public(config, mset)
    }
}

impl std::fmt::Debug for Store {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Store")
    }
}

impl PartialEq for Store {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}
impl Eq for Store {}

impl std::ops::Deref for Store {
    type Target = dyn StoreBackend;
    fn deref(&self) -> &Self::Target {
        &*self.0
    }
}

/// The typed surface, inherent so a call site needs no trait in scope.
impl Store {
    pub fn get<T: serde::de::DeserializeOwned>(&self, path: &str) -> StorageResult<Option<T>> {
        crate::store::StoreExt::get(self, path)
    }
    pub fn set<T: serde::Serialize>(&self, path: &str, value: &T) -> StorageResult<()> {
        crate::store::StoreExt::set(self, path, value)
    }
    pub fn set_owned<T: serde::Serialize>(&self, path: Arc<str>, value: &T) -> StorageResult<()> {
        crate::store::StoreExt::set_owned(self, path, value)
    }
    pub fn set_with_source<T: serde::Serialize>(
        &self,
        path: &str,
        value: &T,
        source: Option<uuid::Uuid>,
    ) -> StorageResult<()> {
        crate::store::StoreExt::set_with_source(self, path, value, source)
    }
    pub fn set_owned_with_source<T: serde::Serialize>(
        &self,
        path: Arc<str>,
        value: &T,
        source: Option<uuid::Uuid>,
    ) -> StorageResult<()> {
        crate::store::StoreExt::set_owned_with_source(self, path, value, source)
    }
    pub fn decode<T: serde::de::DeserializeOwned + Default>(
        &self,
        bytes: &[u8],
    ) -> StorageResult<T> {
        crate::store::StoreExt::decode(self, bytes)
    }
}

impl StoreBackend for Store {
    fn get_raw(&self, path: &str) -> StorageResult<Option<Vec<u8>>> {
        self.0.get_raw(path)
    }
    fn set_erased(
        &self,
        path: &str,
        value: &dyn erased_serde::Serialize,
        source: Option<uuid::Uuid>,
    ) -> StorageResult<()> {
        self.0.set_erased(path, value, source)
    }
    fn set_owned_erased(
        &self,
        path: Arc<str>,
        value: &dyn erased_serde::Serialize,
        source: Option<uuid::Uuid>,
    ) -> StorageResult<()> {
        self.0.set_owned_erased(path, value, source)
    }
    fn get_erased(
        &self,
        path: &str,
        f: &mut dyn FnMut(&mut dyn erased_serde::Deserializer) -> StorageResult<()>,
    ) -> StorageResult<bool> {
        self.0.get_erased(path, f)
    }
    fn decode_erased(
        &self,
        bytes: &[u8],
        f: &mut dyn FnMut(&mut dyn erased_serde::Deserializer) -> StorageResult<()>,
    ) -> StorageResult<()> {
        self.0.decode_erased(bytes, f)
    }
    fn delete_with_source(&self, path: &str, source: Option<uuid::Uuid>) -> StorageResult<()> {
        self.0.delete_with_source(path, source)
    }
    fn delete(&self, path: &str) -> StorageResult<()> {
        self.0.delete(path)
    }
    fn delete_prefix_with_source(
        &self,
        prefix: &str,
        source: Option<uuid::Uuid>,
    ) -> StorageResult<()> {
        self.0.delete_prefix_with_source(prefix, source)
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
    fn subscribe(
        &self,
        kind: crate::SubscriptionKind,
        callback: crate::store::StoreCallback,
    ) -> crate::store::SubscriptionId {
        self.0.subscribe(kind, callback)
    }
    fn unsubscribe(&self, id: crate::store::SubscriptionId) {
        self.0.unsubscribe(id)
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
