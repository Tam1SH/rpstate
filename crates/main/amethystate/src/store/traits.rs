use crate::codec::CodecError;
use crate::migration::AppliedStep;
use crate::migration::set::MigrationSet;
use crate::store::error::{StorageError, StorageResult};
use amethystate_core::path::{IntoStorePath, StorePath};

use crate::store::meta::{PrefixMeta, SchemaSnapshot};
use crate::store::{CodecFormat, Kv, StoreCallback, SubscriptionId};
use crate::{MigrationReport, Store, SubscriptionKind};
use error_stack::{Report, ResultExt};
use serde::{Serialize, de::DeserializeOwned};
use uuid::Uuid;

/// The path a caller named, or why what they gave is not one.
///
/// Every typed entry point takes `impl IntoStorePath` and has to make the same
/// conversion; doing it here means the failure gets named once rather than at
/// each of them.
pub fn to_path(path: impl IntoStorePath) -> StorageResult<StorePath> {
    path.into_store_path().change_context(StorageError::Path)
}

/// One more level under `path`, named by a map key.
///
/// A key comes from the caller's data, so it can turn out not to be a name at
/// all; where that happens the report says which map and which key.
pub fn entry_path(path: &StorePath, key: impl AsRef<str>) -> StorageResult<StorePath> {
    let key = key.as_ref();
    path.try_push(key)
        .change_context(StorageError::Path)
        .attach_with(|| format!("map: {path}, key: {key}"))
}

pub trait MigrationBackendAdapter {
    fn format(&self) -> CodecFormat;

    fn get(&self, key: &str) -> StorageResult<Option<Vec<u8>>>;
    fn set(&mut self, key: &str, value: &[u8]) -> StorageResult<()>;
    fn delete(&mut self, key: &str) -> StorageResult<()>;
    fn scan_prefix(&self, prefix: &StorePath) -> StorageResult<Vec<(StorePath, Vec<u8>)>>;

    fn get_meta(&self, prefix: &StorePath) -> StorageResult<Option<PrefixMeta>>;
    fn set_meta(&mut self, prefix: &StorePath, meta: &PrefixMeta) -> StorageResult<()>;
    fn get_schema_snapshot(&self, prefix: &StorePath) -> StorageResult<Option<SchemaSnapshot>>;
    fn set_schema_snapshot(
        &mut self,
        prefix: &StorePath,
        snapshot: &SchemaSnapshot,
    ) -> StorageResult<()>;
    fn get_migration_log(&self, prefix: &StorePath) -> StorageResult<Option<Vec<AppliedStep>>>;
    fn set_migration_log(&mut self, prefix: &StorePath, log: &[AppliedStep]) -> StorageResult<()>;
}

pub trait SchemaAwareStore: StoreBackend {
    fn run_migrations(&self, mset: MigrationSet) -> StorageResult<MigrationReport>;
}

