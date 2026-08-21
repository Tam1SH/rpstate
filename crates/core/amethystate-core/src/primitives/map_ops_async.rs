use crate::AmeBackendAsync as AmeBackend;
use crate::path::StorePath;
use crate::primitives::error::WriteError;
use crate::primitives::error::{ReactiveMapError, ReactiveMapResult};
use crate::primitives::map_core::{MapEntryPath, ReactiveMapKey, ReactiveMapValue};
use crate::{MapChange, ReactiveMapCore, map_apply_remote_change};
use error_stack::{Report, ResultExt};
use uuid::Uuid;

use serde::de::DeserializeOwned;
use std::fmt::Display;
use std::str::FromStr;

async fn read_entry<B, V>(backend: &B, entry: &StorePath) -> ReactiveMapResult<Option<V>>
where
    B: AmeBackend,
    V: DeserializeOwned,
{
    backend
        .get::<V>(entry)
        .await
        .change_context(WriteError::Storage)
        .attach_with(|| format!("reading map entry: {entry}"))
}

pub async fn map_get_async<B, K, V>(
    backend: &B,
    path: &StorePath,
    key: &K,
) -> ReactiveMapResult<Option<V>>
where
    B: AmeBackend,
    K: Display,
    V: DeserializeOwned,
{
    let entry = path.entry(key)?;
    read_entry::<B, V>(backend, &entry).await
}

pub async fn map_contains_key_async<B, K, V>(
    backend: &B,
    path: &StorePath,
    key: &K,
) -> ReactiveMapResult<bool>
where
    B: AmeBackend,
    K: Display,
    V: DeserializeOwned,
{
    map_get_async::<B, K, V>(backend, path, key)
        .await
        .map(|v| v.is_some())
}

pub async fn map_entries_async<B, K, V>(
    backend: &B,
    path: &StorePath,
) -> ReactiveMapResult<Vec<(K, V)>>
where
    B: AmeBackend,
    K: FromStr,
    V: DeserializeOwned + Default,
{
    let kvs = backend
        .scan_prefix(path)
        .await
        .change_context(WriteError::Storage)
        .attach_with(|| format!("scanning map: {path}"))?;
    let mut results = Vec::new();

    for (full_path, raw) in kvs {
        let Some(key_str) = path.entry_name(&full_path) else {
            continue;
        };
        let Ok(key) = K::from_str(&key_str) else {
            continue;
        };

        let value = backend
            .decode::<V>(&raw)
            .change_context(WriteError::Storage)
            .attach_with(|| format!("map: {path}"))
            .attach_with(|| format!("entry: {key_str}"))?;

        results.push((key, value));
    }

    Ok(results)
}

pub async fn map_len_async<B>(backend: &B, path: &StorePath) -> ReactiveMapResult<usize>
where
    B: AmeBackend,
{
    backend
        .scan_prefix(path)
        .await
        .map(|kvs| kvs.len())
        .change_context(WriteError::Storage)
        .attach_with(|| format!("scanning map: {path}"))
}

pub async fn map_update_async<B, K, V>(
    backend: &B,
    core: &ReactiveMapCore<K, V>,
    path: StorePath,
    key: K,
    value: &V,
    source: Option<Uuid>,
) -> ReactiveMapResult<()>
where
    B: AmeBackend,
    K: ReactiveMapKey,
    V: ReactiveMapValue,
{
    let full_path = path.entry(&key)?;
    let old_value = match read_entry::<B, V>(backend, &full_path).await? {
        Some(old_value) => old_value,
        None => {
            return Err(Report::new(ReactiveMapError::KeyNotFound(key.to_string()))
                .attach(format!("map: {path}")));
        }
    };

    let change = MapChange::Update {
        key,
        old_value,
        new_value: value.clone(),
        source,
    };

    map_apply_change_async(backend, core, path, change).await
}

pub async fn map_insert_async<B, K, V>(
    backend: &B,
    core: &ReactiveMapCore<K, V>,
    path: StorePath,
    key: K,
    value: &V,
    source: Option<Uuid>,
) -> ReactiveMapResult<()>
where
    B: AmeBackend,
    K: ReactiveMapKey,
    V: ReactiveMapValue,
{
    let full_path = path.entry(&key)?;
    let old_value = read_entry::<B, V>(backend, &full_path).await?;
    let change = if let Some(old_value) = old_value {
        MapChange::Update {
            key,
            old_value,
            new_value: value.clone(),
            source,
        }
    } else {
        MapChange::Insert {
            key,
            value: value.clone(),
            source,
        }
    };

    map_apply_change_async(backend, core, path, change).await
}

