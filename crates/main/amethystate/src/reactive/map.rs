use crate::reactive::watch::{Immediate, Watch, Watchable};
use crate::store::Store;
use crate::store::sync_backend::StoreBackend;
use crate::{AccessMode, DefaultStore, Field, ReadOnlyMode, StoreSubscription, WritableMode};
use amethystate_core::backend::AmeBackendSync;
use amethystate_core::{InterceptDisposer, MapChange, ReactiveMapCore, SignalSubscription};
use std::marker::PhantomData;

use std::sync::Arc;
use uuid::Uuid;

pub struct ReactiveMap<K, V, S: Store = DefaultStore, M: AccessMode = ReadOnlyMode> {
    pub core: ReactiveMapCore<K, V>,
    pub path: Arc<str>,
    pub instance_id: Uuid,
    pub store: S,
    pub(crate) store_sub: Arc<StoreSubscription<S>>,
    pub(crate) _mode: PhantomData<M>,
}

use crate::reactive::error::{ReactiveMapError, ReactiveMapResult};
pub use amethystate_core::primitives::map_core::{ReactiveMapKey, ReactiveMapValue};

pub type ReadOnlyReactiveMap<TValue, S> = Field<TValue, S, ReadOnlyMode>;
pub type WritableReactiveMap<TValue, S> = Field<TValue, S, WritableMode>;

impl<K, V, S: Store, M: AccessMode> Clone for ReactiveMap<K, V, S, M> {
    fn clone(&self) -> Self {
        Self {
            core: self.core.clone(),
            path: self.path.clone(),
            instance_id: self.instance_id,
            store: self.store.clone(),
            store_sub: self.store_sub.clone(),
            _mode: PhantomData,
        }
    }
}

impl<K, V, S, M> std::fmt::Debug for ReactiveMap<K, V, S, M>
where
    K: std::fmt::Debug + ReactiveMapKey,
    V: std::fmt::Debug + ReactiveMapValue,
    S: Store,
    M: AccessMode,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut d = f.debug_struct("ReactiveMap");
        d.field("path", &self.path);

        if let Ok(cache) = self.core.cache.try_lock() {
            d.field("cache_entries", &*cache);
        } else {
            d.field("cache_entries", &"<locked>");
        }

        d.field("core", &self.core).finish()
    }
}

