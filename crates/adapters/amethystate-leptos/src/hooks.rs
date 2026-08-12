use crate::MapSignal;
use amethystate::{AccessMode, MapChange, Pipeline, ReactiveMapKey, ReactiveMapValue};
use amethystate_arena::{
    AmeStateFrameworkNested, DefaultArena, FieldHandle, MapHandle, PIPELINE_ARENA, WritableHandle,
    WritableMapHandle,
};
use leptos::callback::Callback;
use leptos::prelude::*;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub type Handle<S> = <S as AmeStateFrameworkNested>::Handle;

#[cfg(target_arch = "wasm32")]
pub fn use_amethystate<S>() -> S::Handle
where
    S: amethystate_arena::AmeStateFrameworkNested + 'static,
{
    use_context::<S::Handle>().unwrap_or_else(|| {
        panic!(
            "amethystate-leptos: State slice '{}' was not initialized! \
             Make sure to include it in preload_slices!(...) at the root AmeStateProvider.",
            std::any::type_name::<S>()
        )
    })
}

#[cfg(not(target_arch = "wasm32"))]
pub fn use_amethystate<S>() -> S::Handle
where
    S: amethystate_arena::AmeStateFramework<crate::LeptosBackend> + 'static,
{
    if let Some(handle) = use_context::<S::Handle>() {
        return handle;
    }

    let store = use_context::<amethystate::DefaultStore>().unwrap_or_else(|| {
        panic!(
            "amethystate-leptos: DefaultStore not found in context while trying to initialize '{}'. \
             Make sure AmeStateProvider is rendered at the root of your application.",
            std::any::type_name::<S>()
        );
    });
    let arena = use_context::<DefaultArena>().unwrap_or_else(|| {
        panic!(
            "amethystate-leptos: DefaultArena not found in context while trying to initialize '{}'. \
             Make sure AmeStateProvider is rendered at the root of your application.",
            std::any::type_name::<S>()
        );
    });

    let state = S::load_slice(&store).unwrap_or_else(|err| {
        panic!(
            "amethystate-leptos: Failed to load state slice '{}': {err}",
            std::any::type_name::<S>()
        );
    });
    let handle = state.register(&arena);

    provide_context(handle);

    handle
}

pub fn use_field<T>(handle: WritableHandle<T>) -> (ReadSignal<T>, SignalSetter<T>)
where
    T: DeserializeOwned + Serialize + Clone + Send + Sync + PartialEq + 'static,
{
    let arena = use_context::<DefaultArena>().expect("amethystate-leptos: Arena not found");
    let (signal, set_signal) = signal(arena.get_field(handle));

    let sub = arena.subscribe_field(handle, move |val| {
        set_signal.set(val.clone());
    });
    on_cleanup(move || drop(sub));

    let arena_clone = arena.clone();
    let setter = SignalSetter::map(move |val: T| {
        let old_val = signal.get_untracked();
        #[cfg(target_arch = "wasm32")]
        {
            set_signal.set(val.clone());
            let arena_clone = arena_clone.clone();
            leptos::task::spawn_local(async move {
                if let Err(e) = arena_clone.set_field(handle, val).await {
                    log::error!("set_field failed: {e:?}");
                    set_signal.set(old_val);
                }
            });
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            if let Err(e) = arena_clone.set_field(handle, val) {
                log::error!("set_field failed: {e:?}");
                set_signal.set(old_val);
            }
        }
    });

    (signal, setter)
}

pub fn use_read_only_field<T, M>(handle: FieldHandle<T, M>) -> ReadSignal<T>
where
    T: DeserializeOwned + Serialize + Clone + Send + Sync + PartialEq + 'static,
    M: AccessMode,
{
    let arena = use_context::<DefaultArena>().expect("amethystate-leptos: Arena not found");
    let (signal, set_signal) = signal(arena.get_field(handle));

    let sub = arena.subscribe_field(handle, move |val| {
        set_signal.set(val.clone());
    });
    on_cleanup(move || drop(sub));

    signal
}

pub fn use_pipeline<T, F>(f: F) -> ReadSignal<T>
where
    T: Clone + Send + Sync + PartialEq + 'static,
    F: FnOnce() -> Pipeline<T> + 'static,
{
    let arena = use_context::<DefaultArena>().expect("amethystate-leptos: Arena not found");

    let handle = PIPELINE_ARENA.with(|a| {
        *a.borrow_mut() = Some(arena.clone());
        let pipeline = f();
        *a.borrow_mut() = None;

        arena.register_pipeline(pipeline)
    });

    let arena_clone = arena.clone();
    on_cleanup(move || {
        arena_clone.remove_pipeline(handle);
    });

    let (signal, set_signal) = signal(arena.get_pipeline(handle));

    let sub = arena.subscribe_pipeline(handle, move |val| {
        set_signal.set(val.clone());
    });
    on_cleanup(move || drop(sub));

    signal
}

