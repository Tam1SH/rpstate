use super::error::RedbStoreError;
use super::tables::{
    TABLE_DATA, TABLE_META, TABLE_MIGRATION_LOG, TABLE_SCHEMA_SNAPSHOT, TableReader, TableWriter,
};
use crate::migration::AppliedStep;
use crate::store::CodecFormat;
use crate::store::error::StorageResult;
use crate::store::meta::{PrefixMeta, SchemaSnapshot};
use crate::store::traits::MigrationBackendAdapter;
use amethystate_core::path::StorePath;
use redb::ReadableTable;

pub(super) struct RedbMigrationBackend<'a> {
    txn: &'a redb::WriteTransaction,
}

impl<'a> RedbMigrationBackend<'a> {
    pub(super) fn new(txn: &'a redb::WriteTransaction) -> Self {
        Self { txn }
    }
}

impl MigrationBackendAdapter for RedbMigrationBackend<'_> {
    fn format(&self) -> CodecFormat {
        CodecFormat::MessagePack
    }

    fn get(&self, key: &str) -> StorageResult<Option<Vec<u8>>> {
        let table = self
            .txn
            .open_table(TABLE_DATA)
            .map_err(RedbStoreError::from)?;
        Ok(table
            .get(key)
            .map_err(RedbStoreError::from)?
            .map(|v| v.value().to_vec()))
    }

    fn set(&mut self, key: &str, value: &[u8]) -> StorageResult<()> {
        let mut table = self
            .txn
            .open_table(TABLE_DATA)
            .map_err(RedbStoreError::from)?;
        table.insert(key, value).map_err(RedbStoreError::from)?;
        Ok(())
    }

    fn delete(&mut self, key: &str) -> StorageResult<()> {
        let mut table = self
            .txn
            .open_table(TABLE_DATA)
            .map_err(RedbStoreError::from)?;
        table.remove(key).map_err(RedbStoreError::from)?;
        Ok(())
    }

    fn scan_prefix(&self, prefix: &StorePath) -> StorageResult<Vec<(String, Vec<u8>)>> {
        let prefix = prefix.as_str();
        use redb::ReadableTable;
        let table = self
            .txn
            .open_table(TABLE_DATA)
            .map_err(RedbStoreError::from)?;
        let mut result = Vec::new();
        for entry in table.iter().map_err(RedbStoreError::from)? {
            let (k, v) = entry.map_err(RedbStoreError::from)?;
            let key = k.value().to_string();
            if key.starts_with(prefix) {
                result.push((key, v.value().to_vec()));
            }
        }
        Ok(result)
    }
    fn get_meta(&self, prefix: &StorePath) -> StorageResult<Option<PrefixMeta>> {
        let prefix = prefix.as_str();
        Ok(self.txn.load_typed(TABLE_META, prefix)?)
    }

    fn set_meta(&mut self, prefix: &StorePath, meta: &PrefixMeta) -> StorageResult<()> {
        let prefix = prefix.as_str();
        Ok(self.txn.save_typed(TABLE_META, prefix, meta)?)
    }

    fn get_schema_snapshot(&self, prefix: &StorePath) -> StorageResult<Option<SchemaSnapshot>> {
        let prefix = prefix.as_str();
        Ok(self.txn.load_typed(TABLE_SCHEMA_SNAPSHOT, prefix)?)
    }

    fn set_schema_snapshot(
        &mut self,
        prefix: &StorePath,
        snapshot: &SchemaSnapshot,
    ) -> StorageResult<()> {
        let prefix = prefix.as_str();
        Ok(self
            .txn
            .save_typed(TABLE_SCHEMA_SNAPSHOT, prefix, snapshot)?)
    }

    fn get_migration_log(&self, prefix: &StorePath) -> StorageResult<Option<Vec<AppliedStep>>> {
        let prefix = prefix.as_str();
        Ok(self.txn.load_typed(TABLE_MIGRATION_LOG, prefix)?)
    }

    fn set_migration_log(&mut self, prefix: &StorePath, log: &[AppliedStep]) -> StorageResult<()> {
        let prefix = prefix.as_str();
        Ok(self.txn.save_typed(TABLE_MIGRATION_LOG, prefix, &log)?)
    }
}
