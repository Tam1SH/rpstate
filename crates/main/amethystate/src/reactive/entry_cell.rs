use crate::reactive::cell::ReactiveCell;
use crate::store::Store;
use crate::{ReactiveMap, ReactiveMapKey, ReactiveMapValue, WritableMode};
use amethystate_core::{MapChange, Signal};
use std::sync::Arc;

impl<K, V, S: Store> ReactiveMap<K, V, S, WritableMode>
where
    K: ReactiveMapKey,
    V: ReactiveMapValue,
{
    /// A live cell over one map entry. `default` stands in while the key is
    /// absent, and again if it is later removed.
    ///
    /// Writes go to the map, and the map's subscription is the only thing that
    /// writes the cache. That single writer is what keeps one write to one
    /// notification: an entry that also wrote its own cache would raise
    /// subscribers twice, once locally and once when the change came back
    /// round through the store.
    pub fn entry_cell(&self, key: K, default: V) -> ReactiveCell<V> {
        let initial = self
            .get(&key)
            .ok()
            .flatten()
            .unwrap_or_else(|| default.clone());
        let cache = Signal::new(initial);

        // map -> cache, and nothing else writes here. The change carries the
        // provenance of whoever wrote it, which is passed through untouched so
        // subscribers can still tell their own writes from anyone else's.
        let sink = cache.clone();
        let read = self.subscribe_key(key.clone(), move |change| {
            let (next, source) = match change {
                MapChange::Insert { value, source, .. }
                | MapChange::Update {
                    new_value: value,
                    source,
                    ..
                } => (value.clone(), *source),
                MapChange::Remove { source, .. } | MapChange::Clear { source } => {
                    (default.clone(), *source)
                }
            };
            sink.set_forwarded(next, source);
        });

        // Writes go to the map and stop there. The map decides what was
        // actually committed and reports it through the subscription above, so
        // a write an interceptor rewrote or refused cannot leave the cell
        // holding a value the store never took.
        let map = self.clone();
        let write_key = key;

        ReactiveCell::from_parts(
            cache,
            Arc::new(move |value: V| Ok(map.set_or_create(write_key.clone(), &value)?)),
            // Unlike a field, nothing else owns this subscription: the map does
            // not hold it, and the writer captures only the map and the key.
            // Drop it and the cache quietly stops being updated.
            Some(Arc::new(read)),
        )
    }
}