pub async fn map_remove_async<B, K, V>(
    backend: &B,
    core: &ReactiveMapCore<K, V>,
    path: StorePath,
    key: K,
    source: Option<Uuid>,
) -> ReactiveMapResult<Option<V>>
where
    B: AmeBackend,
    K: ReactiveMapKey,
    V: ReactiveMapValue,
{
    let exists = core.cache.contains_key(&key);
    if !exists {
        return Ok(None);
    }

    let full_path = path.entry(&key)?;
    let old_value = read_entry::<B, V>(backend, &full_path).await?;
    if let Some(old_value) = old_value {
        let change = MapChange::Remove {
            key,
            old_value: old_value.clone(),
            source,
        };
        map_apply_change_async(backend, core, path, change).await?;
        Ok(Some(old_value))
    } else {
        core.cache.remove(&key);
        Ok(None)
    }
}

pub async fn map_clear_async<B, K, V>(
    backend: &B,
    core: &ReactiveMapCore<K, V>,
    path: StorePath,
    source: Option<Uuid>,
) -> ReactiveMapResult<()>
where
    B: AmeBackend,
    K: ReactiveMapKey,
    V: ReactiveMapValue,
{
    map_apply_change_async(backend, core, path, MapChange::Clear { source }).await
}

pub async fn map_apply_change_async<B, K, V>(
    backend: &B,
    core: &ReactiveMapCore<K, V>,
    path: StorePath,
    change: MapChange<K, V>,
) -> ReactiveMapResult<()>
where
    B: AmeBackend,
    K: ReactiveMapKey,
    V: ReactiveMapValue,
{
    let subject = match change.key() {
        Some(key) => Some(path.entry(key)?),
        None => None,
    };
    let context_path = subject.clone().unwrap_or_else(|| path.clone());

    let processed = core
        .run_interceptors(context_path, change)
        .map_err(ReactiveMapError::intercepted)
        .attach_with(|| format!("map: {path}"))
        .attach_with(|| match &subject {
            Some(entry) => format!("affects: {entry}"),
            None => format!("affects: all of {path}"),
        })?;

    // Writes carry the provenance the change came with, as the sync path does.
    // Without it a handle's own write comes back looking like somebody else's,
    // and anything answering external changes with a write of its own echoes.
    let source = processed.source();

    match &processed {
        MapChange::Insert { key, value, .. }
        | MapChange::Update {
            key,
            new_value: value,
            ..
        } => {
            let entry = path.entry(key)?;
            backend
                .set_with_source(&entry, value, source)
                .await
                .change_context(WriteError::Storage)
                .attach_with(|| format!("writing map entry: {entry}"))?;
        }
        MapChange::Remove { key, .. } => {
            let entry = path.entry(key)?;
            backend
                .delete_with_source(&entry, source)
                .await
                .change_context(WriteError::Storage)
                .attach_with(|| format!("removing map entry: {entry}"))?;
        }
        MapChange::Clear { .. } => {
            let kvs = backend
                .scan_prefix(&path)
                .await
                .change_context(WriteError::Storage)
                .attach_with(|| format!("clearing map: {path}"))?;
            for (full_path, _) in kvs {
                let key = StorePath::parse_joined(&full_path)
                    .change_context(WriteError::Path)
                    .attach_with(|| format!("stored key: {full_path}"))?;
                backend
                    .delete_with_source(&key, source)
                    .await
                    .change_context(WriteError::Storage)
                    .attach_with(|| format!("clearing map entry: {key}"))?;
            }
        }
    }

    // After the write, not before. Updating first meant a failure below left
    // the cache holding a value the backend never took, with nothing to undo
    // it - and values() and get_sync read the cache alone.
    //
    // Subscribers are told by the backend's subscription, as in the sync path;
    // notifying here as well delivered every change twice.
    map_apply_remote_change(core, &processed);

    Ok(())
}