impl<K, V, S, M> ReactiveMap<K, V, S, M>
where
    K: ReactiveMapKey,
    V: ReactiveMapValue,
    S: Store,
    M: AccessMode,
{
    pub fn fork(&self) -> Self {
        self.fork_with_id(Uuid::new_v4())
    }

    pub fn fork_with_id(&self, new_instance_id: Uuid) -> Self {
        Self {
            core: self.core.clone(),
            path: self.path.clone(),
            instance_id: new_instance_id,
            store: self.store.clone(),
            store_sub: self.store_sub.clone(),
            _mode: PhantomData,
        }
    }

    pub fn get(&self, key: &K) -> ReactiveMapResult<Option<V>> {
        let backend = StoreBackend::new(self.store.clone());
        Ok(amethystate_core::map_get(&backend, &self.path, key)?)
    }

    pub fn contains_key(&self, key: &K) -> ReactiveMapResult<bool> {
        let backend = StoreBackend::new(self.store.clone());
        Ok(amethystate_core::map_contains_key::<_, _, V>(
            &backend, &self.path, key,
        )?)
    }

    /// Every entry, sorted by key. Values are decoded as the iterator is
    /// consumed, so `.find()` or `.take(n)` decode only what they reach.
    pub fn entries(&self) -> ReactiveMapResult<impl Iterator<Item = (K, V)>> {
        let backend = StoreBackend::new(self.store.clone());
        let prefix = format!("{}.", self.path);
        let scanned = backend.scan_prefix(&prefix)?;

        Ok(scanned.into_iter().filter_map(move |(full_path, raw)| {
            let key = K::from_str(full_path.strip_prefix(&prefix)?).ok()?;
            let value = backend.decode::<V>(&raw).ok()?;
            Some((key, value))
        }))
    }

    /// Every key, sorted. Values are neither read nor deserialized.
    pub fn keys(&self) -> ReactiveMapResult<Vec<K>> {
        let backend = StoreBackend::new(self.store.clone());
        let prefix = format!("{}.", self.path);

        Ok(backend
            .scan_keys(&prefix)?
            .into_iter()
            .filter_map(|full_path| K::from_str(full_path.strip_prefix(&prefix)?).ok())
            .collect())
    }

    pub fn len(&self) -> ReactiveMapResult<usize> {
        let backend = StoreBackend::new(self.store.clone());
        Ok(amethystate_core::map_len(&backend, &self.path)?)
    }

    pub fn is_empty(&self) -> ReactiveMapResult<bool> {
        self.len().map(|l| l == 0)
    }

    #[track_caller]
    pub fn subscribe_any<F>(&self, callback: F) -> SignalSubscription
    where
        F: Fn(&MapChange<K, V>) + Send + Sync + 'static,
    {
        self.core.subscribe_any(callback)
    }

    #[track_caller]
    pub fn subscribe_key<F>(&self, key: K, callback: F) -> SignalSubscription
    where
        F: Fn(&MapChange<K, V>) + Send + Sync + 'static,
    {
        self.core.subscribe_key(key, callback)
    }

    /// Configures a subscription. See [`Watch`].
    ///
    /// Map changes are events rather than a state, so pair `local` with
    /// [`Watch::every`] unless dropping the intermediate ones is what you want.
    pub fn subscription_with(&self) -> Watch<Self, Immediate> {
        Watch::new(self.clone())
    }
}

/// One key of a map, as a [`Watch`] source. Built by [`Watch::key`].
pub struct KeyOf<K, V, S: Store, M: AccessMode> {
    map: ReactiveMap<K, V, S, M>,
    key: K,
}

impl<K, V, S: Store, M: AccessMode> KeyOf<K, V, S, M> {
    pub(crate) fn new(map: ReactiveMap<K, V, S, M>, key: K) -> Self {
        Self { map, key }
    }
}

impl<K, V, S, M> Watchable for KeyOf<K, V, S, M>
where
    K: ReactiveMapKey,
    V: ReactiveMapValue,
    S: Store,
    M: AccessMode,
{
    type Item = MapChange<K, V>;

    fn filterable(item: &MapChange<K, V>) -> bool {
        matches!(item, MapChange::Update { .. })
    }

    fn watch_id(&self) -> Uuid {
        self.map.instance_id
    }

    fn watch_raw<F>(&self, callback: F) -> SignalSubscription
    where
        F: Fn(&MapChange<K, V>, Option<Uuid>) + Send + Sync + 'static,
    {
        self.map
            .core
            .subscribe_key(self.key.clone(), move |change| {
                callback(change, change.source())
            })
    }
}

impl<K, V, S, M> Watchable for ReactiveMap<K, V, S, M>
where
    K: ReactiveMapKey,
    V: ReactiveMapValue,
    S: Store,
    M: AccessMode,
{
    type Item = MapChange<K, V>;

    fn filterable(item: &MapChange<K, V>) -> bool {
        matches!(item, MapChange::Update { .. })
    }

    fn watch_id(&self) -> Uuid {
        self.instance_id
    }

    fn watch_raw<F>(&self, callback: F) -> SignalSubscription
    where
        F: Fn(&MapChange<K, V>, Option<Uuid>) + Send + Sync + 'static,
    {
        self.core
            .subscribe_any(move |change| callback(change, change.source()))
    }
}

