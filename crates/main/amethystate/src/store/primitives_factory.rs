use crate::observability::register_field;
use crate::store::StorageError;
use crate::store::StorageResult;
use crate::store::StoreSubscription;
use crate::{Field, ReactiveMap, StateScope, Store, StoreBackend, StoreOp, SubscriptionKind};
use crate::{ReactiveMapKey, ReactiveMapValue};
use amethystate_core::path::{IntoStorePath, StorePath};
use amethystate_core::{FieldCore, MapChange, ReactiveMapCore, Signal};
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
) -> StorageResult<Field<TValue>>
where
    TScope: StateScope,
    TValue: Serialize + DeserializeOwned + Default + Clone + Send + Sync + 'static,
{
    let path = TScope::PATH.join(&crate::store::to_path(key)?);
    field_with_path(store, path, default, instance_id)
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
    let path = crate::store::to_path(path)?;

    register_field::<TValue>(&path, instance_id);

    if store.get::<TValue>(&path)?.is_none() {
        seed(store, &path, &default)?;
    }

    let current = store
        .get::<TValue>(&path)?
        .unwrap_or_else(|| default.clone());
    let signal = Signal::new(current);

    let sig_clone = signal.clone();
    let store_clone = store.clone();
    let path_log = path.clone();
    let on_delete = default.clone();

    let unreadable = crate::reactive::field::Unreadable::default();
    let unreadable_sub = unreadable.clone();
    let on_unreadable = default.clone();

    let id = store.subscribe(
        SubscriptionKind::ExactPath(path.clone()),
        Arc::new(move |event| match &event.new {
            Some(raw) => match store_clone.decode::<TValue>(raw) {
                Ok(parsed) => {
                    if let Ok(mut held) = unreadable_sub.lock() {
                        *held = None;
                    }
                    sig_clone.set_forwarded(parsed, event.source)
                }
                // The store holds something this field cannot read. Going on
                // to report the value from before it would be a lie that looks
                // exactly like a write that worked, so the field says what the
                // next startup would say and records why - see
                // `Field::unreadable`.
                Err(e) => {
                    tracing::error!(path = %path_log, error = %e, "decode failed");
                    if let Ok(mut held) = unreadable_sub.lock() {
                        *held = Some(Arc::from(e.to_string().as_str()));
                    }
                    sig_clone.set_forwarded(on_unreadable.clone(), event.source)
                }
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
            unreadable,
            core: FieldCore::new_with_signal(signal),
            path,
            instance_id,
            store_sub: Some(Arc::new(StoreSubscription::new(store.clone(), id))),
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

fn seed<TValue>(store: &Store, path: &StorePath, default: &TValue) -> StorageResult<()>
where
    TValue: Serialize,
{
    match store.set(path, default) {
        Err(report) if report.contains::<crate::store::Occupied>() => {
            tracing::warn!(
                target: "amethystate",
                path = %path,
                error = %crate::store::one_line(&report),
                "the field starts on its default: the store already holds something in the way, \
                 and seeding over it would destroy it",
            );
            Ok(())
        }
        other => other,
    }
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
    // Decoding is the larger half of reading a map back - a value with a few
    // fields in it costs around two hundred nanoseconds where an integer costs
    // eleven - and every entry is independent of every other. Dividing it wants
    // the entries in hand, so that path takes the owning scan.
    if store.parallel_reads() {
        use rayon::prelude::*;

        let scanned = store
            .scan_prefix(path)
            .attach_with(|| format!("map: {path}"))?;

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

        let mut entries = HashMap::with_capacity(scanned.len());
        for (stored, bytes) in &scanned {
            if let Some((key, value)) = decode_entry(store, path, stored.as_str(), bytes)? {
                entries.insert(key, value);
            }
        }
        return Ok(entries);
    }

    // On one thread nothing has to be handed over as owned, so nothing is
    // built: the visiting scan gives the key as it is stored and the value as
    // the engine holds it, and each entry is decoded where it is seen.
    // No `attach` here: what fails inside is one entry, and `decode_entry`
    // names the map and the entry itself. Saying "map" twice is what the outer
    // one did.
    let mut entries = HashMap::new();
    store.visit_prefix(path, &mut |key, bytes| {
        if let Some((k, v)) = decode_entry(store, path, key, bytes)? {
            entries.insert(k, v);
        }
        Ok(())
    })?;

    Ok(entries)
}

/// How few entries are left on one thread.
///
/// Measured rather than guessed: below roughly a thousand, parsing and
/// decoding are quick enough that handing them out and collecting them back
/// costs more than doing them in place. The crossover sits nearer three
/// hundred for both, and this is the round number above it.
const PARALLEL_MIN_LEN: usize = 1024;

/// One scanned entry as a map key and its value, or `None` for the map's own
/// path.
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
    {
        let below = amethystate_core::path::name_under_key(stored, path)
            .change_context(StorageError::Path)
            .attach_with(|| format!("map: {path}"))
            .attach_with(|| format!("stored key: {stored}"))?;

        let name = match below.as_deref() {
            Some(name) => name,
            None if stored == path.as_str() => return Ok(None),
            None => {
                return Err(Report::new(StorageError::Path)
                    .attach(format!("map: {path}"))
                    .attach(format!("stored key: {stored}"))
                    .attach("the key is not under the map it was scanned from"));
            }
        };

        let key = K::from_str(name).map_err(|_| {
            Report::new(StorageError::Codec)
                .attach(format!("map: {path}"))
                .attach(format!("entry: {name}"))
                .attach(format!("key type: {}", std::any::type_name::<K>()))
        })?;

        let value = store
            .decode::<V>(bytes)
            .attach_with(|| format!("map: {path}"))
            .attach_with(|| format!("entry: {name}"))?;

        Ok(Some((key, value)))
    }
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
    let mut known_cache = load_map::<K, V>(store, &path)?;

    // Keyed on this map's own path, not on the scope. A scope is marked
    // initialized once, when its struct is first built, so a map added to that
    // struct later never seeded its defaults for anyone already running - it
    // came up empty with nothing to say why.
    //
    // Keys already on disk count as having been seeded, so upgrading does not
    // restore entries the user has since removed.
    let seeded_before = store.is_initialized(&path)? || !known_cache.is_empty();

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
            store_sub: Arc::new(StoreSubscription::new(store.clone(), id)),
        }),
    })
}
