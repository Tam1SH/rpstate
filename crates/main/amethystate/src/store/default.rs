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

/// A handle on an open store.
///
/// One type over every engine - which one is behind it is settled by
/// [`StoreBuilder`](crate::StoreBuilder) when the store is opened, and nothing
/// downstream is generic over it.
///
/// Cheap to clone and shared by every clone: the file stays open as long as one
/// handle is alive, and closes when the last is dropped.
#[derive(Clone)]
pub struct Store(Arc<dyn StoreBackend>);

impl Store {
    /// Wraps a backend that was built by hand.
    ///
    /// [`StoreBuilder`](crate::StoreBuilder) is the ordinary way in; this is
    /// for a [`StoreBackend`] implemented outside the crate.
    pub fn from_arc(inner: Arc<dyn StoreBackend>) -> Self {
        Self(inner)
    }

    /// The erased backend underneath, for code that is generic over
    /// [`StoreBackend`] rather than over this handle.
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
    /// Reads a value by path, or `None` if nothing is stored there.
    ///
    /// Sees buffered writes as well as committed ones. The type is whatever
    /// you ask for and nothing remembers it - [`Kv::cell`](crate::store::Kv::cell)
    /// is the variant that does.
    ///
    /// ```
    /// # use amethystate::StoreBuilder;
    /// # let path = amethystate_core::test_utils::TempPath::new("doc");
    /// # let store = StoreBuilder::new(&*path).build().unwrap();
    /// store.set("ui.width", &1280u32).unwrap();
    /// assert_eq!(store.get::<u32>("ui.width").unwrap(), Some(1280));
    /// assert_eq!(store.get::<u32>("ui.height").unwrap(), None);
    /// ```
    pub fn get<T: serde::de::DeserializeOwned>(&self, path: &str) -> StorageResult<Option<T>> {
        crate::store::StoreExt::get(self, path)
    }
    /// Writes a value at `path`, creating it or replacing what was there.
    ///
    /// The write lands in the buffer and notifies subscribers; the debouncer
    /// commits it later. Nothing here carries provenance, so a subscription
    /// cannot tell this apart from its own write - use
    /// [`Store::set_with_source`] when it must.
    pub fn set<T: serde::Serialize>(&self, path: &str, value: &T) -> StorageResult<()> {
        crate::store::StoreExt::set(self, path, value)
    }
    /// [`Store::set`] for a path that is already an `Arc<str>`, saving the
    /// copy the borrowed form would make.
    pub fn set_owned<T: serde::Serialize>(&self, path: Arc<str>, value: &T) -> StorageResult<()> {
        crate::store::StoreExt::set_owned(self, path, value)
    }
    /// [`Store::set`] tagged with who made the write.
    ///
    /// Subscribers receive the id, which is how a component ignores the echo
    /// of its own change instead of reacting to it.
    pub fn set_with_source<T: serde::Serialize>(
        &self,
        path: &str,
        value: &T,
        source: Option<uuid::Uuid>,
    ) -> StorageResult<()> {
        crate::store::StoreExt::set_with_source(self, path, value, source)
    }
    /// [`Store::set_with_source`] for a path that is already an `Arc<str>`.
    pub fn set_owned_with_source<T: serde::Serialize>(
        &self,
        path: Arc<str>,
        value: &T,
        source: Option<uuid::Uuid>,
    ) -> StorageResult<()> {
        crate::store::StoreExt::set_owned_with_source(self, path, value, source)
    }
    /// Decodes bytes that arrived in a [`StoreEvent`](crate::StoreEvent),
    /// in whatever format this backend writes.
    ///
    /// Corruption is not an error here: bytes that will not decode - because
    /// the file was edited by hand, or the field changed type - yield
    /// `T::default()` with a warning, which is what the next startup would
    /// read anyway.
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
