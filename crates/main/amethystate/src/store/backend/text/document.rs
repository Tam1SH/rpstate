use crate::store::CodecFormat;
use crate::store::StorageResult;
use serde::Serialize;
use serde::de::DeserializeOwned;
use std::fmt::Debug;

pub trait TextDocument: Send + Sync + Sized + Clone + 'static {
    type Node: Clone + Debug;
    fn format() -> CodecFormat;

    fn get(&self, parts: &[&str]) -> Option<&Self::Node>;
    fn set(&mut self, parts: &[&str], node: Self::Node) -> StorageResult<()>;
    fn delete(&mut self, parts: &[&str]) -> StorageResult<Option<Self::Node>>;
    fn scan(&self, parts: &[&str]) -> Vec<(String, Self::Node)>;
    fn parse(src: &str) -> StorageResult<Self>;
    fn serialize(&self) -> StorageResult<String>;
    fn empty() -> Self;
    fn deserialize_node<T: DeserializeOwned>(node: &Self::Node) -> StorageResult<T>;
    fn serialize_node<T: Serialize>(value: &T) -> StorageResult<Self::Node>;
    fn node_to_bytes(node: &Self::Node) -> StorageResult<Vec<u8>>;
    fn bytes_to_node(bytes: &[u8]) -> StorageResult<Self::Node>;
}

pub trait Navigable: Sized + Clone {
    fn make_empty_map() -> Self;
    fn get_child(&self, key: &str) -> Option<&Self>;
    fn get_child_mut(&mut self, key: &str) -> Option<&mut Self>;
    fn ensure_map(&mut self);
    fn insert_child(&mut self, key: &str, val: Self);
    fn remove_child(&mut self, key: &str) -> Option<Self>;
    fn scan_children(&self) -> Vec<(String, Self)>;
}

/// Normalises `parts` so that `["."]` (the global-namespace sentinel produced
/// by `split_path(".")`) is treated identically to `[]` (the empty path that
/// means "the whole document root").
#[inline]
fn normalise_parts<'a>(parts: &'a [&'a str]) -> &'a [&'a str] {
    if parts == ["."] { &[] } else { parts }
}

pub fn generic_get<'a, N: Navigable>(root: &'a N, parts: &[&str]) -> Option<&'a N> {
    let parts = normalise_parts(parts);

    if parts.is_empty() {
        return Some(root);
    }

    let mut current = root;
    for part in parts {
        current = current.get_child(part)?;
    }
    Some(current)
}

pub fn generic_set<N: Navigable>(root: &mut N, parts: &[&str], node: N) -> StorageResult<()> {
    let parts = normalise_parts(parts);

    if parts.is_empty() {
        *root = node;
        return Ok(());
    }
    let (last, heads) = parts.split_last().unwrap();
    let mut current = root;
    for &part in heads {
        current.ensure_map();
        if current.get_child(part).is_none() {
            current.insert_child(part, N::make_empty_map());
        }
        current = current.get_child_mut(part).unwrap();
    }
    current.ensure_map();
    current.insert_child(last, node);
    Ok(())
}

pub fn generic_delete<N: Navigable>(root: &mut N, parts: &[&str]) -> StorageResult<Option<N>> {
    let parts = normalise_parts(parts);

    if parts.is_empty() {
        return Ok(None);
    }
    let (last, heads) = parts.split_last().unwrap();
    let mut current = root;
    for &part in heads {
        if let Some(next) = current.get_child_mut(part) {
            current = next;
        } else {
            return Ok(None);
        }
    }
    Ok(current.remove_child(last))
}

pub fn generic_scan<N: Navigable>(root: &N, parts: &[&str]) -> Vec<(String, N)> {
    let parts = normalise_parts(parts);

    let mut results = Vec::new();
    let prefix_str = parts.join(".");

    let node = if parts.is_empty() {
        Some(root)
    } else {
        generic_get(root, parts)
    };

    if let Some(node) = node {
        for (k, v) in node.scan_children() {
            let full_key = if prefix_str.is_empty() {
                k
            } else {
                format!("{}.{}", prefix_str, k)
            };
            results.push((full_key, v));
        }
    }
    results
}
