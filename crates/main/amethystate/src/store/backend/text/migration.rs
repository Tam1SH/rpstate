use crate::migration::AppliedStep;
use crate::store::CodecFormat;
use crate::store::StorageResult;
use crate::store::backend::text::document::TextDocument;
use crate::store::backend::text::store;
use crate::store::meta::{PrefixMeta, SchemaSnapshot};
use crate::store::traits::MigrationBackendAdapter;

pub struct TextMigrationBackend<'a, D: TextDocument> {
    pub(crate) data_doc: &'a mut D,
    pub(crate) meta_doc: &'a mut D,
}

impl<D: TextDocument> MigrationBackendAdapter for TextMigrationBackend<'_, D> {
    fn format(&self) -> CodecFormat {
        D::format()
    }

    fn get(&self, key: &str) -> StorageResult<Option<Vec<u8>>> {
        let parts = store::split_path(key);
        if let Some(node) = self.data_doc.get(&parts) {
            Ok(Some(D::node_to_bytes(node)?))
        } else {
            Ok(None)
        }
    }

    fn set(&mut self, key: &str, value: &[u8]) -> StorageResult<()> {
        let parts = store::split_path(key);
        let node = D::bytes_to_node(value)?;
        self.data_doc.set(&parts, node)?;
        Ok(())
    }

    fn delete(&mut self, key: &str) -> StorageResult<()> {
        let parts = store::split_path(key);
        self.data_doc.delete(&parts)?;
        Ok(())
    }

    fn scan_prefix(&self, prefix: &str) -> StorageResult<Vec<(String, Vec<u8>)>> {
        store::scan_prefix_impl(self.data_doc, prefix)
    }

    fn get_meta(&self, prefix: &str) -> StorageResult<Option<PrefixMeta>> {
        let parts = vec!["meta", prefix];
        if let Some(node) = self.meta_doc.get(&parts) {
            Ok(Some(D::deserialize_node(node)?))
        } else {
            Ok(None)
        }
    }

    fn set_meta(&mut self, prefix: &str, meta: &PrefixMeta) -> StorageResult<()> {
        let parts = vec!["meta", prefix];
        let node = D::serialize_node(meta)?;
        self.meta_doc.set(&parts, node)?;
        Ok(())
    }

    fn get_schema_snapshot(&self, prefix: &str) -> StorageResult<Option<SchemaSnapshot>> {
        let parts = vec!["schema", prefix];
        if let Some(node) = self.meta_doc.get(&parts) {
            Ok(Some(D::deserialize_node(node)?))
        } else {
            Ok(None)
        }
    }

    fn set_schema_snapshot(
        &mut self,
        prefix: &str,
        snapshot: &SchemaSnapshot,
    ) -> StorageResult<()> {
        let parts = vec!["schema", prefix];
        let node = D::serialize_node(snapshot)?;
        self.meta_doc.set(&parts, node)?;
        Ok(())
    }

    fn get_migration_log(&self, prefix: &str) -> StorageResult<Option<Vec<AppliedStep>>> {
        let parts = vec!["log", prefix];
        if let Some(node) = self.meta_doc.get(&parts) {
            Ok(Some(D::deserialize_node(node)?))
        } else {
            Ok(None)
        }
    }

    fn set_migration_log(&mut self, prefix: &str, log: &[AppliedStep]) -> StorageResult<()> {
        let parts = vec!["log", prefix];
        let node = D::serialize_node(&log)?;
        self.meta_doc.set(&parts, node)?;
        Ok(())
    }
}
