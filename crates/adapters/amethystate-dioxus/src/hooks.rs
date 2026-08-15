use crate::MapSignal;
use amethystate::{AccessMode, MapChange, Pipeline, ReactiveMapKey, ReactiveMapValue};
use amethystate_arena::PIPELINE_ARENA;
use amethystate_arena::{
    AmeStateFrameworkNested, DefaultArena, FieldHandle, MapHandle, PipelineHandle, WritableHandle,
    WritableMapHandle,
};
use dioxus::core::{Callback, spawn, use_hook};
use dioxus::hooks::{try_use_context, use_callback, use_context};
use dioxus::prelude::*;
use serde::Serialize;
use serde::de::DeserializeOwned;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::mpsc;

pub type Handle<S> = <S as AmeStateFrameworkNested>::Handle;

#[cfg(target_arch = "wasm32")]
pub fn use_amethystate<S>() -> S::Handle
where
    S: AmeStateFrameworkNested + 'static,
{
    if let Some(handle) = try_use_context::<S::Handle>() {
        return handle;
    }

    panic!(
        "amethystate-dioxus: State slice '{}' was not initialized! \
         Make sure to include it in preload_slices!(...) at the root AmeStateProvider.",
        std::any::type_name::<S>()
    );
}

#[cfg(not(target_arch = "wasm32"))]
pub fn use_amethystate<S>() -> S::Handle
where
    S: amethystate_arena::AmeStateFramework<crate::DioxusBackend> + 'static,
{
    if let Some(handle) = try_use_context::<S::Handle>() {
        return handle;
    }

    let store = try_use_context::<amethystate::Store>().unwrap_or_else(|| {
        panic!(
            "amethystate-dioxus: Store not found in context while trying to initialize '{}'. \
             Make sure AmeStateProvider is rendered at the root of your application.",
            std::any::type_name::<S>()
        );
    });
    let arena = try_use_context::<DefaultArena>().unwrap_or_else(|| {
        panic!(
            "amethystate-dioxus: DefaultArena not found in context while trying to initialize '{}'. \
             Make sure AmeStateProvider is rendered at the root of your application.",
            std::any::type_name::<S>()
        );
    });

    let handle = use_hook(|| {
        let state = S::load_slice(&store).unwrap_or_else(|err| {
            panic!(
                "amethystate-dioxus: Failed to load state slice '{}': {err}",
                std::any::type_name::<S>()
            );
        });
        state.register(&arena)
    });

    use_context_provider(|| handle);
    handle
}

pub fn use_field<T>(handle: WritableHandle<T>) -> (ReadSignal<T>, Callback<T>)
where
    T: DeserializeOwned + Serialize + Clone + Send + Sync + PartialEq + 'static,
{
    let arena = use_context::<DefaultArena>();
    let mut signal = use_signal(|| arena.get_field(handle));

    let tx = use_hook(|| {
        let (tx, mut rx) = mpsc::unbounded_channel::<T>();

        spawn(async move {
            while let Some(val) = rx.recv().await {
                signal.set(val);
            }
        });

        tx
    });

    let arena_clone = arena.clone();

    use_hook(move || {
        let sub = arena.subscribe_field(handle, move |val| {
            let _ = tx.send(val.clone());
        });
        Arc::new(sub)
    });

    let setter = use_callback(move |val: T| {
        let _ = arena_clone.set_field(handle, val);
    });

    (signal.into(), setter)
}

pub fn use_read_only_field<T, M>(handle: FieldHandle<T, M>) -> ReadSignal<T>
where
    T: DeserializeOwned + Serialize + Clone + Send + Sync + PartialEq + 'static,
    M: AccessMode,
{
    let arena = use_context::<DefaultArena>();
    let mut signal = use_signal(|| arena.get_field(handle));

    let tx = use_hook(|| {
        let (tx, mut rx) = mpsc::unbounded_channel::<T>();

        spawn(async move {
            while let Some(val) = rx.recv().await {
                signal.set(val);
            }
        });

        tx
    });

    use_hook(move || {
        let sub = arena.subscribe_field(handle, move |val| {
            let _ = tx.send(val.clone());
        });
        Arc::new(sub)
    });

    signal.into()
}

