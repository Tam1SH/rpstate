use crate::migration::AppliedStep;
use crate::migration::set::MigrationSet;
use crate::store::error::StorageResult;
use crate::store::meta::{PrefixMeta, SchemaSnapshot};
use crate::store::{CodecFormat, StoreCallback, SubscriptionId};
use crate::{MigrationReport, Store, SubscriptionKind};
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

pub trait SchemaAwareStore: StoreBackend {
    fn run_migrations(&self, mset: MigrationSet) -> StorageResult<MigrationReport>;
}

pub trait StoreBackend: Send + Sync + 'static {
    fn get_raw(&self, path: &str) -> StorageResult<Option<Vec<u8>>>;

    fn set_erased(
        &self,
        path: &str,
        value: &dyn erased_serde::Serialize,
        source: Option<Uuid>,
    ) -> StorageResult<()>;

    fn set_owned_erased(
        &self,
        path: Arc<str>,
        value: &dyn erased_serde::Serialize,
        source: Option<Uuid>,
    ) -> StorageResult<()>;

    /// Runs `f` against a deserializer positioned at `path`, in the backend's
    /// own format. `Ok(false)` means the key is absent and `f` never ran.
    fn get_erased(
        &self,
        path: &str,
        f: &mut dyn FnMut(&mut dyn erased_serde::Deserializer) -> StorageResult<()>,
    ) -> StorageResult<bool>;

    /// Same, for bytes carried by a [`crate::StoreEvent`].
    fn decode_erased(
        &self,
        bytes: &[u8],
        f: &mut dyn FnMut(&mut dyn erased_serde::Deserializer) -> StorageResult<()>,
    ) -> StorageResult<()>;

    fn delete_with_source(&self, path: &str, source: Option<Uuid>) -> StorageResult<()>;
    fn delete(&self, path: &str) -> StorageResult<()>;

    /// Removes every key under `prefix`, emitting one
    /// [`crate::StoreOp::DeletePrefix`] instead of a `Delete` per key.
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

    /// Flushes pending in-memory modifications under the specified prefix to disk.
    ///
    /// # Note
    /// Behavior is backend-specific: transactional engines (such as `redb`, `sqlite`) will
    /// selectively commit changes under the given prefix, while monolithic document engines
    /// (such as `json`, `toml`) will serialize and rewrite the entire file.
    fn flush_prefix(&self, prefix: &str) -> StorageResult<()>;

    /// Commits without blocking; the future resolves once a flush has landed.
    ///
    /// Waiters ride on the flush the store was going to do anyway, so several
    /// of them cost one commit rather than one each.
    fn flush_async(&self) -> crate::store::durable::Commit;

    fn is_initialized(&self, namespace: &str) -> StorageResult<bool>;
    fn mark_initialized(&self, namespace: &str) -> StorageResult<()>;
}

/// The typed surface over [`StoreBackend`]. Blanket-implemented, including for
/// `dyn StoreBackend`, so a call site never has to know which it holds.
pub trait StoreExt: StoreBackend {
    fn get<T: DeserializeOwned>(&self, path: &str) -> StorageResult<Option<T>> {
        let mut out = None;
        let found = self.get_erased(path, &mut |d| {
            out = Some(erased_serde::deserialize::<T>(d).map_err(crate::codec::CodecError::from)?);
            Ok(())
        })?;
        Ok(if found { out } else { None })
    }

    fn set<T: Serialize>(&self, path: &str, value: &T) -> StorageResult<()> {
        self.set_erased(path, &value, None)
    }

    fn set_owned<T: Serialize>(&self, path: Arc<str>, value: &T) -> StorageResult<()> {
        self.set_owned_erased(path, &value, None)
    }

    fn set_with_source<T: Serialize>(
        &self,
        path: &str,
        value: &T,
        source: Option<Uuid>,
    ) -> StorageResult<()> {
        self.set_erased(path, &value, source)
    }

    fn set_owned_with_source<T: Serialize>(
        &self,
        path: Arc<str>,
        value: &T,
        source: Option<Uuid>,
    ) -> StorageResult<()> {
        self.set_owned_erased(path, &value, source)
    }

    /// Decode failures are not errors here: corrupted bytes or a changed type
    /// yield `T::default()` with a warning, which is what the field would read
    /// on the next startup anyway.
    fn decode<T: DeserializeOwned + Default>(&self, bytes: &[u8]) -> StorageResult<T> {
        let mut out = None;
        let res = self.decode_erased(bytes, &mut |d| {
            out = Some(erased_serde::deserialize::<T>(d).map_err(crate::codec::CodecError::from)?);
            Ok(())
        });
        match res {
            Ok(()) => Ok(out.unwrap_or_default()),
            Err(e) => {
                tracing::warn!(
                    target: "amethystate",
                    "Failed to decode field. Data is corrupted or type changed.                     Using Default value. Error: {e}"
                );
                Ok(T::default())
            }
        }
    }
}

impl<S: StoreBackend + ?Sized> StoreExt for S {}

/// Reactive values addressed by path, without declaring a struct. See [`crate::store::Kv`].
impl Store {
    pub fn kv(&self) -> crate::store::Kv {
        crate::store::Kv::new(self.clone())
    }
}
