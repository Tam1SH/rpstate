use crate::reactive::cell::CellCommit;
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
    ///
    /// The cell writes through to the map, so a change made either way is
    /// visible from the other. Removal does not invalidate it - the value
    /// falls back to `default` and the cell stays usable.
    ///
    /// ```
    /// # use amethystate::StoreBuilder;
    /// # let path = amethystate_core::test_utils::TempPath::new("doc");
    /// # let store = StoreBuilder::new(&*path).build().unwrap();
    /// let widths = store.kv().map::<String, u64>("columns").unwrap();
    /// let cpu = widths.entry_cell("cpu".to_string(), 100);
    ///
    /// // The key is absent, so the cell reads the default.
    /// assert_eq!(cpu.get(), 100);
    ///
    /// // Writing through the cell writes the map entry.
    /// cpu.set(120).unwrap();
    /// assert_eq!(widths.get(&"cpu".to_string()).unwrap(), Some(120));
    ///
    /// // And a write to the map reaches the cell.
    /// widths.update("cpu".into(), &200).unwrap();
    /// assert_eq!(cpu.get(), 200);
    ///
    /// // Removing the key puts the default back, without breaking the cell.
    /// widths.remove("cpu".to_string()).unwrap();
    /// assert_eq!(cpu.get(), 100);
    /// cpu.set(80).unwrap();
    /// assert_eq!(widths.get(&"cpu".to_string()).unwrap(), Some(80));
    /// ```
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
        let commit = CellCommit {
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
