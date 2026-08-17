use crate::reactive::cell::ReactiveCell;
use crate::store::StoreBackend;
use crate::{ReactiveMap, ReactiveMapKey, ReactiveMapValue, WritableMode};
use amethystate_core::{MapChange, Signal};
use std::sync::Arc;

impl<K, V> ReactiveMap<K, V, WritableMode>
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

        let flush_store = self.store.clone();
        let flush_path = self.path.clone();
        let start_store = self.store.clone();
        let commit = crate::reactive::cell::CellCommit {
            now: Arc::new(move || Ok(flush_store.flush_prefix(&flush_path)?)),
            start: Arc::new(move || start_store.flush_async()),
        };

        ReactiveCell::from_parts(
            cache,
            Arc::new(move |value: V| Ok(map.insert(write_key.clone(), &value)?)),
            origin,
            Some(commit),
            Some(Arc::new(read)),
        )
    }
}
