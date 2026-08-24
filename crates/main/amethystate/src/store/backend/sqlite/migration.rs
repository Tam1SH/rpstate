use super::error::SqliteStoreError;
use crate::codec::CodecError;
use crate::migration::AppliedStep;
use crate::store::error::StorageError;
use crate::store::meta::{PrefixMeta, SchemaSnapshot};
use crate::store::traits::MigrationBackendAdapter;
use crate::store::{CodecFormat, StorageResult};
use amethystate_core::path::StorePath;
use error_stack::ResultExt;
use rusqlite::{OptionalExtension, Transaction};
use serde::Serialize;
use serde::de::DeserializeOwned;

pub struct SqliteMigrationBackend<'a> {
    pub(crate) txn: &'a Transaction<'a>,
}

impl<'a> SqliteMigrationBackend<'a> {
    pub fn new(txn: &'a Transaction<'a>) -> Self {
        Self { txn }
    }

    fn get_typed<T: DeserializeOwned>(&self, table: &str, key: &str) -> StorageResult<Option<T>> {
        let sql = format!("SELECT value FROM {} WHERE key = ?", table);
        let mut stmt = self
            .txn
            .prepare_cached(&sql)
            .map_err(SqliteStoreError::from)
            .change_context(StorageError::Meta)
            .attach_with(|| format!("table: {table}"))
            .attach_with(|| format!("key: {key}"))?;
        let res: Option<Vec<u8>> = stmt
            .query_row([key], |row| row.get(0))
            .optional()
            .map_err(SqliteStoreError::from)
            .change_context(StorageError::Meta)
            .attach_with(|| format!("table: {table}"))
            .attach_with(|| format!("key: {key}"))?;

        match res {
            Some(bytes) => Ok(Some(
                sonic_rs::from_slice(&bytes)
                    .map_err(CodecError::from)
                    .change_context(StorageError::Codec)
                    .attach_with(|| format!("table: {table}"))
                    .attach_with(|| format!("key: {key}"))
                    .attach_with(|| format!("value bytes: {}", bytes.len()))?,
            )),
            None => Ok(None),
        }
    }

    fn set_typed<T: Serialize>(&self, table: &str, key: &str, value: &T) -> StorageResult<()> {
        let bytes = sonic_rs::to_vec(value)
            .map_err(CodecError::from)
            .change_context(StorageError::Codec)
            .attach_with(|| format!("table: {table}"))
            .attach_with(|| format!("key: {key}"))?;

        let sql = format!("REPLACE INTO {} (key, value) VALUES (?, ?)", table);
        let mut stmt = self
            .txn
            .prepare_cached(&sql)
            .map_err(SqliteStoreError::from)
            .change_context(StorageError::Meta)
            .attach_with(|| format!("table: {table}"))
            .attach_with(|| format!("key: {key}"))?;
        stmt.execute(rusqlite::params![key, bytes])
            .map_err(SqliteStoreError::from)
            .change_context(StorageError::Meta)
            .attach_with(|| format!("table: {table}"))
            .attach_with(|| format!("key: {key}"))?;
        Ok(())
    }
}

impl MigrationBackendAdapter for SqliteMigrationBackend<'_> {
    fn format(&self) -> CodecFormat {
        CodecFormat::SonicJson
    }

    fn get(&self, key: &str) -> StorageResult<Option<Vec<u8>>> {
        let mut stmt = self
            .txn
            .prepare_cached("SELECT value FROM data WHERE key = ?")
            .map_err(SqliteStoreError::from)
            .change_context(StorageError::Read)
            .attach_with(|| format!("key: {key}"))?;
        stmt.query_row([key], |row| row.get(0))
            .optional()
            .map_err(SqliteStoreError::from)
            .change_context(StorageError::Read)
            .attach_with(|| format!("key: {key}"))
    }

    fn set(&mut self, key: &str, value: &[u8]) -> StorageResult<()> {
        let mut stmt = self
            .txn
            .prepare_cached("REPLACE INTO data (key, value) VALUES (?, ?)")
            .map_err(SqliteStoreError::from)
            .change_context(StorageError::Write)
            .attach_with(|| format!("key: {key}"))?;
        stmt.execute(rusqlite::params![key, value])
            .map_err(SqliteStoreError::from)
            .change_context(StorageError::Write)
            .attach_with(|| format!("key: {key}"))
            .attach_with(|| format!("value bytes: {}", value.len()))?;
        Ok(())
    }

    fn delete(&mut self, key: &str) -> StorageResult<()> {
        let mut stmt = self
            .txn
            .prepare_cached("DELETE FROM data WHERE key = ?")
            .map_err(SqliteStoreError::from)
            .change_context(StorageError::Delete)
            .attach_with(|| format!("key: {key}"))?;
        stmt.execute([key])
            .map_err(SqliteStoreError::from)
            .change_context(StorageError::Delete)
            .attach_with(|| format!("key: {key}"))?;
        Ok(())
    }

    fn scan_prefix(&self, prefix: &StorePath) -> StorageResult<Vec<(StorePath, Vec<u8>)>> {
        let mut stmt = self
            .txn
            .prepare_cached("SELECT key, value FROM data WHERE key GLOB ?")
            .map_err(SqliteStoreError::from)
            .change_context(StorageError::Scan)
            .attach_with(|| format!("prefix: {prefix}"))?;
        let pattern = format!("{}*", prefix);
        let rows = stmt
            .query_map([&pattern], |row| Ok((row.get(0)?, row.get(1)?)))
            .map_err(SqliteStoreError::from)
            .change_context(StorageError::Scan)
            .attach_with(|| format!("glob: {pattern}"))?;

        let mut res = Vec::new();
        for row in rows {
            let (key, value): (String, Vec<u8>) = row
                .map_err(SqliteStoreError::from)
                .change_context(StorageError::Scan)
                .attach_with(|| format!("glob: {pattern}"))
                .attach_with(|| format!("rows read: {}", res.len()))?;
            res.push((crate::store::backend::utils::stored_path(&key)?, value));
        }
        Ok(res)
    }

    fn get_meta(&self, prefix: &StorePath) -> StorageResult<Option<PrefixMeta>> {
        self.get_typed("metadata", prefix.as_str())
    }
    fn set_meta(&mut self, prefix: &StorePath, meta: &PrefixMeta) -> StorageResult<()> {
        self.set_typed("metadata", prefix.as_str(), meta)
    }

    fn get_schema_snapshot(&self, prefix: &StorePath) -> StorageResult<Option<SchemaSnapshot>> {
        self.get_typed("schema_snapshot", prefix.as_str())
    }
    fn set_schema_snapshot(
        &mut self,
        prefix: &StorePath,
        snapshot: &SchemaSnapshot,
    ) -> StorageResult<()> {
        self.set_typed("schema_snapshot", prefix.as_str(), snapshot)
    }

    fn get_migration_log(&self, prefix: &StorePath) -> StorageResult<Option<Vec<AppliedStep>>> {
        self.get_typed("migration_log", prefix.as_str())
    }
    fn set_migration_log(&mut self, prefix: &StorePath, log: &[AppliedStep]) -> StorageResult<()> {
        self.set_typed("migration_log", prefix.as_str(), &log)
    }
}
