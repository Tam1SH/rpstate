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
    pub fn entry_cell(&self, key: K, default: V) -> ReactiveCell<V> {
        let initial = self
            .get(&key)
            .ok()
            .flatten()
            .unwrap_or_else(|| default.clone());
        let cache = Signal::new(initial);

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

        let origin = self.instance_id;
        let map = self.clone();
        let write_key = key;

        ReactiveCell::from_parts(
            cache,
            Arc::new(move |value: V| Ok(map.set_or_create(write_key.clone(), &value)?)),
            origin,
            Some(Arc::new(read)),
        )
    }
}
