use crate::observability::register_field;
use crate::reactive::field::{Unread, Unreadable};
use crate::store::StorageError;
use crate::store::StorageResult;
use crate::store::StoreSubscription;
use crate::store::facts::{Entry, Facts, Key, Prefix, RawKey, Refused};
use crate::store::rules::{OnDelete, OnUnreadable, ReadRules};
use crate::store::traits::{StoreExt as _, StoredAs};
use crate::{Field, ReactiveMap, StateScope, Store, StoreBackend, StoreOp, SubscriptionKind};
use crate::{ReactiveMapKey, ReactiveMapValue};
use amethystate_core::path::{IntoStorePath, Level, StorePath};
use amethystate_core::{FieldCore, MapChange, ReactiveMapCore, Signal};
use error_stack::{Report, ResultExt};
use indexmap::IndexMap;
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
) -> StorageResult<Field<TValue>>
where
    TScope: StateScope,
    TValue: Serialize + DeserializeOwned + Default + Clone + Send + Sync + 'static,
{
    let path = TScope::PATH.join(&crate::store::to_path(key)?);
    field_with_path(store, path, default, instance_id)
}

/// Records that whoever is being built owns this path, or refuses because
/// somebody else already does.
///
/// The claim is the schema's own type name, which is what makes it idempotent:
/// building the same struct twice claims the same path twice and changes
/// nothing. An instance nobody registered claims nothing - there is no name to
/// attribute it to, and refusing what cannot be attributed would be guessing.
fn claim(store: &Store, path: &StorePath, instance_id: Uuid) -> StorageResult<()> {
    match crate::observability::resolve_instance(instance_id) {
        Some(by) => store.owners().claim(path, by),
        None => Ok(()),
    }
}

pub fn field_with_path<TValue>(
    store: &Store,
    path: impl IntoStorePath,
    default: TValue,
    instance_id: Uuid,
) -> StorageResult<Field<TValue>>
where
    TValue: Serialize + DeserializeOwned + Default + Clone + Send + Sync + 'static,
{
    field_with_path_under(store, path, default, instance_id, ReadRules::new())
}

/// [`field_with_path`] with a say in what a value it cannot read, and a key
/// removed under it, each do.
pub fn field_with_path_where<TValue>(
    store: &Store,
    path: impl IntoStorePath,
    default: TValue,
    instance_id: Uuid,
    policy: OnUnreadable,
    on_delete: OnDelete,
) -> StorageResult<Field<TValue>>
where
    TValue: Serialize + DeserializeOwned + Default + Clone + Send + Sync + 'static,
{
    field_with_path_under(
        store,
        path,
        default,
        instance_id,
        ReadRules::new().on_unreadable(policy).on_delete(on_delete),
    )
}

