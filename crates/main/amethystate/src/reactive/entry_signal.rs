use crate::store::Store;
use crate::{DefaultStore, ReactiveMap, ReactiveMapKey, ReactiveMapValue, WritableMode};
use amethystate_core::{MapChange, Signal, SignalSubscription};
use uuid::Uuid;

pub struct MapEntrySignal<K, V, S: Store = DefaultStore> {
    key: K,
    map: ReactiveMap<K, V, S, WritableMode>,
    inner: Signal<V>,
    _sync: (SignalSubscription, SignalSubscription),
}

impl<K, V, S: Store> ReactiveMap<K, V, S, WritableMode>
where
    K: ReactiveMapKey,
    V: ReactiveMapValue,
{
    /// Live per-key cell over this map entry. `default` is used when the key
    /// is absent (and again if the key is later removed from the store).
    pub fn entry_signal(&self, key: K, default: V) -> MapEntrySignal<K, V, S> {
        let initial = self
            .get(&key)
            .ok()
            .flatten()
            .unwrap_or_else(|| default.clone());
        let inner = Signal::new(initial);

        // Marks signal updates that came *from* the store, so the write-back
        // subscription below skips them (deterministic anti-echo).
        let sync_source = Uuid::new_v4();

        // store -> signal (any write for this key; the write-back below is
        // suppressed for store-originated updates via the marker, so own
        // writes arriving here are harmless re-sets of the same value)
        let inner_for_read = inner.clone();
        let read = self.subscribe_key(key.clone(), move |change| {
            let next = match change {
                MapChange::Insert { value, .. }
                | MapChange::Update {
                    new_value: value, ..
                } => value.clone(),
                MapChange::Remove { .. } | MapChange::Clear { .. } => default.clone(),
            };
            inner_for_read.set_with_source(next, sync_source);
        });

        // signal -> store (skip store-originated updates)
        let map_for_write = self.clone();
        let key_for_write = key.clone();
        let write = inner.subscribe_with_source(move |value, source| {
            if source != Some(sync_source) {
                let _ = map_for_write.set_or_create(key_for_write.clone(), value);
            }
        });

        MapEntrySignal {
            key,
            map: self.clone(),
            inner,
            _sync: (read, write),
        }
    }
}

impl<K, V, S> MapEntrySignal<K, V, S>
where
    K: ReactiveMapKey,
    V: ReactiveMapValue,
    S: Store,
{
    /// The shared inner signal - hand it to anything that already speaks
    /// `Signal<V>`; writes to it land in the store.
    pub fn signal(&self) -> Signal<V> {
        self.inner.clone()
    }

    pub fn get(&self) -> V {
        self.inner.get()
    }

    /// Writes through the inner signal (and thus to the store).
    pub fn set(&self, value: V) {
        self.inner.set(value);
    }

    pub fn key(&self) -> &K {
        &self.key
    }

    pub fn map(&self) -> &ReactiveMap<K, V, S, WritableMode> {
        &self.map
    }
}
