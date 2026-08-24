use crate::migration::AppliedStep;
use crate::store::CodecFormat;
use crate::store::StorageError;
use crate::store::StorageResult;
use crate::store::backend::text::document::TextDocument;
use crate::store::backend::text::store;
use crate::store::meta::{PrefixMeta, SchemaSnapshot};
use crate::store::traits::MigrationBackendAdapter;
use amethystate_core::path::StorePath;
use error_stack::ResultExt;

fn migration_path(key: &str) -> StorageResult<StorePath> {
    StorePath::parse_joined(key)
        .change_context(StorageError::Path)
        .attach_with(|| format!("migration key: {key}"))
}

pub struct TextMigrationBackend<'a, D: TextDocument> {
    pub(crate) data_doc: &'a mut D,
    pub(crate) meta_doc: &'a mut D,
}

impl<D: TextDocument> MigrationBackendAdapter for TextMigrationBackend<'_, D> {
    fn format(&self) -> CodecFormat {
        D::format()
    }

    fn get(&self, key: &str) -> StorageResult<Option<Vec<u8>>> {
        let path = migration_path(key)?;
        let parts: Vec<&str> = path.segments().collect();
        if let Some(node) = self.data_doc.get(&parts) {
            Ok(Some(
                D::node_to_bytes(node)
                    .change_context(StorageError::Migrate)
                    .attach_with(|| format!("node: {key}"))
                    .attach("reading the data file through the migration adapter")?,
            ))
        } else {
            Ok(None)
        }
    }

    fn set(&mut self, key: &str, value: &[u8]) -> StorageResult<()> {
        let path = migration_path(key)?;
        let parts: Vec<&str> = path.segments().collect();
        let node = D::bytes_to_node(value)
            .change_context(StorageError::Migrate)
            .attach_with(|| format!("node: {key}"))
            .attach("writing the data file through the migration adapter")?;
        self.data_doc
            .set(&parts, node)
            .change_context(StorageError::Migrate)
            .attach_with(|| format!("node: {key}"))
            .attach("writing the data file through the migration adapter")?;
        Ok(())
    }

    fn delete(&mut self, key: &str) -> StorageResult<()> {
        let path = migration_path(key)?;
        let parts: Vec<&str> = path.segments().collect();
        self.data_doc
            .delete(&parts)
            .change_context(StorageError::Migrate)
            .attach_with(|| format!("node: {key}"))
            .attach("deleting from the data file through the migration adapter")?;
        Ok(())
    }

    fn scan_prefix(&self, prefix: &StorePath) -> StorageResult<Vec<(StorePath, Vec<u8>)>> {
        store::scan_prefix_impl(self.data_doc, prefix)
            .change_context(StorageError::Migrate)
            .attach_with(|| format!("prefix: {prefix}"))
            .attach("scanning the data file through the migration adapter")
    }

    fn get_meta(&self, prefix: &StorePath) -> StorageResult<Option<PrefixMeta>> {
        let key = store::meta_key("meta", prefix);
        if let Some(node) = self.meta_doc.get(&[key.as_str()]) {
            Ok(Some(
                D::deserialize_node(node)
                    .change_context(StorageError::Meta)
                    .attach_with(|| format!("meta node: meta.{prefix}"))?,
            ))
        } else {
            Ok(None)
        }
    }

    fn set_meta(&mut self, prefix: &StorePath, meta: &PrefixMeta) -> StorageResult<()> {
        let key = store::meta_key("meta", prefix);
        let parts = [key.as_str()];
        let node = D::serialize_node(meta)
            .change_context(StorageError::Meta)
            .attach_with(|| format!("meta node: meta.{prefix}"))?;
        self.meta_doc
            .set(&parts, node)
            .change_context(StorageError::Meta)
            .attach_with(|| format!("meta node: meta.{prefix}"))?;
        Ok(())
    }

    fn get_schema_snapshot(&self, prefix: &StorePath) -> StorageResult<Option<SchemaSnapshot>> {
        let key = store::meta_key("schema", prefix);
        if let Some(node) = self.meta_doc.get(&[key.as_str()]) {
            Ok(Some(
                D::deserialize_node(node)
                    .change_context(StorageError::Meta)
                    .attach_with(|| format!("meta node: schema.{prefix}"))?,
            ))
        } else {
            Ok(None)
        }
    }

    fn set_schema_snapshot(
        &mut self,
        prefix: &StorePath,
        snapshot: &SchemaSnapshot,
    ) -> StorageResult<()> {
        let key = store::meta_key("schema", prefix);
        let parts = [key.as_str()];
        let node = D::serialize_node(snapshot)
            .change_context(StorageError::Meta)
            .attach_with(|| format!("meta node: schema.{prefix}"))?;
        self.meta_doc
            .set(&parts, node)
            .change_context(StorageError::Meta)
            .attach_with(|| format!("meta node: schema.{prefix}"))?;
        Ok(())
    }

    fn get_migration_log(&self, prefix: &StorePath) -> StorageResult<Option<Vec<AppliedStep>>> {
        let key = store::meta_key("log", prefix);
        if let Some(node) = self.meta_doc.get(&[key.as_str()]) {
            Ok(Some(
                D::deserialize_node(node)
                    .change_context(StorageError::Meta)
                    .attach_with(|| format!("meta node: log.{prefix}"))?,
            ))
        } else {
            Ok(None)
        }
    }

    fn set_migration_log(&mut self, prefix: &StorePath, log: &[AppliedStep]) -> StorageResult<()> {
        let key = store::meta_key("log", prefix);
        let parts = [key.as_str()];
        let node = D::serialize_node(&log)
            .change_context(StorageError::Meta)
            .attach_with(|| format!("meta node: log.{prefix}"))?;
        self.meta_doc
            .set(&parts, node)
            .change_context(StorageError::Meta)
            .attach_with(|| format!("meta node: log.{prefix}"))?;
        Ok(())
    }
}
