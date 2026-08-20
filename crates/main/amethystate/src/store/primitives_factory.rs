use crate::observability::register_field;
use crate::store::StorageResult;
use crate::{
    Field, ReactiveMap, StateScope, Store, StoreBackend, StoreOp, StoreSubscription,
    SubscriptionKind,
};
use crate::{ReactiveMapKey, ReactiveMapValue};
use amethystate_core::path::{IntoStorePath, StorePath};
use amethystate_core::{AccessMode, FieldCore, MapChange, ReactiveMapCore, Signal, WritableMode};
use serde::Serialize;
use serde::de::DeserializeOwned;
use std::collections::HashMap;
use std::sync::Arc;
use uuid::Uuid;

pub fn field<TScope, TValue>(
    store: &Store,
    key: &str,
    default: TValue,
    instance_id: Uuid,
) -> StorageResult<Field<TValue, WritableMode>>
where
    TScope: StateScope,
    TValue: Serialize + DeserializeOwned + Default + Clone + Send + Sync + 'static,
{
    let path = scoped_path::<TScope>(key);
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
    let path = path.into_store_path()?;

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

pub fn reactive_map<TScope, K, V>(
    store: &Store,
    key: &str,
    default: HashMap<K, V>,
    instance_id: Uuid,
) -> StorageResult<ReactiveMap<K, V, WritableMode>>
where
    TScope: StateScope,
    K: ReactiveMapKey,
    V: ReactiveMapValue,
{
    let path = scoped_path::<TScope>(key);
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
/// A stored key that does not read back as a path, or whose level does not
/// parse into `K`, is not an entry of this map and is left alone.
pub fn load_map<K, V>(store: &Store, path: &StorePath) -> StorageResult<HashMap<K, V>>
where
    K: ReactiveMapKey,
    V: ReactiveMapValue,
{
    let mut entries = HashMap::new();

    for (stored, bytes) in store.scan_prefix(path)? {
        if let Some(key) = path.entry_name(&stored)
            && let Ok(key) = K::from_str(&key)
        {
            entries.insert(key, store.decode::<V>(&bytes)?);
        }
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
    let path = path.into_store_path()?;
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
            let full_path = path.try_push(k.to_string())?;
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

            if let Some(key_str) = path_for_keys.entry_name(&event.path)
                && let Ok(k) = K::from_str(&key_str)
            {
                let source = event.source;

                let new_val = event
                    .new
                    .as_ref()
                    .and_then(|b| store_clone.decode::<V>(b).ok());
                let old_val = event
                    .old
                    .as_ref()
                    .and_then(|b| store_clone.decode::<V>(b).ok());

                let change = {
                    let keys = &core_clone.cache;

                    match event.op {
                        StoreOp::Set => {
                            if keys.contains_key(&k) {
                                let old_value = old_val.unwrap_or_default();
                                let new_value = new_val.unwrap_or_default();
                                keys.insert(k.clone(), new_value.clone());
                                MapChange::Update {
                                    key: k.clone(),
                                    old_value,
                                    new_value,
                                    source,
                                }
                            } else {
                                let val = new_val.unwrap_or_default();
                                keys.insert(k.clone(), val.clone());
                                MapChange::Insert {
                                    key: k.clone(),
                                    value: val,
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

/// Composes a path out of a prefix and a key, both of which spell their levels
/// with the separator.
///
/// The macro still writes both as one string, so this is where that spelling is
/// read: empty levels are dropped, which is what the trimming here always did.
pub fn join_path(prefix: &str, key: &str) -> StorePath {
    let segments: Vec<&str> = prefix
        .split('.')
        .chain(key.split('.'))
        .filter(|s| !s.is_empty())
        .collect();

    StorePath::from_segments(segments)
}

pub fn scoped_path<T: StateScope>(key: &str) -> StorePath {
    join_path(T::PREFIX, key)
}
