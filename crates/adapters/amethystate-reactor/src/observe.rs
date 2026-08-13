use amethystate::{
    AccessMode, Field, ReactiveCell, ReactiveMap, ReactiveMapKey, ReactiveMapValue,
    SignalSubscription, Store,
};
use amethystate_core::primitives::field_core::FieldValue;

/// A reactive source a component can mirror into its own state.
///
/// [`snapshot`](Observe::snapshot) is what the component renders;
/// [`watch`](Observe::watch) says when to take another one.
pub trait Observe: Clone + Send + Sync + 'static {
    type Value: Clone + PartialEq + Send + 'static;

    fn snapshot(&self) -> Self::Value;

    fn watch<F>(&self, on_change: F) -> SignalSubscription
    where
        F: Fn() + Send + Sync + 'static;
}

impl<T> Observe for ReactiveCell<T>
where
    T: Clone + PartialEq + Send + Sync + 'static,
{
    type Value = T;

    fn snapshot(&self) -> T {
        self.get()
    }

    fn watch<F>(&self, on_change: F) -> SignalSubscription
    where
        F: Fn() + Send + Sync + 'static,
    {
        self.subscribe(move |_| on_change())
    }
}

impl<T, S, M> Observe for Field<T, S, M>
where
    T: FieldValue + PartialEq,
    S: Store,
    M: AccessMode,
{
    type Value = T;

    fn snapshot(&self) -> T {
        self.get()
    }

    fn watch<F>(&self, on_change: F) -> SignalSubscription
    where
        F: Fn() + Send + Sync + 'static,
    {
        self.subscribe(move |_| on_change())
    }
}

impl<K, V, S, M> Observe for ReactiveMap<K, V, S, M>
where
    K: ReactiveMapKey + PartialEq,
    V: ReactiveMapValue + PartialEq,
    S: Store,
    M: AccessMode,
{
    type Value = Vec<(K, V)>;

    fn snapshot(&self) -> Vec<(K, V)> {
        self.entries().map(Iterator::collect).unwrap_or_default()
    }

    fn watch<F>(&self, on_change: F) -> SignalSubscription
    where
        F: Fn() + Send + Sync + 'static,
    {
        self.subscribe_any(move |_| on_change())
    }
}

/// One key of a map, as an [`Observe`]. Built by [`entry`].
pub struct Entry<K, V, S: Store, M: AccessMode> {
    map: ReactiveMap<K, V, S, M>,
    key: K,
}

impl<K: Clone, V, S: Store, M: AccessMode> Clone for Entry<K, V, S, M> {
    fn clone(&self) -> Self {
        Self {
            map: self.map.clone(),
            key: self.key.clone(),
        }
    }
}

/// Observes a single key. Absent keys read as `None`.
pub fn entry<K, V, S, M>(map: &ReactiveMap<K, V, S, M>, key: K) -> Entry<K, V, S, M>
where
    K: ReactiveMapKey,
    V: ReactiveMapValue,
    S: Store,
    M: AccessMode,
{
    Entry {
        map: map.clone(),
        key,
    }
}

impl<K, V, S, M> Observe for Entry<K, V, S, M>
where
    K: ReactiveMapKey + PartialEq,
    V: ReactiveMapValue + PartialEq,
    S: Store,
    M: AccessMode,
{
    type Value = Option<V>;

    fn snapshot(&self) -> Option<V> {
        self.map.get(&self.key).ok().flatten()
    }

    fn watch<F>(&self, on_change: F) -> SignalSubscription
    where
        F: Fn() + Send + Sync + 'static,
    {
        self.map
            .subscribe_key(self.key.clone(), move |_| on_change())
    }
}
