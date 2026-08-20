use super::error::SqliteStoreError;
use crate::codec::CodecError;
use crate::migration::AppliedStep;
use crate::store::meta::{PrefixMeta, SchemaSnapshot};
use crate::store::traits::MigrationBackendAdapter;
use crate::store::{CodecFormat, StorageResult};
use amethystate_core::path::StorePath;
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
            .map_err(SqliteStoreError::from)?;
        let res: Option<Vec<u8>> = stmt
            .query_row([key], |row| row.get(0))
            .optional()
            .map_err(SqliteStoreError::from)?;

        match res {
            Some(bytes) => Ok(Some(
                sonic_rs::from_slice(&bytes)
                    .map_err(CodecError::from)
                    .map_err(SqliteStoreError::from)?,
            )),
            None => Ok(None),
        }
    }

    fn set_typed<T: Serialize>(&self, table: &str, key: &str, value: &T) -> StorageResult<()> {
        let bytes = sonic_rs::to_vec(value)
            .map_err(CodecError::from)
            .map_err(SqliteStoreError::from)?;

        let sql = format!("REPLACE INTO {} (key, value) VALUES (?, ?)", table);
        let mut stmt = self
            .txn
            .prepare_cached(&sql)
            .map_err(SqliteStoreError::from)?;
        stmt.execute(rusqlite::params![key, bytes])
            .map_err(SqliteStoreError::from)?;
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
            .map_err(SqliteStoreError::from)?;
        Ok(stmt
            .query_row([key], |row| row.get(0))
            .optional()
            .map_err(SqliteStoreError::from)?)
    }

    fn set(&mut self, key: &str, value: &[u8]) -> StorageResult<()> {
        let mut stmt = self
            .txn
            .prepare_cached("REPLACE INTO data (key, value) VALUES (?, ?)")
            .map_err(SqliteStoreError::from)?;
        stmt.execute(rusqlite::params![key, value])
            .map_err(SqliteStoreError::from)?;
        Ok(())
    }

    fn delete(&mut self, key: &str) -> StorageResult<()> {
        let mut stmt = self
            .txn
            .prepare_cached("DELETE FROM data WHERE key = ?")
            .map_err(SqliteStoreError::from)?;
        stmt.execute([key]).map_err(SqliteStoreError::from)?;
        Ok(())
    }

    fn scan_prefix(&self, prefix: &StorePath) -> StorageResult<Vec<(String, Vec<u8>)>> {
        let mut stmt = self
            .txn
            .prepare_cached("SELECT key, value FROM data WHERE key GLOB ?")
            .map_err(SqliteStoreError::from)?;
        let pattern = format!("{}*", prefix);
        let rows = stmt
            .query_map([pattern], |row| Ok((row.get(0)?, row.get(1)?)))
            .map_err(SqliteStoreError::from)?;

        let mut res = Vec::new();
        for row in rows {
            res.push(row.map_err(SqliteStoreError::from)?);
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
