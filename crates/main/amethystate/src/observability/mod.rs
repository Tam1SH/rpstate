mod inspector_trait;
mod scheme;
pub use inspector_trait::*;

pub use scheme::*;

use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use uuid::Uuid;

pub fn short_type_name(full: &str) -> &str {
    full.rsplit("::").next().unwrap_or(full)
}

#[derive(Debug, Clone)]
pub struct FieldMeta {
    pub struct_type_name: &'static str,
    pub field_name: Arc<str>,
    pub value_type_name: &'static str,
}

static INSTANCE_REGISTRY: std::sync::LazyLock<RwLock<HashMap<Uuid, &'static str>>> =
    std::sync::LazyLock::new(|| RwLock::new(HashMap::new()));

static SCHEMA_REGISTRY: std::sync::LazyLock<RwLock<HashMap<Arc<str>, FieldMeta>>> =
    std::sync::LazyLock::new(|| RwLock::new(HashMap::new()));

pub fn register_instance(id: Uuid, struct_type_name: &'static str) {
    if let Ok(mut map) = INSTANCE_REGISTRY.write() {
        map.insert(id, struct_type_name);
    }
}

pub fn deregister_instance(id: Uuid) {
    if let Ok(mut map) = INSTANCE_REGISTRY.write() {
        map.remove(&id);
    }
}

pub fn resolve_instance(id: Uuid) -> Option<&'static str> {
    INSTANCE_REGISTRY.read().ok()?.get(&id).copied()
}

pub fn resolve_instance_short(id: Uuid) -> Option<&'static str> {
    resolve_instance(id).map(short_type_name)
}

pub fn register_field(path: Arc<str>, instance_id: Uuid, value_type_name: &'static str) {
    let struct_type_name = match resolve_instance(instance_id) {
        Some(n) => n,
        None => return,
    };
    let field_name: Arc<str> = Arc::from(path.rsplit('.').next().unwrap_or(path.as_ref()));
    if let Ok(mut map) = SCHEMA_REGISTRY.write() {
        map.entry(Arc::clone(&path)).or_insert(FieldMeta {
            struct_type_name,
            field_name,
            value_type_name,
        });
    }
}

pub fn resolve_field(path: &str) -> Option<FieldMeta> {
    SCHEMA_REGISTRY.read().ok()?.get(path).cloned()
}