impl<K, V, S> ReactiveMap<K, V, S, WritableMode>
where
    K: ReactiveMapKey,
    V: ReactiveMapValue,
    S: Store,
{
    pub fn update<F>(&self, key: K, f: F) -> ReactiveMapResult<Option<V>>
    where
        F: FnOnce(V) -> V,
    {
        if let Some(val) = self.get(&key)? {
            let new_val = f(val);
            self.set(key, &new_val)?;
            Ok(Some(new_val))
        } else {
            Err(ReactiveMapError::KeyNotFound(key.to_string()))
        }
    }

    pub fn modify<F>(&self, key: K, f: F) -> ReactiveMapResult<()>
    where
        F: FnOnce(&mut V),
    {
        if let Some(mut val) = self.get(&key)? {
            f(&mut val);
            self.set(key, &val)
        } else {
            Err(ReactiveMapError::KeyNotFound(key.to_string()))
        }
    }

    pub fn set(&self, key: K, value: &V) -> ReactiveMapResult<()> {
        let backend = StoreBackend::new(self.store.clone());
        Ok(amethystate_core::map_set_existing(
            &backend,
            &self.core,
            self.path.clone(),
            key,
            value,
            Some(self.instance_id),
        )?)
    }

    pub fn set_or_create(&self, key: K, value: &V) -> ReactiveMapResult<()> {
        let backend = StoreBackend::new(self.store.clone());
        Ok(amethystate_core::map_set_or_create(
            &backend,
            &self.core,
            self.path.clone(),
            key,
            value,
            Some(self.instance_id),
        )?)
    }

    pub fn remove(&self, key: K) -> ReactiveMapResult<Option<V>> {
        let backend = StoreBackend::new(self.store.clone());
        Ok(amethystate_core::map_remove(
            &backend,
            &self.core,
            self.path.clone(),
            key,
            Some(self.instance_id),
        )?)
    }

    pub fn clear(&self) -> ReactiveMapResult<()> {
        let backend = StoreBackend::new(self.store.clone());
        Ok(amethystate_core::map_clear(
            &backend,
            &self.core,
            self.path.clone(),
            Some(self.instance_id),
        )?)
    }

    pub fn intercept<F>(&self, callback: F) -> InterceptDisposer
    where
        F: Fn(MapChange<K, V>) -> Option<MapChange<K, V>> + Send + Sync + 'static,
    {
        self.core.intercept(self.path.clone(), callback)
    }

    pub fn intercept_key<F>(&self, key: K, callback: F) -> InterceptDisposer
    where
        F: Fn(MapChange<K, V>) -> Option<MapChange<K, V>> + Send + Sync + 'static,
    {
        self.core.intercept_key(key, callback)
    }
}

impl<K, V, S: Store, M: AccessMode> PartialEq for ReactiveMap<K, V, S, M> {
    fn eq(&self, other: &Self) -> bool {
        self.path == other.path
            && self.instance_id == other.instance_id
            && Arc::ptr_eq(&self.core.next_id, &other.core.next_id)
    }
}

impl<K, V, S: Store, M: AccessMode> Eq for ReactiveMap<K, V, S, M> {}

#[cfg(test)]
mod tests {
    struct TestScope;
    impl crate::StateScope for TestScope {
        const PREFIX: &'static str = "test";
    }

    use super::*;
    use crate::DefaultStore;

    use crate::test_utils::unique_store;
    use amethystate_core::WritableMode;
    use std::collections::HashMap;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;
    use tracing_test::traced_test;

