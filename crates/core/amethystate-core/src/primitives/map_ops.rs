use crate::AmeBackendSync;
use crate::path::StorePath;
use crate::primitives::error::WriteError;
use crate::primitives::error::{ReactiveMapError, ReactiveMapResult};
use crate::primitives::map_core::{MapEntryPath, ReactiveMapKey, ReactiveMapValue};
use crate::{MapChange, ReactiveMapCore};
use error_stack::{Report, ResultExt};
use std::borrow::Borrow;

use serde::de::DeserializeOwned;
use std::fmt::Display;
use std::str::FromStr;
use uuid::Uuid;

pub fn map_get<B, K, V>(backend: &B, path: &StorePath, key: &K) -> ReactiveMapResult<Option<V>>
where
    B: AmeBackendSync,
    K: Display,
    V: DeserializeOwned,
{
    let entry = path.entry(key)?;

    backend
        .get(&entry)
        .change_context(WriteError::Storage)
        .attach_with(|| format!("reading map entry: {entry}"))
}

pub fn map_contains_key<B, K, V>(backend: &B, path: &StorePath, key: &K) -> ReactiveMapResult<bool>
where
    B: AmeBackendSync,
    K: Display,
    V: DeserializeOwned,
{
    map_get::<B, K, V>(backend, path, key).map(|v| v.is_some())
}

pub fn map_entries<B, K, V>(backend: &B, path: &StorePath) -> ReactiveMapResult<Vec<(K, V)>>
where
    B: AmeBackendSync,
    K: FromStr,
    V: DeserializeOwned + Default,
{
    let kvs = backend
        .scan_prefix(path)
        .change_context(WriteError::Storage)
        .attach_with(|| format!("scanning map: {path}"))?;

    let mut results = Vec::new();

    for (full_path, raw) in kvs {
        let Some(key_str) = full_path
            .strip_prefix(path)
            .as_ref()
            .and_then(StorePath::name)
            .map(str::to_string)
        else {
            continue;
        };
        let Ok(key) = K::from_str(&key_str) else {
            continue;
        };

        let value = backend
            .decode::<V>(raw.borrow())
            .change_context(WriteError::Storage)
            .attach_with(|| format!("map: {path}"))
            .attach_with(|| format!("entry: {key_str}"))?;

        results.push((key, value));
    }

    Ok(results)
}

pub fn map_len<B>(backend: &B, path: &StorePath) -> ReactiveMapResult<usize>
where
    B: AmeBackendSync,
{
    backend
        .scan_prefix(path)
        .map(|kvs| kvs.len())
        .change_context(WriteError::Storage)
        .attach_with(|| format!("scanning map: {path}"))
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
) -> ReactiveMapResult<()>
where
    B: AmeBackendSync,
    K: ReactiveMapKey,
    V: ReactiveMapValue,
{
    let full_path = path.entry(&key)?;
    let old_value = match read_entry::<B, V>(backend, &full_path)? {
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
) -> ReactiveMapResult<()>
where
    B: AmeBackendSync,
    K: ReactiveMapKey,
    V: ReactiveMapValue,
{
    let full_path = path.entry(&key)?;
    let old_value = read_entry::<B, V>(backend, &full_path)?;
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
) -> ReactiveMapResult<Option<V>>
where
    B: AmeBackendSync,
    K: ReactiveMapKey,
    V: ReactiveMapValue,
{
    let exists = core.cache.contains_key(&key);
    if !exists {
        return Ok(None);
    }

    let full_path = path.entry(&key)?;
    let old_value = read_entry::<B, V>(backend, &full_path)?;
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

fn read_entry<B, V>(backend: &B, entry: &StorePath) -> ReactiveMapResult<Option<V>>
where
    B: AmeBackendSync,
    V: DeserializeOwned,
{
    backend
        .get::<V>(entry)
        .change_context(WriteError::Storage)
        .attach_with(|| format!("reading map entry: {entry}"))
}

pub fn map_clear<B, K, V>(
    backend: &B,
    core: &ReactiveMapCore<K, V>,
    path: StorePath,
    source: Option<Uuid>,
) -> ReactiveMapResult<()>
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
) -> ReactiveMapResult<()>
where
    B: AmeBackendSync,
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

    match &processed {
        MapChange::Insert { key, value, .. }
        | MapChange::Update {
            key,
            new_value: value,
            ..
        } => {
            let entry = path.entry(key)?;
            backend
                .set_with_source(&entry, value, processed.source())
                .change_context(WriteError::Storage)
                .attach_with(|| format!("writing map entry: {entry}"))?;
        }
        MapChange::Remove { key, .. } => {
            let entry = path.entry(key)?;
            backend
                .delete_with_source(&entry, processed.source())
                .change_context(WriteError::Storage)
                .attach_with(|| format!("removing map entry: {entry}"))?;
        }
        MapChange::Clear { .. } => {
            backend
                .delete_prefix(&path, processed.source())
                .change_context(WriteError::Storage)
                .attach_with(|| format!("clearing map: {path}"))?;
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
