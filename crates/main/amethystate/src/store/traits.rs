use crate::migration::AppliedStep;
use crate::migration::set::MigrationSet;
use crate::store::error::StorageResult;
use crate::store::meta::{PrefixMeta, SchemaSnapshot};
use crate::store::{CodecFormat, StoreCallback, SubscriptionId};
use crate::{MigrationReport, SubscriptionKind};
use serde::{Serialize, de::DeserializeOwned};
use std::sync::Arc;
use uuid::Uuid;

pub trait MigrationBackendAdapter {
    fn format(&self) -> CodecFormat;

    fn get(&self, key: &str) -> StorageResult<Option<Vec<u8>>>;
    fn set(&mut self, key: &str, value: &[u8]) -> StorageResult<()>;
    fn delete(&mut self, key: &str) -> StorageResult<()>;
    fn scan_prefix(&self, prefix: &str) -> StorageResult<Vec<(String, Vec<u8>)>>;

    fn get_meta(&self, prefix: &str) -> StorageResult<Option<PrefixMeta>>;
    fn set_meta(&mut self, prefix: &str, meta: &PrefixMeta) -> StorageResult<()>;
    fn get_schema_snapshot(&self, prefix: &str) -> StorageResult<Option<SchemaSnapshot>>;
    fn set_schema_snapshot(&mut self, prefix: &str, snapshot: &SchemaSnapshot)
    -> StorageResult<()>;
    fn get_migration_log(&self, prefix: &str) -> StorageResult<Option<Vec<AppliedStep>>>;
    fn set_migration_log(&mut self, prefix: &str, log: &[AppliedStep]) -> StorageResult<()>;
}

pub trait SchemaAwareStore: Store {
    fn run_migrations(&self, mset: MigrationSet) -> StorageResult<MigrationReport>;
}

pub trait Store: Eq + Clone + Sized + Send + Sync + 'static {
    fn get<T: DeserializeOwned>(&self, path: &str) -> StorageResult<Option<T>>;

    fn set<T: Serialize>(&self, path: &str, value: &T) -> StorageResult<()>;
    fn set_owned<T: Serialize>(&self, path: Arc<str>, value: &T) -> StorageResult<()> {
        self.set(&path, value)
    }
    fn set_with_source<T: Serialize>(
        &self,
        path: &str,
        value: &T,
        source: Option<Uuid>,
    ) -> StorageResult<()>;
    fn set_owned_with_source<T: Serialize>(
        &self,
        path: Arc<str>,
        value: &T,
        source: Option<Uuid>,
    ) -> StorageResult<()>;

    fn delete_with_source(&self, path: &str, source: Option<Uuid>) -> StorageResult<()>;
    fn delete(&self, path: &str) -> StorageResult<()>;

    /// Removes every key under `prefix`, emitting one
    /// [`StoreOp::DeletePrefix`] instead of a `Delete` per key.
    fn delete_prefix_with_source(&self, prefix: &str, source: Option<Uuid>) -> StorageResult<()>;

    fn delete_prefix(&self, prefix: &str) -> StorageResult<()> {
        self.delete_prefix_with_source(prefix, None)
    }

    /// Every key under `prefix`, sorted by key on every backend.
    fn scan_prefix(&self, prefix: &str) -> StorageResult<Vec<(String, Vec<u8>)>>;

    /// The keys under `prefix`, sorted, without reading their values.
    ///
    /// `scan_prefix` copies every value out of the backend, which is wasted
    /// work when only the keys are wanted - and grows with the data rather
    /// than with the answer.
    fn scan_keys(&self, prefix: &str) -> StorageResult<Vec<String>>;

    fn save_now(&self) -> StorageResult<()>;

    fn subscribe(&self, kind: SubscriptionKind, callback: StoreCallback) -> SubscriptionId;
    fn unsubscribe(&self, id: SubscriptionId);

    fn decode<T: DeserializeOwned + Default>(&self, bytes: &[u8]) -> StorageResult<T>;

    /// Flushes pending in-memory modifications under the specified prefix to disk.
    ///
    /// # Note
    /// Behavior is backend-specific: transactional engines (such as `redb`, `sqlite`) will
    /// selectively commit changes under the given prefix, while monolithic document engines
    /// (such as `json`, `toml`) will serialize and rewrite the entire file.
    fn flush_prefix(&self, prefix: &str) -> StorageResult<()>;

    fn is_initialized(&self, namespace: &str) -> StorageResult<bool>;

    /// Reactive values addressed by path, without declaring a struct. See [`Kv`].
    fn kv(&self) -> crate::store::Kv<Self> {
        crate::store::Kv::new(self.clone())
    }
    fn mark_initialized(&self, namespace: &str) -> StorageResult<()>;
}
