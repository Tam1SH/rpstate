use crate::observe::{Entry, Observe, entry};
use amethystate::{
    AccessMode, AmeStateSlice, DefaultStore, ReactiveMap, ReactiveMapKey, ReactiveMapValue, Store,
    global_store,
};
use windows_reactor::{Deps, RenderCx};

/// Reads reactive state into a component.
pub trait AmeCx {
    /// Loads a state slice from the global store, once per component.
    ///
    /// Requires a store installed with `init_global`. Panics if the slice will
    /// not load - a missing store or a schema that does not match what is on
    /// disk is a startup fault, not something a render can recover from.
    fn use_ame_state<T>(&mut self) -> T
    where
        T: AmeStateSlice<DefaultStore> + Clone + 'static;

    /// Loads a state slice from `store`, once per component.
    fn use_ame_state_in<T, S>(&mut self, store: &S) -> T
    where
        T: AmeStateSlice<S> + Clone + 'static,
        S: Store;

    /// Mirrors `source` into component state, re-rendering when it changes.
    ///
    /// The subscription is bound on the first render and dropped when the
    /// component unmounts. To follow a source that changes across renders, use
    /// [`AmeCx::use_ame_keyed`].
    fn use_ame<O: Observe>(&mut self, source: &O) -> O::Value;

    /// Like [`AmeCx::use_ame`], but resubscribes whenever `deps` change.
    ///
    /// The new value arrives on the render after `deps` changed, so one render
    /// still sees the previous source.
    fn use_ame_keyed<O: Observe, D: Deps + Clone>(&mut self, source: &O, deps: D) -> O::Value;

    /// Observes one key of a map, resubscribing when `key` changes.
    fn use_ame_entry<K, V, S, M>(&mut self, map: &ReactiveMap<K, V, S, M>, key: K) -> Option<V>
    where
        K: ReactiveMapKey + PartialEq,
        V: ReactiveMapValue + PartialEq,
        S: Store,
        M: AccessMode;
}

impl AmeCx for RenderCx {
    fn use_ame_state<T>(&mut self) -> T
    where
        T: AmeStateSlice<DefaultStore> + Clone + 'static,
    {
        self.use_ame_state_in(&global_store())
    }

    fn use_ame_state_in<T, S>(&mut self, store: &S) -> T
    where
        T: AmeStateSlice<S> + Clone + 'static,
        S: Store,
    {
        let store = store.clone();
        self.use_memo((), move || {
            T::load_slice(&store).unwrap_or_else(|err| {
                panic!(
                    "amethystate-reactor: could not load `{}`: {err}",
                    std::any::type_name::<T>()
                )
            })
        })
    }

    fn use_ame<O: Observe>(&mut self, source: &O) -> O::Value {
        self.use_ame_keyed(source, ())
    }

    fn use_ame_keyed<O: Observe, D: Deps + Clone>(&mut self, source: &O, deps: D) -> O::Value {
        let seed = {
            let source = source.clone();
            self.use_memo((), move || source.snapshot())
        };
        let (value, set) = self.use_async_state(seed);

        let rendered = value.clone();
        let source = source.clone();
        self.use_effect_with_cleanup(deps, move || {
            let now = source.snapshot();
            if now != rendered {
                set.call(now);
            }

            let resnapshot = source.clone();
            let sub = source.watch(move || set.call(resnapshot.snapshot()));

            Some(move || drop(sub))
        });

        value
    }

    fn use_ame_entry<K, V, S, M>(&mut self, map: &ReactiveMap<K, V, S, M>, key: K) -> Option<V>
    where
        K: ReactiveMapKey + PartialEq,
        V: ReactiveMapValue + PartialEq,
        S: Store,
        M: AccessMode,
    {
        let source: Entry<K, V, S, M> = entry(map, key.clone());
        self.use_ame_keyed(&source, key)
    }
}
