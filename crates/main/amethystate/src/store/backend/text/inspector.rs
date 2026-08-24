use crate::StorageResult;
use crate::observability::InspectorBackend;
use crate::store::CodecFormat;
use crate::store::StorageError;
use crate::store::backend::text::store::scan_prefix_recursive;
use crate::store::backend::text::{TextDocument, TextStore};
use crate::store::meta::SchemaSnapshot;
use amethystate_core::path::StorePath;
use error_stack::ResultExt;

impl<D: TextDocument + Send + 'static> InspectorBackend for TextStore<D> {
    fn format(&self) -> CodecFormat {
        D::format()
    }

    fn scan_all(&self) -> StorageResult<Vec<(StorePath, Vec<u8>)>> {
        let guard = self.inner.files.data.doc.read();
        let mut raw_nodes = Vec::new();
        scan_prefix_recursive(&*guard, &[], "", &mut raw_nodes, None)
            .attach_with(|| format!("file: {}", self.inner.files.data.path.display()))?;

        let mut results = Vec::new();
        for (k, node) in raw_nodes {
            let bytes = D::node_to_bytes(&node)
                .change_context(StorageError::Scan)
                .attach_with(|| format!("file: {}", self.inner.files.data.path.display()))
                .attach_with(|| format!("node: {k}"))?;
            results.push((crate::store::backend::utils::stored_path(&k)?, bytes));
        }
        Ok(results)
    }

    fn get_schema_snapshots(&self) -> StorageResult<Vec<(String, SchemaSnapshot)>> {
        let guard = self.inner.files.meta.doc.read();
        let records = guard
            .scan(&[])
            .attach_with(|| format!("meta file: {}", self.inner.files.meta.path.display()))?;

        let mut results = Vec::new();
        for (full_key, node) in records {
            if let Some(prefix) = full_key.strip_prefix("schema.") {
                let snapshot: SchemaSnapshot = D::deserialize_node(&node)
                    .change_context(StorageError::Meta)
                    .attach_with(|| format!("meta file: {}", self.inner.files.meta.path.display()))
                    .attach_with(|| format!("meta node: {full_key}"))?;
                results.push((prefix.to_string(), snapshot));
            }
        }
        Ok(results)
    }

    fn set_raw(&mut self, key: &str, value: &[u8]) -> StorageResult<()> {
        self.inner.check_debouncer()?;
        let path = StorePath::parse_joined(key)
            .change_context(StorageError::Path)
            .attach_with(|| format!("key: {key}"))?;

        let node = D::bytes_to_node(value)
            .change_context(StorageError::Write)
            .attach_with(|| format!("file: {}", self.inner.files.data.path.display()))
            .attach_with(|| format!("node: {path}"))?;

        self.inner.set_node(path, node, None)
    }
}
