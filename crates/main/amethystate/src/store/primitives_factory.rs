use crate::observability::register_field;
use crate::store::StorageResult;
use crate::{Field, ReactiveMap, StateScope, Store, StoreOp, StoreSubscription, SubscriptionKind};
use amethystate_core::{AccessMode, FieldCore, MapChange, ReactiveMapCore, Signal, WritableMode};
use serde::Serialize;
use serde::de::DeserializeOwned;
use std::collections::HashMap;
use std::fmt::Display;
use std::hash::Hash;
use std::str::FromStr;
use std::sync::Arc;
use uuid::Uuid;

pub fn field<TScope, TValue, S>(
    store: &S,
    key: &str,
    default: TValue,
    instance_id: Uuid,
) -> StorageResult<Field<TValue, S, WritableMode>>
where
    TScope: StateScope,
    S: Store,
    TValue: Serialize + DeserializeOwned + Default + Clone + Send + Sync + 'static,
{
    let path: Arc<str> = scoped_path::<TScope>(key).into();
    field_with_path(store, path, default, instance_id)
}

pub fn field_with_path<TValue, S, M>(
    store: &S,
    path: Arc<str>,
    default: TValue,
    instance_id: Uuid,
) -> StorageResult<Field<TValue, S, M>>
where
    S: Store,
    TValue: Serialize + DeserializeOwned + Default + Clone + Send + Sync + 'static,
    M: AccessMode,
{
    register_field(
        Arc::clone(&path),
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
    let path_log = Arc::clone(&path);

    let id = store.subscribe(
        SubscriptionKind::ExactPath(path.clone()),
        Arc::new(move |event| {
            if let Some(raw) = &event.new {
                match store_clone.decode::<TValue>(raw) {
                    Ok(parsed) => sig_clone.set_forwarded(parsed, event.source),
                    Err(e) => tracing::error!(path = %path_log, error = %e, "decode failed"),
                }
            }
        }),
    );

    Ok(Field {
        core: FieldCore::new_with_signal(signal),
        path,
        instance_id,
        store_sub: Some(Arc::new(StoreSubscription {
            store: store.clone(),
            id,
        })),
        _mode: std::marker::PhantomData,
    })
}

pub fn reactive_map<TScope, K, V, S>(
    store: &S,
    key: &str,
    default: HashMap<K, V>,
    instance_id: Uuid,
) -> StorageResult<ReactiveMap<K, V, S, WritableMode>>
where
    TScope: StateScope,
    S: Store,
    K: FromStr + Display + Clone + Hash + Eq + Send + Sync + 'static,
    V: Serialize + DeserializeOwned + Default + Clone + Send + Sync + 'static,
{
    let path: Arc<str> = scoped_path::<TScope>(key).into();
    reactive_map_with_path::<TScope, _, _, _, _>(store, path, default, instance_id)
}

pub fn reactive_map_with_path<TScope, K, V, S, M>(
    store: &S,
    path: Arc<str>,
    defaults: HashMap<K, V>,
    instance_id: Uuid,
) -> StorageResult<ReactiveMap<K, V, S, M>>
where
    TScope: StateScope,
    S: Store,
    K: FromStr + Display + Clone + Hash + Eq + Send + Sync + 'static,
    V: Serialize + Default + DeserializeOwned + Clone + Send + Sync + 'static,
    M: AccessMode,
{
    reactive_map_with_scope_key(store, path, TScope::PREFIX, defaults, instance_id)
}

pub fn reactive_map_with_scope_key<K, V, S, M>(
    store: &S,
    path: Arc<str>,
    scope_key: &str,
    defaults: HashMap<K, V>,
    instance_id: Uuid,
) -> StorageResult<ReactiveMap<K, V, S, M>>
where
    S: Store,
    K: FromStr + Display + Clone + Hash + Eq + Send + Sync + 'static,
    V: Serialize + Default + DeserializeOwned + Clone + Send + Sync + 'static,
    M: AccessMode,
{
    let mut known_cache = HashMap::new();

    let prefix = format!("{}.", path);
    let existing = store.scan_prefix(&prefix)?;
    for (fpath, val) in existing {
        if let Some(k_str) = fpath.strip_prefix(&prefix)
            && let Ok(k) = K::from_str(k_str)
            && let Ok(v) = store.decode::<V>(&val)
        {
            known_cache.insert(k, v);
        }
    }

    if !store.is_initialized(scope_key)? {
        for (k, v) in defaults {
            let full_path = format!("{}.{}", path, k);
            store.set(&full_path, &v)?;
            known_cache.insert(k, v);
        }
    }

    let core = ReactiveMapCore::new();
    {
        let mut keys = core.cache.lock().unwrap();
        *keys = known_cache;
    }

    let core_clone = core.clone();
    let prefix_for_strip = format!("{}.", path);
    let store_clone = store.clone();
    let path_for_sub = path.clone();

    let id = store.subscribe(
        SubscriptionKind::Prefix(path_for_sub),
        Arc::new(move |event| {
            if let Some(key_str) = event.path.strip_prefix(&prefix_for_strip)
                && let Ok(k) = K::from_str(key_str)
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
                    let mut keys = core_clone.cache.lock().unwrap();

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
                        StoreOp::Delete => {
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
        core,
        path,
        instance_id,
        store: store.clone(),
        store_sub: Arc::new(StoreSubscription {
            store: store.clone(),
            id,
        }),
        _mode: std::marker::PhantomData,
    })
}

pub fn join_path(prefix: &str, key: &str) -> String {
    let trimmed_prefix = prefix.trim_end_matches('.');
    let trimmed_key = key.trim_start_matches('.');
    if trimmed_prefix.is_empty() {
        trimmed_key.to_string()
    } else {
        format!("{}.{}", trimmed_prefix, trimmed_key)
    }
}

pub fn scoped_path<T: StateScope>(key: &str) -> String {
    join_path(T::PREFIX, key)
}