/// [`field_with_path`] under everything the field declared about disagreeing
/// with the store.
pub fn field_with_path_under<TValue>(
    store: &Store,
    path: impl IntoStorePath,
    default: TValue,
    instance_id: Uuid,
    rules: ReadRules<TValue>,
) -> StorageResult<Field<TValue>>
where
    TValue: Serialize + DeserializeOwned + Default + Clone + Send + Sync + 'static,
{
    let path = crate::store::to_path(path)?;
    let ReadRules {
        on_unreadable: policy,
        on_delete,
        check,
        stored_as,
    } = rules;

    claim(store, &path, instance_id)?;
    register_field::<TValue>(&path, instance_id);

    let mut refused: Option<Unread> = None;

    let current = match read_stored(store, &path, stored_as) {
        Ok(Some(stored)) => match check.map(|check| check(&stored, store.context())) {
            None | Some(Ok(())) => stored,
            Some(Err(invalid)) => {
                if policy == OnUnreadable::Refuse {
                    return Err(crate::store::refused(&path, &invalid));
                }

                tracing::error!(
                    path = %path,
                    reason = %invalid,
                    "a declared check refused the stored value, so the field starts on its default"
                );
                refused = Some(Unread::Refused(Arc::from(invalid.reason())));
                default.clone()
            }
        },
        Ok(None) => {
            if let Some(in_the_way) = seed(store, &path, &default, stored_as)? {
                refused = Some(Unread::Occupied(in_the_way));
            }
            default.clone()
        }
        Err(why) if policy.covers(&why) => {
            tracing::error!(path = %path, error = %why, "decode failed while building");
            refused = Some(Unread::Undecodable(Arc::from(why.to_string().as_str())));
            default.clone()
        }
        Err(why) => return Err(why),
    };

    let signal = Signal::new(current);

    let sig_clone = signal.clone();
    let store_clone = store.clone();
    let path_log = path.clone();
    let deleted = default.clone();

    let unreadable = Unreadable::new(std::sync::Mutex::new(refused));
    let unreadable_sub = unreadable.clone();

    let id = store.subscribe(
        SubscriptionKind::ExactPath(path.clone()),
        Arc::new(move |event| match &event.new {
            Some(raw) => match match stored_as.read {
                Some(read) => store_clone.decode_with(raw, read),
                None => store_clone.decode::<TValue>(raw),
            } {
                Ok(parsed) => {
                    if let Some(check) = check.filter(|_| event.is_external_edit())
                        && let Err(invalid) = check(&parsed, store_clone.context())
                    {
                        if let Ok(mut held) = unreadable_sub.lock() {
                            *held = Some(Unread::Refused(Arc::from(invalid.reason())));
                        }

                        return Err(Report::new(StorageError::Notify)
                            .attach(Key(path_log.clone()))
                            .attach(Refused(invalid.reason().to_string()))
                            .attach("the field kept what it had"));
                    }

                    if let Ok(mut held) = unreadable_sub.lock() {
                        *held = None;
                    }
                    sig_clone.set_forwarded(parsed, event.source.handle());
                    Ok(())
                }
                Err(e) => {
                    if let Ok(mut held) = unreadable_sub.lock() {
                        *held = Some(Unread::Undecodable(Arc::from(e.to_string().as_str())));
                    }

                    Err(e
                        .change_context(StorageError::Notify)
                        .attach(Key(path_log.clone())))
                }
            },
            None => {
                match on_delete {
                    OnDelete::UseDefault => {
                        sig_clone.set_forwarded(deleted.clone(), event.source.handle())
                    }
                    OnDelete::Keep => {}
                }
                Ok(())
            }
        }),
    );

    Ok(Field {
        inner: Arc::new(crate::reactive::field::FieldInner {
            unreadable,
            core: FieldCore::new_with_signal(signal),
            path,
            instance_id,
            store_sub: Some(Arc::new(StoreSubscription::new(store.clone(), id))),
            stored_as,
        }),
    })
}

/// A map under `TScope`'s path, at the levels `key` names.
pub fn reactive_map<TScope, K, V>(
    store: &Store,
    key: impl IntoStorePath,
    default: HashMap<K, V>,
    instance_id: Uuid,
) -> StorageResult<ReactiveMap<K, V>>
where
    TScope: StateScope,
    K: ReactiveMapKey,
    V: ReactiveMapValue,
{
    let path = TScope::PATH.join(&crate::store::to_path(key)?);
    reactive_map_with_path::<TScope, _, _>(store, path, default, instance_id)
}

pub fn reactive_map_with_path<TScope, K, V>(
    store: &Store,
    path: impl IntoStorePath,
    defaults: HashMap<K, V>,
    instance_id: Uuid,
) -> StorageResult<ReactiveMap<K, V>>
where
    TScope: StateScope,
    K: ReactiveMapKey,
    V: ReactiveMapValue,
{
    reactive_map_with_path_only(store, path, defaults, instance_id)
}

/// The value at `path`, read the way the field says rather than the way its
/// type would.
fn read_stored<TValue>(
    store: &Store,
    path: &StorePath,
    stored_as: StoredAs<TValue>,
) -> StorageResult<Option<TValue>>
where
    TValue: DeserializeOwned + 'static,
{
    let Some(read) = stored_as.read else {
        return store.get::<TValue>(path);
    };

    match store.get_raw(path)? {
        Some(bytes) => store.decode_with(&bytes, read).map(Some),
        None => Ok(None),
    }
}

