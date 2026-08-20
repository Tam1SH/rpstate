use crate::AmeBackendAsync as AmeBackend;
use crate::path::StorePath;
use crate::primitives::error::WriteError;
use crate::primitives::error::{ReactiveMapError, ReactiveMapResult};
use crate::primitives::map_core::{ReactiveMapKey, ReactiveMapValue};
use crate::{MapChange, ReactiveMapCore, map_apply_remote_change};
use uuid::Uuid;

use serde::de::DeserializeOwned;
use std::fmt::Display;
use std::str::FromStr;

pub async fn map_get_async<B, K, V>(
    backend: &B,
    path: &StorePath,
    key: &K,
) -> ReactiveMapResult<Option<V>, B::Error>
where
    B: AmeBackend,
    K: Display,
    V: DeserializeOwned,
{
    Ok(backend
        .get(&path.try_push(key.to_string()).map_err(WriteError::Path)?)
        .await?)
}

pub async fn map_contains_key_async<B, K, V>(
    backend: &B,
    path: &StorePath,
    key: &K,
) -> ReactiveMapResult<bool, B::Error>
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
) -> ReactiveMapResult<Vec<(K, V)>, B::Error>
where
    B: AmeBackend,
    K: FromStr,
    V: DeserializeOwned + Default,
{
    let kvs = backend.scan_prefix(path).await?;
    let mut results = Vec::new();

    for (full_path, raw) in kvs {
        if let Some(key_str) = path.entry_name(&full_path)
            && let Ok(k) = K::from_str(&key_str)
            && let Ok(v) = backend.decode::<V>(&raw)
        {
            results.push((k, v));
        }
    }

    Ok(results)
}

pub async fn map_len_async<B>(backend: &B, path: &StorePath) -> ReactiveMapResult<usize, B::Error>
where
    B: AmeBackend,
{
    Ok(backend.scan_prefix(path).await.map(|kvs| kvs.len())?)
}

pub async fn map_update_async<B, K, V>(
    backend: &B,
    core: &ReactiveMapCore<K, V>,
    path: StorePath,
    key: K,
    value: &V,
    source: Option<Uuid>,
) -> ReactiveMapResult<(), B::Error>
where
    B: AmeBackend,
    K: ReactiveMapKey,
    V: ReactiveMapValue,
{
    let full_path = path.try_push(key.to_string()).map_err(WriteError::Path)?;
    let old_value = match backend.get::<V>(&full_path).await? {
        Some(old_value) => old_value,
        None => return Err(ReactiveMapError::KeyNotFound(key.to_string())),
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
) -> ReactiveMapResult<(), B::Error>
where
    B: AmeBackend,
    K: ReactiveMapKey,
    V: ReactiveMapValue,
{
    let full_path = path.try_push(key.to_string()).map_err(WriteError::Path)?;
    let old_value = backend.get::<V>(&full_path).await?;
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
) -> ReactiveMapResult<Option<V>, B::Error>
where
    B: AmeBackend,
    K: ReactiveMapKey,
    V: ReactiveMapValue,
{
    let exists = core.cache.contains_key(&key);
    if !exists {
        return Ok(None);
    }

    let full_path = path.try_push(key.to_string()).map_err(WriteError::Path)?;
    let old_value = backend.get::<V>(&full_path).await?;
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
) -> ReactiveMapResult<(), B::Error>
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
) -> ReactiveMapResult<(), B::Error>
where
    B: AmeBackend,
    K: ReactiveMapKey,
    V: ReactiveMapValue,
{
    let context_path = match change.key() {
        Some(key) => path.try_push(key.to_string()).map_err(WriteError::Path)?,
        None => path.clone(),
    };

    let processed = core
        .run_interceptors(context_path, change)
        .map_err(|_| ReactiveMapError::Intercepted)?;

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
            backend
                .set_with_source(
                    &path.try_push(key.to_string()).map_err(WriteError::Path)?,
                    value,
                    source,
                )
                .await?;
        }
        MapChange::Remove { key, .. } => {
            backend
                .delete_with_source(
                    &path.try_push(key.to_string()).map_err(WriteError::Path)?,
                    source,
                )
                .await?;
        }
        MapChange::Clear { .. } => {
            let kvs = backend.scan_prefix(&path).await?;
            for (full_path, _) in kvs {
                let key = StorePath::parse_joined(&full_path).map_err(WriteError::Path)?;
                backend.delete_with_source(&key, source).await?;
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
