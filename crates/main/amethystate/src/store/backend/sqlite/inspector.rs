use crate::codec::CodecError;
use crate::observability::InspectorBackend;
use crate::store::CodecFormat;
use crate::store::backend::sqlite::error::SqliteStoreError;
use crate::store::backend::utils;
use crate::store::error::StorageError;
use crate::store::meta::SchemaSnapshot;
use crate::stores::SqliteStore;
use crate::{StorageResult, StoreBackend};
use amethystate_core::path::StorePath;
use error_stack::ResultExt;

impl InspectorBackend for SqliteStore {
    fn format(&self) -> CodecFormat {
        CodecFormat::SonicJson
    }

    fn scan_all(&self) -> StorageResult<Vec<(String, Vec<u8>)>> {
        self.scan_prefix(&StorePath::root())
    }

    fn get_schema_snapshots(&self) -> StorageResult<Vec<(String, SchemaSnapshot)>> {
        let conn = self.inner.conn.lock();
        let mut stmt = conn
            .prepare_cached("SELECT key, value FROM schema_snapshot")
            .map_err(SqliteStoreError::from)
            .change_context(StorageError::Meta)
            .attach("table: schema_snapshot")?;
        let rows = stmt
            .query_map([], |row| {
                let key: String = row.get(0)?;
                let bytes: Vec<u8> = row.get(1)?;
                Ok((key, bytes))
            })
            .map_err(SqliteStoreError::from)
            .change_context(StorageError::Meta)
            .attach("table: schema_snapshot")?;

        let mut results = Vec::new();
        for row in rows {
            let (key, bytes) = row
                .map_err(SqliteStoreError::from)
                .change_context(StorageError::Meta)
                .attach("table: schema_snapshot")
                .attach_with(|| format!("snapshots read: {}", results.len()))?;
            let snapshot: SchemaSnapshot = sonic_rs::from_slice(&bytes)
                .map_err(CodecError::from)
                .change_context(StorageError::Codec)
                .attach("table: schema_snapshot")
                .attach_with(|| format!("key: {key}"))
                .attach_with(|| format!("value bytes: {}", bytes.len()))?;
            results.push((key, snapshot));
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