/// Writes the field's declared default, and says so if it could not.
///
/// `Some` is what stood in the way. Building carries on - the field takes the
/// default it was declared with - but it is now reporting something the store
/// does not hold, so what came back here goes to [`Unread::Occupied`] and out
/// through [`Field::try_get`](crate::Field::try_get).
fn seed<TValue>(
    store: &Store,
    path: &StorePath,
    default: &TValue,
    stored_as: StoredAs<TValue>,
) -> StorageResult<Option<Arc<str>>>
where
    TValue: Serialize + 'static,
{
    let written = match stored_as.write {
        Some(write) => write(default, &mut |erased| {
            StoreBackend::set_erased(store, path, erased, None)
        }),
        None => store.set(path, default),
    };

    match written {
        Err(report) if report.contains::<crate::store::Occupied>() => {
            Ok(Some(Arc::from(crate::store::one_line(&report).as_str())))
        }
        Err(other) => Err(other),
        Ok(()) => Ok(None),
    }
}

/// Every entry stored under `path`, keyed by the level below it.
///
/// A key that cannot be read back is an error rather than an absence. The path
/// itself is not an entry.
pub fn load_map<K, V>(store: &Store, path: &StorePath) -> StorageResult<IndexMap<K, V>>
where
    K: ReactiveMapKey,
    V: ReactiveMapValue,
{
    if store.parallel_reads() {
        use rayon::prelude::*;

        let scanned = store.scan_prefix(path).attach_prefix(path)?;

        if scanned.len() >= PARALLEL_MIN_LEN {
            let decoded: Vec<(K, V)> = scanned
                .par_iter()
                .with_min_len(PARALLEL_MIN_LEN)
                .filter_map(|(stored, bytes)| {
                    decode_entry(store, path, stored.as_str(), bytes).transpose()
                })
                .collect::<StorageResult<Vec<_>>>()?;

            return Ok(decoded.into_iter().collect());
        }

        let mut entries = IndexMap::with_capacity(scanned.len());
        for (stored, bytes) in &scanned {
            if let Some((key, value)) = decode_entry(store, path, stored.as_str(), bytes)? {
                entries.insert(key, value);
            }
        }
        return Ok(entries);
    }

    let mut entries = IndexMap::new();
    store.visit_prefix(path, &mut |key, bytes| {
        if let Some((k, v)) = decode_entry(store, path, key, bytes)? {
            entries.insert(k, v);
        }
        Ok(())
    })?;

    Ok(entries)
}

const PARALLEL_MIN_LEN: usize = 1024;

fn decode_entry<K, V>(
    store: &Store,
    path: &StorePath,
    stored: &str,
    bytes: &[u8],
) -> StorageResult<Option<(K, V)>>
where
    K: ReactiveMapKey,
    V: ReactiveMapValue,
{
    let below = amethystate_core::path::level_under(stored, path)
        .change_context(StorageError::Path)
        .attach_prefix(path)
        .attach_raw_key(stored)?;

    let name = match &below {
        Level::Entry(name) => name.as_ref(),
        Level::Prefix => return Ok(None),
        Level::Deeper(name) => {
            return Err(Report::new(StorageError::Path)
                .attach(Prefix(path.clone()))
                .attach(RawKey(stored.to_owned()))
                .attach(Entry(name.to_string()))
                .attach(
                    "a map owns the level below it and nothing further, so this key \
                     belongs to whatever claimed that level",
                ));
        }
        Level::Outside => {
            return Err(Report::new(StorageError::Path)
                .attach(Prefix(path.clone()))
                .attach(RawKey(stored.to_owned()))
                .attach("the key is not under the map it was scanned from"));
        }
    };

    let key = K::from_str(name).map_err(|_| {
        Report::new(StorageError::Codec)
            .attach(Prefix(path.clone()))
            .attach(Entry(name.to_owned()))
            .attach(format!("key type: {}", std::any::type_name::<K>()))
    })?;

    let value = store
        .decode::<V>(bytes)
        .attach_prefix(path)
        .attach_entry(name)?;

    Ok(Some((key, value)))
}

