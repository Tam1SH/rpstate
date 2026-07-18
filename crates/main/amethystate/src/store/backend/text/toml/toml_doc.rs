use crate::StorageResult;
use crate::codec::CodecError;
use crate::store::backend::text::TextStoreError;
use crate::store::backend::text::document::{
    Navigable, TextDocument, generic_delete, generic_get, generic_scan, generic_set,
};
use crate::store::{CodecFormat, StorageError};
use serde::Serialize;
use serde::de::DeserializeOwned;

#[derive(Clone, Debug)]
pub struct TomlDocument(pub toml_edit::DocumentMut);

impl Navigable for toml_edit::Item {
    fn make_empty_map() -> Self {
        toml_edit::Item::Table(toml_edit::Table::new())
    }
    fn get_child(&self, key: &str) -> Option<&Self> {
        self.get(key)
    }
    fn get_child_mut(&mut self, key: &str) -> Option<&mut Self> {
        self.get_mut(key)
    }
    fn ensure_map(&mut self) {
        if !self.is_table() {
            *self = Self::make_empty_map();
        }
    }
    fn insert_child(&mut self, key: &str, val: Self) {
        self.ensure_map();
        self.as_table_mut().unwrap().insert(key, val);
    }
    fn remove_child(&mut self, key: &str) -> Option<Self> {
        self.as_table_mut().and_then(|t| t.remove(key))
    }
    fn scan_children(&self) -> Vec<(String, Self)> {
        let mut results = Vec::new();
        if let Some(tbl) = self.as_table_like() {
            for (k, v) in tbl.iter() {
                results.push((k.to_string(), v.clone()));
            }
        }
        results
    }
}

impl TextDocument for TomlDocument {
    type Node = toml_edit::Item;

    fn format() -> CodecFormat {
        CodecFormat::Toml
    }

    fn get(&self, parts: &[&str]) -> Option<&Self::Node> {
        generic_get(self.0.as_item(), parts)
    }

    fn set(&mut self, parts: &[&str], node: Self::Node) -> StorageResult<()> {
        let is_root = parts.is_empty() || parts == ["."];
        if is_root {
            let table = match node.into_table() {
                Ok(t) => t,
                Err(_) => {
                    return Err(StorageError::TextStore(TextStoreError::RootMustBeObject));
                }
            };
            *self.0.as_item_mut() = toml_edit::Item::Table(table);
            return Ok(());
        }
        generic_set(self.0.as_item_mut(), parts, node)
    }

    fn delete(&mut self, parts: &[&str]) -> StorageResult<Option<Self::Node>> {
        if parts.is_empty() {
            return Ok(None);
        }
        generic_delete(self.0.as_item_mut(), parts)
    }

    fn scan(&self, parts: &[&str]) -> Vec<(String, Self::Node)> {
        generic_scan(self.0.as_item(), parts)
    }

    fn parse(src: &str) -> StorageResult<Self> {
        let doc = src
            .parse::<toml_edit::DocumentMut>()
            .map_err(|e| CodecError::Toml(e.to_string()))
            .map_err(TextStoreError::from)?;

        Ok(TomlDocument(doc))
    }

    fn serialize(&self) -> StorageResult<String> {
        Ok(self.0.to_string())
    }

    fn empty() -> Self {
        TomlDocument(toml_edit::DocumentMut::new())
    }

    fn deserialize_node<T: DeserializeOwned>(node: &Self::Node) -> StorageResult<T> {
        let mut doc = toml_edit::DocumentMut::new();
        doc.as_table_mut().insert("val", node.clone());
        let s = doc.to_string();

        #[derive(serde::Deserialize)]
        struct Unwrap<T> {
            val: T,
        }
        let unwrapped: Unwrap<T> = toml_edit::de::from_str(&s)
            .map_err(|e| CodecError::Toml(e.to_string()))
            .map_err(TextStoreError::from)?;

        Ok(unwrapped.val)
    }

    fn serialize_node<T: Serialize>(value: &T) -> StorageResult<Self::Node> {
        #[derive(serde::Serialize)]
        struct Wrap<'a, T> {
            val: &'a T,
        }

        let s = toml_edit::ser::to_string(&Wrap { val: value })
            .map_err(|e| CodecError::Toml(e.to_string()))
            .map_err(TextStoreError::from)?;

        let doc = s
            .parse::<toml_edit::DocumentMut>()
            .map_err(|e| CodecError::Toml(e.to_string()))
            .map_err(TextStoreError::from)?;
        Ok(doc
            .as_table()
            .get("val")
            .cloned()
            .unwrap_or(toml_edit::Item::None))
    }

    fn node_to_bytes(node: &Self::Node) -> StorageResult<Vec<u8>> {
        let mut doc = toml_edit::DocumentMut::new();
        doc.as_table_mut().insert("val", node.clone());
        Ok(doc.to_string().into_bytes())
    }

    fn bytes_to_node(bytes: &[u8]) -> StorageResult<Self::Node> {
        let s = std::str::from_utf8(bytes)
            .map_err(|e| CodecError::Toml(e.to_string()))
            .map_err(TextStoreError::from)?;

        let doc = s
            .parse::<toml_edit::DocumentMut>()
            .map_err(|e| CodecError::Toml(e.to_string()))
            .map_err(TextStoreError::from)?;

        Ok(doc
            .as_table()
            .get("val")
            .cloned()
            .unwrap_or(toml_edit::Item::None))
    }
}
