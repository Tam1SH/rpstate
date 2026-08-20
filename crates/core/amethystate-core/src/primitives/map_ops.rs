use crate::AmeBackendSync;
use crate::path::StorePath;
use crate::primitives::error::WriteError;
use crate::primitives::error::{ReactiveMapError, ReactiveMapResult};
use crate::primitives::map_core::{ReactiveMapKey, ReactiveMapValue};
use crate::{MapChange, ReactiveMapCore};
use std::borrow::Borrow;

use serde::de::DeserializeOwned;
use std::fmt::Display;
use std::str::FromStr;
use uuid::Uuid;

pub fn map_get<B, K, V>(
    backend: &B,
    path: &StorePath,
    key: &K,
) -> ReactiveMapResult<Option<V>, B::Error>
where
    B: AmeBackendSync,
    K: Display,
    V: DeserializeOwned,
{
    Ok(backend.get(&path.try_push(key.to_string()).map_err(WriteError::Path)?)?)
}

pub fn map_contains_key<B, K, V>(
    backend: &B,
    path: &StorePath,
    key: &K,
) -> ReactiveMapResult<bool, B::Error>
where
    B: AmeBackendSync,
    K: Display,
    V: DeserializeOwned,
{
    map_get::<B, K, V>(backend, path, key).map(|v| v.is_some())
}

pub fn map_entries<B, K, V>(
    backend: &B,
    path: &StorePath,
) -> ReactiveMapResult<Vec<(K, V)>, B::Error>
where
    B: AmeBackendSync,
    K: FromStr,
    V: DeserializeOwned + Default,
{
    let kvs = backend.scan_prefix(path)?;

    let mut results = Vec::new();

    for (full_path, raw) in kvs {
        if let Some(key_str) = path.entry_name(&full_path)
            && let Ok(k) = K::from_str(&key_str)
            && let Ok(v) = backend.decode::<V>(raw.borrow())
        {
            results.push((k, v));
        }
    }

    Ok(results)
}

pub fn map_len<B>(backend: &B, path: &StorePath) -> ReactiveMapResult<usize, B::Error>
where
    B: AmeBackendSync,
{
    Ok(backend.scan_prefix(path).map(|kvs| kvs.len())?)
}

/// Writes a key that already exists, and fails with
/// [`ReactiveMapError::KeyNotFound`] otherwise.
///
/// The old value is read first because [`MapChange::Update`] carries it to
/// subscribers; a key that does not exist has none to carry.
pub fn map_update<B, K, V>(
    backend: &B,
    core: &ReactiveMapCore<K, V>,
    path: StorePath,
    key: K,
    value: &V,
    source: Option<Uuid>,
) -> ReactiveMapResult<(), B::Error>
where
    B: AmeBackendSync,
    K: ReactiveMapKey,
    V: ReactiveMapValue,
{
    let full_path = path.try_push(key.to_string()).map_err(WriteError::Path)?;
    let old_value = match backend.get::<V>(&full_path)? {
        Some(old_value) => old_value,
        None => return Err(ReactiveMapError::KeyNotFound(key.to_string())),
    };

    let change = MapChange::Update {
        key,
        old_value,
        new_value: value.clone(),
        source,
    };

    map_apply_change(backend, core, path, change)
}

/// Writes a key whether or not it exists, emitting [`MapChange::Insert`] for
/// a new one and [`MapChange::Update`] for one that was already there.
pub fn map_insert<B, K, V>(
    backend: &B,
    core: &ReactiveMapCore<K, V>,
    path: StorePath,
    key: K,
    value: &V,
    source: Option<Uuid>,
) -> ReactiveMapResult<(), B::Error>
where
    B: AmeBackendSync,
    K: ReactiveMapKey,
    V: ReactiveMapValue,
{
    let full_path = path.try_push(key.to_string()).map_err(WriteError::Path)?;
    let old_value = backend.get::<V>(&full_path)?;
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

    map_apply_change(backend, core, path, change)
}

pub fn map_remove<B, K, V>(
    backend: &B,
    core: &ReactiveMapCore<K, V>,
    path: StorePath,
    key: K,
    source: Option<Uuid>,
) -> ReactiveMapResult<Option<V>, B::Error>
where
    B: AmeBackendSync,
    K: ReactiveMapKey,
    V: ReactiveMapValue,
{
    let exists = core.cache.contains_key(&key);
    if !exists {
        return Ok(None);
    }

    let full_path = path.try_push(key.to_string()).map_err(WriteError::Path)?;
    let old_value = backend.get::<V>(&full_path)?;
    if let Some(old_value) = old_value {
        let change = MapChange::Remove {
            key,
            old_value: old_value.clone(),
            source,
        };
        map_apply_change(backend, core, path, change)?;
        Ok(Some(old_value))
    } else {
        core.cache.remove(&key);
        Ok(None)
    }
}

pub fn map_clear<B, K, V>(
    backend: &B,
    core: &ReactiveMapCore<K, V>,
    path: StorePath,
    source: Option<Uuid>,
) -> ReactiveMapResult<(), B::Error>
where
    B: AmeBackendSync,
    K: ReactiveMapKey,
    V: ReactiveMapValue,
{
    map_apply_change(backend, core, path, MapChange::Clear { source })
}

pub fn map_apply_change<B, K, V>(
    backend: &B,
    core: &ReactiveMapCore<K, V>,
    path: StorePath,
    change: MapChange<K, V>,
) -> ReactiveMapResult<(), B::Error>
where
    B: AmeBackendSync,
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

    match &processed {
        MapChange::Insert { key, value, .. }
        | MapChange::Update {
            key,
            new_value: value,
            ..
        } => {
            backend.set_with_source(
                &path.try_push(key.to_string()).map_err(WriteError::Path)?,
                value,
                processed.source(),
            )?;
        }
        MapChange::Remove { key, .. } => {
            backend.delete_with_source(
                &path.try_push(key.to_string()).map_err(WriteError::Path)?,
                processed.source(),
            )?;
        }
        MapChange::Clear { .. } => {
            backend.delete_prefix(&path, processed.source())?;
        }
    }

    // Subscribers are told by the backend's subscription, not from here. One
    // notifier means a change is reported once and always says what the store
    // actually took, rather than what was asked for.
    map_apply_remote_change(core, &processed);

    Ok(())
}

pub fn map_apply_remote_change<K, V>(core: &ReactiveMapCore<K, V>, change: &MapChange<K, V>)
where
    K: ReactiveMapKey,
    V: ReactiveMapValue,
{
    let keys = &core.cache;
    match change {
        MapChange::Insert { key, value, .. }
        | MapChange::Update {
            key,
            new_value: value,
            ..
        } => {
            keys.insert(key.clone(), value.clone());
        }
        MapChange::Remove { key, .. } => {
            keys.remove(key);
        }
        MapChange::Clear { .. } => {
            keys.clear();
        }
    }
}