pub fn reactive_map_with_path_only<K, V>(
    store: &Store,
    path: impl IntoStorePath,
    defaults: HashMap<K, V>,
    instance_id: Uuid,
) -> StorageResult<ReactiveMap<K, V>>
where
    K: ReactiveMapKey,
    V: ReactiveMapValue,
{
    let path = crate::store::to_path(path)?;
    claim(store, &path, instance_id)?;

    let mut known_cache = load_map::<K, V>(store, &path)?;

    let seeded_before = store.is_initialized(&path)? || !known_cache.is_empty();

    if !seeded_before {
        for (k, v) in defaults {
            let full_path = path
                .try_push(k.to_string())
                .change_context(StorageError::Path)
                .attach_prefix(&path)
                .attach_entry(&k.to_string())?;
            store.set(&full_path, &v)?;
            known_cache.insert(k, v);
        }
    }
    store.mark_initialized(&path)?;

    let core = ReactiveMapCore::with_capacity(known_cache.len());
    for (k, v) in known_cache {
        core.cache.insert(k, v);
    }

    let core_clone = core.clone();
    let map_path = path.clone();
    let path_for_keys = path.clone();
    let store_clone = store.clone();
    let id = store.subscribe(
        SubscriptionKind::Prefix(path.clone()),
        Arc::new(move |event| {
            if event.op == StoreOp::DeletePrefix && event.path == map_path {
                core_clone.cache.clear();
                core_clone.notify(&MapChange::Clear {
                    source: event.source.handle(),
                });
                return Ok(());
            }

            let Some(key_str) = path_for_keys.entry_name(&event.path) else {
                return Err(Report::new(StorageError::Notify)
                    .attach(Key(event.path.clone()))
                    .attach(Prefix(path_for_keys.clone()))
                    .attach("not a path this library could have written, so the map did not take it"));
            };

            let Ok(k) = K::from_str(&key_str) else {
                return Err(Report::new(StorageError::Notify)
                    .attach(Key(event.path.clone()))
                    .attach(Prefix(path_for_keys.clone()))
                    .attach(format!("does not parse as {}", std::any::type_name::<K>())));
            };

            {
                let source = event.source.handle();

                let new_val = match event.new.as_ref().map(|b| store_clone.decode::<V>(b)) {
                    Some(Ok(value)) => Some(value),
                    Some(Err(e)) => {
                        return Err(e
                            .change_context(StorageError::Notify)
                            .attach(Key(event.path.clone()))
                            .attach("the map kept what it had"));
                    }
                    None => None,
                };

                let stored_old = match event.old.as_ref().map(|b| store_clone.decode::<V>(b)) {
                    Some(Ok(value)) => Some(value),
                    Some(Err(e)) => {
                        tracing::warn!(
                            path = %event.path,
                            "the value being replaced would not read as this map's value type, so what this map last held is what subscribers are told: {e:?}"
                        );
                        None
                    }
                    None => None,
                };

                let old_val = stored_old.or_else(|| core_clone.cache.get(&k));

                let change = {
                    let keys = &core_clone.cache;

                    match event.op {
                        StoreOp::Set => {
                            let Some(new_value) = new_val else {
                                return Err(Report::new(StorageError::Notify)
                                    .attach(Key(event.path.clone()))
                                    .attach("a set carried no value, so the map kept what it had"));
                            };

                            if keys.contains_key(&k) {
                                let old_value = old_val;
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
                                old_value: old_val,
                                source,
                            }
                        }
                    }
                };

                core_clone.notify(&change);
            }

            Ok(())
        }),
    );

    Ok(ReactiveMap {
        inner: Arc::new(crate::reactive::map::MapInner {
            core,
            path,
            instance_id,
            store: store.clone(),
            store_sub: Arc::new(StoreSubscription::new(store.clone(), id)),
        }),
    })
}
