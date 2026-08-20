use crate::observability::register_field;
use crate::store::StorageError;
use crate::store::StorageResult;
use crate::{
    Field, ReactiveMap, StateScope, Store, StoreBackend, StoreOp, StoreSubscription,
    SubscriptionKind,
};
use crate::{ReactiveMapKey, ReactiveMapValue};
use amethystate_core::path::{IntoStorePath, StorePath};
use amethystate_core::{AccessMode, FieldCore, MapChange, ReactiveMapCore, Signal, WritableMode};
use error_stack::{Report, ResultExt};
use serde::Serialize;
use serde::de::DeserializeOwned;
use std::collections::HashMap;
use std::sync::Arc;
use uuid::Uuid;

/// A field under `TScope`'s path, at the levels `key` names.
pub fn field<TScope, TValue>(
    store: &Store,
    key: impl IntoStorePath,
    default: TValue,
    instance_id: Uuid,
) -> StorageResult<Field<TValue, WritableMode>>
where
    TScope: StateScope,
    TValue: Serialize + DeserializeOwned + Default + Clone + Send + Sync + 'static,
{
    let path = TScope::PATH.join(&crate::store::to_path(key)?);
    field_with_path(store, path, default, instance_id)
}

pub fn field_with_path<TValue, M>(
    store: &Store,
    path: impl IntoStorePath,
    default: TValue,
    instance_id: Uuid,
) -> StorageResult<Field<TValue, M>>
where
    TValue: Serialize + DeserializeOwned + Default + Clone + Send + Sync + 'static,
    M: AccessMode,
{
    let path = crate::store::to_path(path)?;

    register_field(
        Arc::from(path.as_str()),
        instance_id,
        std::any::type_name::<TValue>(),
    );

    if store.get::<TValue>(&path)?.is_none() {
        store.set(&path, &default)?;
    }

    let current = store
        .get::<TValue>(&path)?
        .unwrap_or_else(|| default.clone());
    let signal = Signal::new(current);

    let sig_clone = signal.clone();
    let store_clone = store.clone();
    let path_log: Arc<str> = Arc::from(path.as_str());
    let on_delete = default.clone();

    let id = store.subscribe(
        SubscriptionKind::ExactPath(Arc::from(path.as_str())),
        Arc::new(move |event| match &event.new {
            Some(raw) => match store_clone.decode::<TValue>(raw) {
                Ok(parsed) => sig_clone.set_forwarded(parsed, event.source),
                Err(e) => tracing::error!(path = %path_log, error = %e, "decode failed"),
            },
            // The key is gone - from `delete`, or from an edit to the file
            // outside the process. Reporting the default is what the next
            // startup would read, and beats holding a value the store no
            // longer has.
            None => sig_clone.set_forwarded(on_delete.clone(), event.source),
        }),
    );

    Ok(Field {
        inner: Arc::new(crate::reactive::field::FieldInner {
            core: FieldCore::new_with_signal(signal),
            path,
            instance_id,
            store_sub: Some(Arc::new(StoreSubscription {
                store: store.clone(),
                id,
            })),
        }),
        _mode: std::marker::PhantomData,
    })
}

/// A map under `TScope`'s path, at the levels `key` names.
pub fn reactive_map<TScope, K, V>(
    store: &Store,
    key: impl IntoStorePath,
    default: HashMap<K, V>,
    instance_id: Uuid,
) -> StorageResult<ReactiveMap<K, V, WritableMode>>
where
    TScope: StateScope,
    K: ReactiveMapKey,
    V: ReactiveMapValue,
{
    let path = TScope::PATH.join(&crate::store::to_path(key)?);
    reactive_map_with_path::<TScope, _, _, _>(store, path, default, instance_id)
}

pub fn reactive_map_with_path<TScope, K, V, M>(
    store: &Store,
    path: impl IntoStorePath,
    defaults: HashMap<K, V>,
    instance_id: Uuid,
) -> StorageResult<ReactiveMap<K, V, M>>
where
    TScope: StateScope,
    K: ReactiveMapKey,
    V: ReactiveMapValue,
    M: AccessMode,
{
    reactive_map_with_path_only(store, path, defaults, instance_id)
}

/// Every entry stored under `path`, keyed by the level below it.
///
/// A key under this path that cannot be read back is an error rather than an
/// absence: the scan was asked what is here, and answering short means the
/// caller acts on a map that is missing an entry the store holds.
///
/// The path itself is not one of them. The text engines leave an empty node
/// behind where a map was cleared and a scan reports it, but a map's entries
/// are the level below its path and nothing is stored at the path.
pub fn load_map<K, V>(store: &Store, path: &StorePath) -> StorageResult<HashMap<K, V>>
where
    K: ReactiveMapKey,
    V: ReactiveMapValue,
{
    let mut entries = HashMap::new();

    let scanned = store
        .scan_prefix(path)
        .attach_with(|| format!("map: {path}"))?;

    for (stored, bytes) in scanned {
        let name = match path.entry_name(&stored) {
            Some(name) => name,
            None if StorePath::parse_joined(&stored).is_ok_and(|key| &key == path) => continue,
            None => {
                return Err(Report::new(StorageError::Path)
                    .attach(format!("map: {path}"))
                    .attach(format!("stored key: {stored}"))
                    .attach("the key is not a path this library could have written"));
            }
        };

        let key = K::from_str(&name).map_err(|_| {
            Report::new(StorageError::Codec)
                .attach(format!("map: {path}"))
                .attach(format!("entry: {name}"))
                .attach(format!("key type: {}", std::any::type_name::<K>()))
        })?;

        let value = store
            .decode::<V>(&bytes)
            .attach_with(|| format!("map: {path}"))
            .attach_with(|| format!("entry: {name}"))?;

        entries.insert(key, value);
    }

    Ok(entries)
}