/// The store addressed by path, with nothing in the way.
///
/// These are the backrooms. Here be dragons.
///
/// [`Kv`](crate::store::Kv) is the surface to reach for: it refuses a write at a
/// path a declared struct owns, so a `u16` field cannot be overwritten with a
/// `String` by code that never saw the declaration. Nothing here does. A write
/// through this trait lands wherever it is aimed, and `delete_prefix` takes the
/// subtree it is given - declared paths included, and the initialization markers
/// that decide whether defaults are seeded left behind.
///
/// Which is the point: the engines implement it, the schema layer is built on
/// it, and a caller who knows exactly what they are addressing can use it. A
/// caller who is guessing wants `Kv`.
pub trait StoreBackend: Send + Sync + 'static {
    fn get_raw(&self, path: &StorePath) -> StorageResult<Option<Vec<u8>>>;

    fn set_erased(
        &self,
        path: &StorePath,
        value: &dyn erased_serde::Serialize,
        source: Option<Uuid>,
    ) -> StorageResult<()>;

    fn set_owned_erased(
        &self,
        path: StorePath,
        value: &dyn erased_serde::Serialize,
        source: Option<Uuid>,
    ) -> StorageResult<()>;

    /// Runs `f` against a deserializer positioned at `path`, in the backend's
    /// own format. `Ok(false)` means the key is absent and `f` never ran.
    fn get_erased(
        &self,
        path: &StorePath,
        f: &mut dyn FnMut(&mut dyn erased_serde::Deserializer) -> StorageResult<()>,
    ) -> StorageResult<bool>;

    /// Same, for bytes carried by a [`crate::StoreEvent`].
    fn decode_erased(
        &self,
        bytes: &[u8],
        f: &mut dyn FnMut(&mut dyn erased_serde::Deserializer) -> StorageResult<()>,
    ) -> StorageResult<()>;

    fn delete_with_source(&self, path: &StorePath, source: Option<Uuid>) -> StorageResult<()>;
    fn delete(&self, path: &StorePath) -> StorageResult<()>;

    /// Removes every key under `prefix`, emitting one
    /// [`crate::StoreOp::DeletePrefix`] instead of a `Delete` per key.
    fn delete_prefix_with_source(
        &self,
        prefix: &StorePath,
        source: Option<Uuid>,
    ) -> StorageResult<()>;

    fn delete_prefix(&self, prefix: &StorePath) -> StorageResult<()> {
        self.delete_prefix_with_source(prefix, None)
    }

    /// Every key under `prefix`, sorted by key on every backend.
    ///
    /// Lists what [`StoreBackend::scan_keys`] lists.
    fn scan_prefix(&self, prefix: &StorePath) -> StorageResult<Vec<(StorePath, Vec<u8>)>>;

    /// Hands every entry under `prefix` to `visit`, in the order a scan lists.
    ///
    /// What it saves over [`StoreBackend::scan_prefix`] is everything that has
    /// to be built to hand an entry over as owned: a `StorePath` per key,
    /// which is a string and a walk, and a `Vec` per value, which is a copy
    /// out of the engine's page. A caller that decodes each entry on the spot
    /// - which is what loading a map is - drops both immediately.
    ///
    /// The key arrives as it is stored, joined and escaped, and has not been
    /// checked: [`name_under_key`](amethystate_core::path::name_under_key)
    /// reads a level out of one and refuses a key this library did not write.
    ///
    /// Defaulted through `scan_prefix`, so a backend implemented outside this
    /// crate stays correct without knowing this exists - it simply pays what it
    /// paid before.
    fn visit_prefix(
        &self,
        prefix: &StorePath,
        visit: &mut dyn FnMut(&str, &[u8]) -> StorageResult<()>,
    ) -> StorageResult<()> {
        for (path, bytes) in self.scan_prefix(prefix)? {
            visit(path.as_str(), &bytes)?;
        }
        Ok(())
    }

    /// The keys under `prefix`, sorted, without reading their values.
    ///
    /// `scan_prefix` copies every value out of the backend, which is wasted
    /// work when only the keys are wanted - and grows with the data rather
    /// than with the answer.
    ///
    #[doc = include_str!("scan_contract.md")]
    fn scan_keys(&self, prefix: &StorePath) -> StorageResult<Vec<StorePath>>;

    /// Whether this store was asked to read large collections on more than one
    /// core - [`StoreConfig::parallel_reads`](crate::store::config::StoreConfig).
    ///
    /// Defaulted so a backend implemented outside this crate need not know the
    /// question exists; answering `false` only means its reads stay on the
    /// calling thread, which is what they did before the question was asked.
    fn parallel_reads(&self) -> bool {
        false
    }

    fn save_now(&self) -> StorageResult<()>;

    fn subscribe(&self, kind: SubscriptionKind, callback: StoreCallback) -> SubscriptionId;
    fn unsubscribe(&self, id: SubscriptionId);

    /// Flushes pending in-memory modifications under the specified prefix to disk.
    ///
    /// # Note
    /// Behavior is backend-specific: transactional engines (such as `redb`, `sqlite`) will
    /// selectively commit changes under the given prefix, while monolithic document engines
    /// (such as `json`, `toml`) will serialize and rewrite the entire file.
    fn flush_prefix(&self, prefix: &StorePath) -> StorageResult<()>;

    /// Commits without blocking; the future resolves once a flush has landed.
    ///
    /// Waiters ride on the flush the store was going to do anyway, so several
    /// of them cost one commit rather than one each.
    fn flush_async(&self) -> crate::store::durable::Commit;

    fn is_initialized(&self, namespace: &str) -> StorageResult<bool>;

    /// Records whether `namespace` has been seeded.
    ///
    /// The one bit no amount of reading the data reproduces: a namespace whose
    /// values were all removed looks exactly like one that was never written.
    /// Which way it reads decides whether the next construction puts the
    /// declared defaults back.
    ///
    /// Setting a namespace [`Fresh`](InitState::Fresh) that was never seeded is
    /// not an error.
    fn set_initialized(&self, namespace: &str, state: InitState) -> StorageResult<()>;

    fn mark_initialized(&self, namespace: &str) -> StorageResult<()> {
        self.set_initialized(namespace, InitState::Seeded)
    }
}