    /// `*_external` filters `Update` and nothing else: a value this handle
    /// rewrote is its own business, but a key appearing or disappearing changes
    /// what the map holds and goes to everyone.
    ///
    /// Pins the whole matrix, where `test_map_subscribe_external` covers only
    /// `Insert` and `Update`.
    #[test]
    fn external_subscriptions_filter_own_updates_only() {
        let store = unique_store("external-own-changes");
        let map: ReactiveMap<String, i32, DefaultStore, WritableMode> =
            crate::store::reactive_map_with_path::<TestScope, _, _, _, _>(
                &store,
                Arc::from("test_map.external"),
                HashMap::new(),
                Uuid::new_v4(),
            )
            .expect("map should be created");

        let seen = Arc::new(Mutex::new(Vec::<String>::new()));
        let any = seen.clone();
        let _any_sub = map.subscription_with().external().register(move |change| {
            any.lock().unwrap().push(match change {
                MapChange::Insert { .. } => "insert".into(),
                MapChange::Update { .. } => "update".into(),
                MapChange::Remove { .. } => "remove".into(),
                MapChange::Clear { .. } => "clear".into(),
            });
        });

        map.set_or_create("a".to_string(), &1).unwrap(); // Insert - delivered
        map.set("a".to_string(), &2).unwrap(); // Update - filtered
        map.set_or_create("a".to_string(), &3).unwrap(); // Update - filtered
        map.remove("a".to_string()).unwrap(); // Remove - delivered

        assert_eq!(
            *seen.lock().unwrap(),
            vec!["insert".to_string(), "remove".to_string()],
            "own updates stay hidden, own structural changes do not"
        );

        seen.lock().unwrap().clear();
        map.set_or_create("b".to_string(), &4).unwrap();
        seen.lock().unwrap().clear();
        map.clear().unwrap();

        assert_eq!(
            *seen.lock().unwrap(),
            vec!["clear".to_string()],
            "clear is one event, not one per key it dropped"
        );

        seen.lock().unwrap().clear();
        let other = map.fork();
        other.set_or_create("c".to_string(), &5).unwrap();
        other.set("c".to_string(), &6).unwrap();

        assert_eq!(
            *seen.lock().unwrap(),
            vec!["insert".to_string(), "update".to_string()],
            "another handle's updates must arrive"
        );
    }

    /// The keyed variant filters on the same rule, scoped to one key.
    #[test]
    fn external_key_subscription_filters_own_updates_only() {
        let store = unique_store("external-own-key");
        let map: ReactiveMap<String, i32, DefaultStore, WritableMode> =
            crate::store::reactive_map_with_path::<TestScope, _, _, _, _>(
                &store,
                Arc::from("test_map.external_key"),
                HashMap::new(),
                Uuid::new_v4(),
            )
            .expect("map should be created");

        let seen = Arc::new(Mutex::new(Vec::<String>::new()));
        let keyed = seen.clone();
        let _key_sub = map
            .subscription_with()
            .key("a".to_string())
            .external()
            .register(move |change| {
                keyed.lock().unwrap().push(match change {
                    MapChange::Insert { .. } => "insert".into(),
                    MapChange::Update { .. } => "update".into(),
                    MapChange::Remove { .. } => "remove".into(),
                    MapChange::Clear { .. } => "clear".into(),
                });
            });

        map.set_or_create("a".to_string(), &1).unwrap();
        map.set("a".to_string(), &2).unwrap();
        map.set_or_create("b".to_string(), &3).unwrap(); // other key, not ours
        map.remove("a".to_string()).unwrap();

        assert_eq!(
            *seen.lock().unwrap(),
            vec!["insert".to_string(), "remove".to_string()],
            "own update filtered, own structural changes delivered, other keys ignored"
        );
    }