pub fn reactive_map_with_path_only<K, V, M>(
    store: &Store,
    path: impl IntoStorePath,
    defaults: HashMap<K, V>,
    instance_id: Uuid,
) -> StorageResult<ReactiveMap<K, V, M>>
where
    K: ReactiveMapKey,
    V: ReactiveMapValue,
    M: AccessMode,
{
    let path = crate::store::to_path(path)?;
    let mut known_cache = load_map::<K, V>(store, &path)?;

    // Keyed on this map's own path, not on the scope. A scope is marked
    // initialized once, when its struct is first built, so a map added to that
    // struct later never seeded its defaults for anyone already running - it
    // came up empty with nothing to say why.
    //
    // Keys already on disk count as having been seeded, so upgrading does not
    // restore entries the user has since removed.
    let seeded_before = store.is_initialized(path.as_str())? || !known_cache.is_empty();

    if !seeded_before {
        for (k, v) in defaults {
            let full_path = path
                .try_push(k.to_string())
                .change_context(StorageError::Path)
                .attach_with(|| format!("map: {path}, default key: {k}"))?;
            store.set(&full_path, &v)?;
            known_cache.insert(k, v);
        }
    }
    store.mark_initialized(path.as_str())?;

    let core = ReactiveMapCore::new();
    for (k, v) in known_cache {
        core.cache.insert(k, v);
    }

    let core_clone = core.clone();
    let prefix_for_strip = format!("{}.", path);
    let path_str: Arc<str> = Arc::from(path.as_str());
    let path_for_keys = path.clone();
    let store_clone = store.clone();
    let path_for_sub: Arc<str> = Arc::from(path.as_str());

    let id = store.subscribe(
        SubscriptionKind::Prefix(path_for_sub),
        Arc::new(move |event| {
            if event.op == StoreOp::DeletePrefix
                && (*event.path == *prefix_for_strip || *event.path == *path_str)
            {
                core_clone.cache.clear();
                core_clone.notify(&MapChange::Clear {
                    source: event.source,
                });
                return;
            }

            let Some(key_str) = path_for_keys.entry_name(&event.path) else {
                tracing::error!(
                    path = %event.path,
                    map = %path_for_keys,
                    "a key under this map is not a path this library could have written, so the change was not applied"
                );
                return;
            };

            let Ok(k) = K::from_str(&key_str) else {
                tracing::error!(
                    path = %event.path,
                    map = %path_for_keys,
                    key_type = std::any::type_name::<K>(),
                    "a key under this map does not parse as its key type, so the change was not applied"
                );
                return;
            };

            {
                let source = event.source;

                let new_val = match event.new.as_ref().map(|b| store_clone.decode::<V>(b)) {
                    Some(Ok(value)) => Some(value),
                    Some(Err(e)) => {
                        tracing::error!(
                            path = %event.path,
                            "a map entry cannot be read as this map's value type, so the map kept what it had: {e:?}"
                        );
                        return;
                    }
                    None => None,
                };

                let decoded_old = match event.old.as_ref().map(|b| store_clone.decode::<V>(b)) {
                    Some(Ok(value)) => Some(value),
                    Some(Err(e)) => {
                        tracing::warn!(
                            path = %event.path,
                            "the value being replaced could not be read, so subscribers are told what this map had: {e:?}"
                        );
                        None
                    }
                    None => None,
                };

                let old_val =
                    decoded_old.or_else(|| core_clone.cache.get(&k).map(|v| v.clone()));

                let change = {
                    let keys = &core_clone.cache;

                    match event.op {
                        StoreOp::Set => {
                            let Some(new_value) = new_val else {
                                tracing::error!(
                                    path = %event.path,
                                    "a set carried no value, so the map kept what it had"
                                );
                                return;
                            };

                            if keys.contains_key(&k) {
                                let old_value = old_val.unwrap_or_default();
                                keys.insert(k.clone(), new_value.clone());
                                MapChange::Update {
                                    key: k.clone(),
                                    old_value,
                                    new_value,
                                    source,
                                }
                            } else {
                                keys.insert(k.clone(), new_value.clone());
                                MapChange::Insert {
                                    key: k.clone(),
                                    value: new_value,
                                    source,
                                }
                            }
                        }
                        StoreOp::Delete | StoreOp::DeletePrefix => {
                            keys.remove(&k);
                            MapChange::Remove {
                                key: k.clone(),
                                old_value: old_val.unwrap_or_default(),
                                source,
                            }
                        }
                    }
                };

                core_clone.notify(&change);
            }
        }),
    );

    Ok(ReactiveMap {
        inner: Arc::new(crate::reactive::map::MapInner {
            core,
            path,
            instance_id,
            store: store.clone(),
            store_sub: Arc::new(StoreSubscription {
                store: store.clone(),
                id,
            }),
        }),
        _mode: std::marker::PhantomData,
    })
}