/// Whether a namespace has had its declared defaults written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InitState {
    /// The defaults have been written; do not write them again.
    Seeded,

    /// Nothing has been written here, so the next construction seeds it.
    Fresh,
}

impl InitState {
    pub fn is_seeded(self) -> bool {
        matches!(self, InitState::Seeded)
    }
}

/// The typed surface over [`StoreBackend`]. Blanket-implemented, including for
/// `dyn StoreBackend`, so a call site never has to know which it holds.
pub trait StoreExt: StoreBackend {
    fn get<T: DeserializeOwned>(&self, path: impl IntoStorePath) -> StorageResult<Option<T>> {
        let path = to_path(path)?;
        let mut out = None;
        let found = self.get_erased(&path, &mut |d| {
            out = Some(
                erased_serde::deserialize::<T>(d)
                    .map_err(CodecError::from)
                    .change_context(StorageError::Codec)
                    .attach_with(|| format!("path: {path}"))?,
            );
            Ok(())
        })?;
        Ok(if found { out } else { None })
    }

    fn set<T: Serialize>(&self, path: impl IntoStorePath, value: &T) -> StorageResult<()> {
        self.set_erased(&to_path(path)?, &value, None)
    }

    fn set_owned<T: Serialize>(&self, path: StorePath, value: &T) -> StorageResult<()> {
        self.set_owned_erased(path, &value, None)
    }

    fn set_with_source<T: Serialize>(
        &self,
        path: impl IntoStorePath,
        value: &T,
        source: Option<Uuid>,
    ) -> StorageResult<()> {
        self.set_erased(&to_path(path)?, &value, source)
    }

    fn set_owned_with_source<T: Serialize>(
        &self,
        path: StorePath,
        value: &T,
        source: Option<Uuid>,
    ) -> StorageResult<()> {
        self.set_owned_erased(path, &value, source)
    }

    /// Reads bytes that arrived in a [`StoreEvent`](crate::StoreEvent) as `T`.
    fn decode<T: DeserializeOwned>(&self, bytes: &[u8]) -> StorageResult<T> {
        let mut out = None;
        self.decode_erased(bytes, &mut |d| {
            out = Some(
                erased_serde::deserialize::<T>(d)
                    .map_err(CodecError::from)
                    .change_context(StorageError::Codec)
                    .attach_with(|| format!("as: {}", std::any::type_name::<T>()))?,
            );
            Ok(())
        })?;

        out.ok_or_else(|| {
            Report::new(StorageError::Codec)
                .attach("the backend accepted the bytes without producing a value")
                .attach(format!("decoding {} bytes", bytes.len()))
        })
    }
}

impl<S: StoreBackend + ?Sized> StoreExt for S {}

/// Reactive values addressed by path, without declaring a struct. See [`crate::store::Kv`].
impl Store {
    pub fn kv(&self) -> Kv {
        Kv::new(self.clone())
    }
}