    #[test]
    fn test_map_crud_logic() {
        let store = unique_store("crud");
        let path: Arc<str> = Arc::from("test_map.data");

        let map: ReactiveMap<String, i32, DefaultStore, WritableMode> =
            crate::store::reactive_map_with_path::<TestScope, _, _, _, _>(
                &store,
                path,
                HashMap::new(),
                Uuid::new_v4(),
            )
            .unwrap();

        map.set_or_create("a".into(), &10).unwrap();
        assert_eq!(map.get(&"a".into()).unwrap(), Some(10));
        assert_eq!(map.len().unwrap(), 1);

        map.set("a".into(), &20).unwrap();
        assert_eq!(map.get(&"a".into()).unwrap(), Some(20));

        let res = map.set("missing".into(), &30);
        assert!(matches!(res, Err(ReactiveMapError::KeyNotFound(_))));

        map.set_or_create("b".into(), &100).unwrap();
        let entries: Vec<_> = map.entries().unwrap().collect();
        assert_eq!(entries.len(), 2);

        let removed = map.remove("a".into()).unwrap();
        assert_eq!(removed, Some(20));
        assert_eq!(map.len().unwrap(), 1);

        store.save_now().unwrap();
        assert_eq!(map.get(&"a".into()).unwrap(), None);
    }

    #[test]
    fn test_map_intercept_and_reject() {
        let store = unique_store("reject");
        let map: ReactiveMap<String, i32, DefaultStore, WritableMode> =
            crate::store::reactive_map_with_path::<TestScope, _, _, _, _>(
                &store,
                Arc::from("test.intercept"),
                HashMap::new(),
                Uuid::new_v4(),
            )
            .unwrap();

        map.intercept(|change| match change {
            MapChange::Insert { value, .. }
            | MapChange::Update {
                new_value: value, ..
            } if value < 0 => None,
            _ => Some(change),
        });

        let res = map.set_or_create("val".into(), &-1);
        assert!(matches!(res, Err(ReactiveMapError::Intercepted)));

        store.save_now().unwrap();
        assert_eq!(map.get(&"val".into()).unwrap(), None);

        map.set_or_create("val".into(), &10).unwrap();
        assert_eq!(map.get(&"val".into()).unwrap(), Some(10));
    }

    #[test]
    fn test_map_intercept_transform() {
        let store = unique_store("transform");
        let map: ReactiveMap<String, i32, DefaultStore, WritableMode> =
            crate::store::reactive_map_with_path::<TestScope, _, _, _, _>(
                &store,
                Arc::from("test.transform"),
                HashMap::new(),
                Uuid::new_v4(),
            )
            .unwrap();

        map.intercept(|change| match change {
            MapChange::Insert { key, value, source } => Some(MapChange::Insert {
                key,
                value: value * 2,
                source,
            }),
            MapChange::Update {
                key,
                old_value,
                new_value,
                source,
            } => Some(MapChange::Update {
                key,
                old_value,
                new_value: new_value * 2,
                source,
            }),
            _ => Some(change),
        });

        map.set_or_create("x".into(), &5).unwrap();
        assert_eq!(map.get(&"x".into()).unwrap(), Some(10));
    }

    #[test]
    fn test_map_subscriptions() {
        let store = unique_store("subs");
        let map: ReactiveMap<String, i32, DefaultStore, WritableMode> =
            crate::store::reactive_map_with_path::<TestScope, _, _, _, _>(
                &store,
                Arc::from("test.subs"),
                HashMap::new(),
                Uuid::new_v4(),
            )
            .unwrap();

        let events = Arc::new(Mutex::new(Vec::new()));
        let e_clone = events.clone();

        let _sub = map.subscribe_any(move |change| {
            e_clone.lock().unwrap().push(change.clone());
        });

        map.set_or_create("key1".into(), &1).unwrap();
        map.set("key1".into(), &2).unwrap();
        map.remove("key1".into()).unwrap();

        std::thread::sleep(Duration::from_millis(100));

        let res = events.lock().unwrap();

        assert!(res.len() >= 3);
        assert!(matches!(res[0], MapChange::Insert { .. }));
        assert!(matches!(res[1], MapChange::Update { .. }));
        assert!(matches!(res[2], MapChange::Remove { .. }));
    }

