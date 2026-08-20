use crate::codec::CodecError;
use crate::observability::InspectorBackend;
use crate::store::CodecFormat;
use crate::store::backend::redb::error::RedbStoreError;
use crate::store::backend::redb::tables::TABLE_SCHEMA_SNAPSHOT;
use crate::store::backend::utils;
use crate::store::meta::SchemaSnapshot;
use crate::stores::RedbStore;
use crate::{StorageResult, StoreBackend};
use amethystate_core::path::StorePath;
use redb::{ReadableDatabase, ReadableTable};

impl InspectorBackend for RedbStore {
    fn format(&self) -> CodecFormat {
        CodecFormat::MessagePack
    }

    fn scan_all(&self) -> StorageResult<Vec<(String, Vec<u8>)>> {
        self.scan_prefix(&StorePath::root())
    }

    fn get_schema_snapshots(&self) -> StorageResult<Vec<(String, SchemaSnapshot)>> {
        let read_txn = self.inner.db.begin_read().map_err(RedbStoreError::from)?;
        let table = read_txn
            .open_table(TABLE_SCHEMA_SNAPSHOT)
            .map_err(RedbStoreError::from)?;

        let mut results = Vec::new();
        for entry in table.iter().map_err(RedbStoreError::from)? {
            let (k, v) = entry.map_err(RedbStoreError::from)?;
            let prefix = k.value().to_string();
            let snapshot: SchemaSnapshot = rmp_serde::from_slice(v.value())
                .map_err(CodecError::from)
                .map_err(RedbStoreError::from)?;
            results.push((prefix, snapshot));
        }
        Ok(results)
    }

    fn set_raw(&mut self, key: &str, value: &[u8]) -> StorageResult<()> {
        self.inner.check_debouncer();
        utils::set_raw_pending(
            &self.inner.pending,
            &self.inner.subscriptions,
            &self.inner.debouncer,
            key,
            value,
        )
    }
}
