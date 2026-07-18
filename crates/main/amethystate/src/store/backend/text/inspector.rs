use crate::StorageResult;
use crate::observability::InspectorBackend;
use crate::store::CodecFormat;
use crate::store::backend::text::store::{normalize_path, scan_prefix_recursive};
use crate::store::backend::text::{TextDocument, TextStore};
use crate::store::meta::SchemaSnapshot;

impl<D: TextDocument + Send + 'static> InspectorBackend for TextStore<D> {
    fn format(&self) -> CodecFormat {
        D::format()
    }

    fn scan_all(&self) -> StorageResult<Vec<(String, Vec<u8>)>> {
        let guard = self.inner.files.data.doc.read();
        let mut raw_nodes = Vec::new();
        scan_prefix_recursive(&*guard, &[], "", &mut raw_nodes, None);

        let mut results = Vec::new();
        for (k, node) in raw_nodes {
            let bytes = D::node_to_bytes(&node)?;
            results.push((k, bytes));
        }
        Ok(results)
    }

    fn get_schema_snapshots(&self) -> StorageResult<Vec<(String, SchemaSnapshot)>> {
        let guard = self.inner.files.meta.doc.read();
        let mut raw_nodes = Vec::new();
        scan_prefix_recursive(&*guard, &["schema"], "schema", &mut raw_nodes, Some(2));

        let mut results = Vec::new();
        for (full_key, node) in raw_nodes {
            if let Some(prefix) = full_key.strip_prefix("schema.") {
                let snapshot: SchemaSnapshot = D::deserialize_node(&node)?;
                results.push((prefix.to_string(), snapshot));
            }
        }
        Ok(results)
    }

    fn set_raw(&mut self, key: &str, value: &[u8]) -> StorageResult<()> {
        self.inner.check_debouncer()?;
        let path_str = normalize_path(key)?;

        let node = D::bytes_to_node(value)?;

        self.inner.set_node(path_str, node, None)
    }
}