    #[test]
    fn test_reentrancy_guard() {
        let store = unique_store("reentrancy");
        let map: ReactiveMap<String, i32, DefaultStore, WritableMode> =
            crate::store::reactive_map_with_path::<TestScope, _, _, _, _>(
                &store,
                Arc::from("test.reentrancy"),
                HashMap::new(),
                Uuid::new_v4(),
            )
            .unwrap();

        let map_clone = map.clone();
        map.intercept(move |change| {
            if let MapChange::Update { key, .. } = &change
                && key == "a"
            {
                let _ = map_clone.set("a".into(), &999);
            }
            Some(change)
        });

        map.set_or_create("a".into(), &1).unwrap();
        map.set("a".into(), &2).unwrap();

        assert_eq!(map.get(&"a".into()).unwrap(), Some(2));
    }

    #[test]
    fn test_map_clear() {
        let store = unique_store("clear");
        let map: ReactiveMap<String, i32, DefaultStore, WritableMode> =
            crate::store::reactive_map_with_path::<TestScope, _, _, _, _>(
                &store,
                Arc::from("test.clear"),
                HashMap::new(),
                Uuid::new_v4(),
            )
            .unwrap();

        map.set_or_create("k1".into(), &1).unwrap();
        map.set_or_create("k2".into(), &2).unwrap();

        assert_eq!(map.len().unwrap(), 2);

        let clear_events_count = Arc::new(AtomicUsize::new(0));
        let clear_events_count_clone = clear_events_count.clone();

        let _sub = map.subscribe_any(move |change| {
            if let MapChange::Clear { .. } = change {
                clear_events_count_clone.fetch_add(1, Ordering::SeqCst);
            }
        });

        map.clear().unwrap();
        store.save_now().unwrap();

        assert_eq!(map.len().unwrap(), 0);
        assert!(map.is_empty().unwrap());

        assert_eq!(clear_events_count.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn test_contains_key_and_cleanup() {
        let store = unique_store("contains");
        let map: ReactiveMap<String, i32, DefaultStore, WritableMode> =
            crate::store::reactive_map_with_path::<TestScope, _, _, _, _>(
                &store,
                Arc::from("test.contains"),
                HashMap::new(),
                Uuid::new_v4(),
            )
            .unwrap();

        map.set_or_create("key1".into(), &1).unwrap();

        assert!(map.contains_key(&"key1".into()).unwrap());
        assert!(!map.contains_key(&"key2".into()).unwrap());

        let call_count = Arc::new(AtomicUsize::new(0));
        let c_clone = call_count.clone();
        {
            let _sub = map.subscribe_any(move |_| {
                c_clone.fetch_add(1, Ordering::SeqCst);
            });
            map.set("key1".into(), &2).unwrap();
            assert_eq!(call_count.load(Ordering::SeqCst), 1);
        }

        map.set("key1".into(), &3).unwrap();
        assert_eq!(call_count.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn test_key_specific_logic() {
        let store = unique_store("key_spec");
        let map: ReactiveMap<String, i32, DefaultStore, WritableMode> =
            crate::store::reactive_map_with_path::<TestScope, _, _, _, _>(
                &store,
                Arc::from("test.keyspec"),
                HashMap::new(),
                Uuid::new_v4(),
            )
            .unwrap();

        map.set_or_create("target".into(), &10).unwrap();
        map.set_or_create("other".into(), &20).unwrap();

        let target_calls = Arc::new(AtomicUsize::new(0));
        let t_clone = target_calls.clone();
        let _sub = map.subscribe_key("target".into(), move |_| {
            t_clone.fetch_add(1, Ordering::SeqCst);
        });

        map.set("target".into(), &11).unwrap();
        map.set("other".into(), &21).unwrap();
        assert_eq!(target_calls.load(Ordering::SeqCst), 1);

        map.intercept_key("target".into(), |change| {
            if let MapChange::Update { new_value, .. } = change
                && new_value > 100
            {
                return None;
            }
            Some(change)
        });

        map.set("target".into(), &50).unwrap();
        let res = map.set("target".into(), &150);
        assert!(matches!(res, Err(ReactiveMapError::Intercepted)));

        map.set("other".into(), &150).unwrap();
    }

    #[test]
    fn test_entries_parsing_failures() {
        let store = unique_store("parsing");
        let path: Arc<str> = Arc::from("test.parse");

        {
            let map_str: ReactiveMap<String, String, DefaultStore, WritableMode> =
                crate::store::reactive_map_with_path::<TestScope, _, _, _, _>(
                    &store,
                    path.clone(),
                    HashMap::new(),
                    Uuid::new_v4(),
                )
                .unwrap();

            map_str
                .set_or_create("not_int_key".into(), &"1".into())
                .unwrap();
            map_str
                .set_or_create("123".into(), &"invalid_value".into())
                .unwrap();
        }

        let map_int: ReactiveMap<i32, i32, DefaultStore, WritableMode> =
            crate::store::reactive_map_with_path::<TestScope, _, _, _, _>(
                &store,
                path,
                HashMap::new(),
                Uuid::new_v4(),
            )
            .unwrap();

        let entries: Vec<_> = map_int.entries().unwrap().collect();

        // i32::from_str("123") succeed, but decoder falls back to Default (0) for invalid bytes
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0], (123, 0));
    }

    #[test]
    fn test_remove_edge_cases() {
        let store = unique_store("remove_edge");
        let map: ReactiveMap<String, i32, DefaultStore, WritableMode> =
            crate::store::reactive_map_with_path::<TestScope, _, _, _, _>(
                &store,
                Arc::from("test.remove"),
                HashMap::new(),
                Uuid::new_v4(),
            )
            .unwrap();

        let res = map.remove("none".into()).unwrap();
        assert!(res.is_none());

        map.set_or_create("ghost".into(), &1).unwrap();
        store.delete("test.remove.ghost").unwrap();

        let res = map.remove("ghost".into()).unwrap();
        assert!(res.is_none());
        assert!(!map.contains_key(&"ghost".into()).unwrap());
    }

    #[test]
    #[traced_test]
    fn test_map_recursion_warning() {
        let store = unique_store("map_trace");
        let map: ReactiveMap<String, i32, DefaultStore, WritableMode> =
            crate::store::reactive_map_with_path::<TestScope, _, _, _, _>(
                &store,
                Arc::from("test.recursive_map"),
                HashMap::new(),
                Uuid::new_v4(),
            )
            .unwrap();

        let map_clone = map.clone();

        map.intercept(move |change| {
            if let Some(key) = change.key() {
                let _ = map_clone.set_or_create(key.clone(), &999);
            }
            Some(change)
        });

        let _ = map.set_or_create("key_a".into(), &1);

        assert!(logs_contain("maximum intercept depth reached"));
        assert!(logs_contain("test.recursive_map.key_a"));
    }
    #[test]
    fn test_map_subscribe_external() {
        let store = unique_store("map_external");
        let map = crate::store::reactive_map_with_path::<
            TestScope,
            String,
            i32,
            DefaultStore,
            WritableMode,
        >(
            &store,
            Arc::from("test.external"),
            HashMap::new(),
            Uuid::new_v4(),
        )
        .unwrap();

        let fork = map.fork();

        let calls = Arc::new(AtomicUsize::new(0));
        let c_clone = calls.clone();

        let _sub = map.subscription_with().external().register(move |_| {
            c_clone.fetch_add(1, Ordering::SeqCst);
        });

        map.set_or_create("a".into(), &1).unwrap();

        assert_eq!(
            calls.load(Ordering::SeqCst),
            1,
            "Creation (Insert) is NOT ignored"
        );

        map.set("a".into(), &10).unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 1, "Own updates are ignored");

        fork.set("a".into(), &20).unwrap();

        assert_eq!(
            calls.load(Ordering::SeqCst),
            2,
            "Fork updates are processed"
        );
    }
}