pub fn use_pipeline<T, F>(f: F) -> ReadSignal<T>
where
    T: Clone + Send + Sync + PartialEq + 'static,
    F: FnOnce() -> Pipeline<T> + 'static,
{
    let arena = use_context::<DefaultArena>();

    let handle = use_hook(|| {
        PIPELINE_ARENA.with(|a| *a.borrow_mut() = Some(arena.clone()));
        let pipeline = f();
        PIPELINE_ARENA.with(|a| *a.borrow_mut() = None);

        arena.register_pipeline(pipeline)
    });

    let arena_clone = arena.clone();
    use_hook(move || {
        struct Guard<T: 'static> {
            arena: DefaultArena,
            handle: PipelineHandle<T>,
        }
        impl<T: 'static> Drop for Guard<T> {
            fn drop(&mut self) {
                self.arena.remove_pipeline(self.handle);
            }
        }
        Arc::new(Guard {
            arena: arena_clone,
            handle,
        })
    });

    let mut signal = use_signal(|| arena.get_pipeline(handle));

    let tx = use_hook(|| {
        let (tx, mut rx) = mpsc::unbounded_channel::<T>();
        spawn(async move {
            while let Some(val) = rx.recv().await {
                signal.set(val);
            }
        });
        tx
    });

    use_hook(move || {
        let sub = arena.subscribe_pipeline(handle, move |val| {
            let _ = tx.send(val.clone());
        });
        Arc::new(sub)
    });

    signal.into()
}

pub fn use_map<K, V>(handle: WritableMapHandle<K, V>) -> MapSignal<K, V>
where
    K: ReactiveMapKey,
    V: ReactiveMapValue,
{
    let arena = use_context::<DefaultArena>();
    let mut signal = use_signal(|| {
        arena
            .get_map_entries(handle)
            .unwrap_or_default()
            .into_iter()
            .collect::<HashMap<K, V>>()
    });

    let tx = use_hook(|| {
        let (tx, mut rx) = mpsc::unbounded_channel::<HashMap<K, V>>();
        spawn(async move {
            while let Some(val) = rx.recv().await {
                signal.set(val);
            }
        });
        tx
    });

    let arena_sub = arena.clone();
    use_hook(move || {
        let arena_sub_sub = arena_sub.clone();
        let sub = arena_sub.subscribe_map_any(handle, move |_| {
            let entries = arena_sub_sub
                .get_map_entries(handle)
                .unwrap_or_default()
                .into_iter()
                .collect();
            let _ = tx.send(entries);
        });
        Arc::new(sub)
    });

    let arena_set = arena.clone();
    let _set = use_callback(move |(key, val)| {
        let _ = arena_set.set_map_entry(handle, key, val);
    });

    let arena_set_or_create = arena.clone();
    let _set_or_create = use_callback(move |(key, val)| {
        let _ = arena_set_or_create.set_map_entry(handle, key, val);
    });

    let arena_remove = arena.clone();
    let _remove = use_callback(move |key| {
        let _ = arena_remove.remove_map_entry(handle, key);
    });

    let arena_clear = arena.clone();
    let _clear = use_callback(move |_| {
        let _ = arena_clear.clear_map(handle);
    });

    MapSignal::new(signal.into(), _set, _set_or_create, _remove, _clear)
}

pub fn use_map_entry<K, V, M>(handle: MapHandle<K, V, M>, key: K) -> ReadSignal<Option<V>>
where
    K: ReactiveMapKey,
    V: ReactiveMapValue,
    M: AccessMode,
{
    let arena = use_context::<DefaultArena>();
    let mut signal = use_signal(|| arena.get_map_entry(handle, &key).ok().flatten());

    let tx = use_hook(|| {
        let (tx, mut rx) = mpsc::unbounded_channel::<Option<V>>();

        spawn(async move {
            while let Some(val) = rx.recv().await {
                signal.set(val);
            }
        });

        tx
    });

    let key_clone = key.clone();
    use_hook(move || {
        let sub = arena.subscribe_map_key(handle, key_clone, move |change| match change {
            MapChange::Insert { value, .. }
            | MapChange::Update {
                new_value: value, ..
            } => {
                let _ = tx.send(Some(value.clone()));
            }
            MapChange::Remove { .. } | MapChange::Clear { .. } => {
                let _ = tx.send(None);
            }
        });
        Arc::new(sub)
    });

    signal.into()
}

pub fn use_map_subscribe_any<K, V, M, F>(handle: MapHandle<K, V, M>, callback: F)
where
    K: ReactiveMapKey,
    V: ReactiveMapValue,
    M: AccessMode,
    F: Fn(&MapChange<K, V>) + Send + Sync + 'static,
{
    let arena = use_context::<DefaultArena>();
    use_hook(move || {
        let sub = arena.subscribe_map_any(handle, callback);
        Arc::new(sub)
    });
}

pub fn use_map_subscribe_key<K, V, M, F>(handle: MapHandle<K, V, M>, key: K, callback: F)
where
    K: ReactiveMapKey,
    V: ReactiveMapValue,
    M: AccessMode,
    F: Fn(&MapChange<K, V>) + Send + Sync + 'static,
{
    let arena = use_context::<DefaultArena>();
    use_hook(move || {
        let sub = arena.subscribe_map_key(handle, key, callback);
        Arc::new(sub)
    });
}