pub fn use_map<K, V>(handle: WritableMapHandle<K, V>) -> MapSignal<K, V>
where
    K: ReactiveMapKey + for<'de> Deserialize<'de>,
    V: ReactiveMapValue,
{
    let arena = use_context::<DefaultArena>().expect("amethystate-leptos: Arena not found");
    let (signal, set_signal) = signal(
        arena
            .get_map_entries(handle)
            .unwrap_or_default()
            .into_iter()
            .collect::<HashMap<K, V>>(),
    );

    let arena_sub = arena.clone();
    let sub = arena.subscribe_map_any(handle, move |_| {
        let entries = arena_sub
            .get_map_entries(handle)
            .unwrap_or_default()
            .into_iter()
            .collect();
        set_signal.set(entries);
    });
    on_cleanup(move || drop(sub));

    let arena_set = arena.clone();
    let _set = Callback::new(move |(key, val): (K, V)| {
        #[cfg(target_arch = "wasm32")]
        {
            let old = signal.get_untracked();
            set_signal.update(|m| {
                m.insert(key.clone(), val.clone());
            });
            let arena_clone = arena_set.clone();
            leptos::task::spawn_local(async move {
                if let Err(e) = arena_clone.set_map_entry(handle, key, val).await {
                    log::error!("set_map_entry failed: {e:?}");
                    set_signal.set(old);
                }
            });
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let _ = arena_set.set_map_entry(handle, key, val);
        }
    });

    let arena_set_or_create = arena.clone();
    let _set_or_create = Callback::new(move |(key, val): (K, V)| {
        #[cfg(target_arch = "wasm32")]
        {
            let old = signal.get_untracked();
            set_signal.update(|m| {
                m.insert(key.clone(), val.clone());
            });
            let arena_clone = arena_set_or_create.clone();
            leptos::task::spawn_local(async move {
                if let Err(e) = arena_clone.set_map_entry(handle, key, val).await {
                    log::error!("set_or_create_map_entry failed: {e:?}");
                    set_signal.set(old);
                }
            });
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let _ = arena_set_or_create.set_map_entry(handle, key, val);
        }
    });

    let arena_remove = arena.clone();
    let _remove = Callback::new(move |key: K| {
        #[cfg(target_arch = "wasm32")]
        {
            let old = signal.get_untracked();
            set_signal.update(|m| {
                m.remove(&key);
            });
            let arena_clone = arena_remove.clone();
            let key_clone = key.clone();
            leptos::task::spawn_local(async move {
                if let Err(e) = arena_clone.remove_map_entry(handle, key_clone).await {
                    log::error!("remove_map_entry failed: {e:?}");
                    set_signal.set(old);
                }
            });
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let _ = arena_remove.remove_map_entry(handle, key);
        }
    });

    let arena_clear = arena.clone();
    let _clear = Callback::new(move |_: ()| {
        #[cfg(target_arch = "wasm32")]
        {
            let old = signal.get_untracked();
            set_signal.set(HashMap::new());
            let arena_clone = arena_clear.clone();
            leptos::task::spawn_local(async move {
                if let Err(e) = arena_clone.clear_map(handle).await {
                    log::error!("clear_map failed: {e:?}");
                    set_signal.set(old);
                }
            });
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let _ = arena_clear.clear_map(handle);
        }
    });

    MapSignal::new(signal, _set, _set_or_create, _remove, _clear)
}

pub fn use_map_entry<K, V, M>(handle: MapHandle<K, V, M>, key: K) -> ReadSignal<Option<V>>
where
    K: ReactiveMapKey + for<'de> serde::Deserialize<'de>,
    V: ReactiveMapValue,
    M: AccessMode,
{
    let arena = use_context::<DefaultArena>().expect("amethystate-leptos: Arena not found");
    let (signal, set_signal) = signal(arena.get_map_entry(handle, &key).ok().flatten());

    let key_clone = key.clone();
    let sub =
        arena.subscribe_map_key(
            handle,
            key_clone,
            move |change: &MapChange<_, V>| match change {
                MapChange::Insert { value, .. }
                | MapChange::Update {
                    new_value: value, ..
                } => {
                    set_signal.set(Some(value.clone()));
                }
                MapChange::Remove { .. } | MapChange::Clear { .. } => {
                    set_signal.set(None);
                }
            },
        );
    on_cleanup(move || drop(sub));

    signal
}

pub fn use_map_subscribe_any<K, V, M, F>(handle: MapHandle<K, V, M>, callback: F)
where
    K: ReactiveMapKey + for<'de> Deserialize<'de>,
    V: ReactiveMapValue,
    M: AccessMode,
    F: Fn(&MapChange<K, V>) + Send + Sync + 'static,
{
    let arena = use_context::<DefaultArena>().expect("amethystate-leptos: Arena not found");
    let sub = arena.subscribe_map_any(handle, callback);
    on_cleanup(move || drop(sub));
}

pub fn use_map_subscribe_key<K, V, M, F>(handle: MapHandle<K, V, M>, key: K, callback: F)
where
    K: ReactiveMapKey + for<'de> Deserialize<'de>,
    V: ReactiveMapValue,
    M: AccessMode,
    F: Fn(&MapChange<K, V>) + Send + Sync + 'static,
{
    let arena = use_context::<DefaultArena>().expect("amethystate-leptos: Arena not found");
    let sub = arena.subscribe_map_key(handle, key, callback);
    on_cleanup(move || drop(sub));
}
