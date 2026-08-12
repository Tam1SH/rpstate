use crate::primitives::*;
use amethystate::reactive::error::{ReactiveFieldResult, ReactiveMapResult};
use amethystate::store::Store;
use amethystate::{
    AccessMode, Field, MapChange, Pipeline, Reactive, ReactiveMap, ReactiveMapKey,
    ReactiveMapValue, SignalSubscription, WritableMode,
};
use parking_lot::RwLock;
use serde::{Serialize, de::DeserializeOwned};
use slotmap::{DefaultKey, SlotMap};
use std::any::Any;
use std::marker::PhantomData;
use std::sync::Arc;
use uuid::Uuid;

type ErasedItem = Box<dyn Any + Send + Sync>;

#[derive(Clone)]
pub struct Arena<S: Store> {
    storage: Arc<RwLock<SlotMap<DefaultKey, ErasedItem>>>,
    _backend: PhantomData<S>,
}

impl<S: Store> Default for Arena<S> {
    fn default() -> Self {
        Self::new()
    }
}

impl<S: Store> Arena<S> {
    pub fn new() -> Self {
        Self {
            storage: Arc::new(RwLock::new(SlotMap::new())),
            _backend: PhantomData,
        }
    }

    pub fn with_item<Item, R, F>(&self, key: DefaultKey, type_name: &str, f: F) -> R
    where
        Item: Any,
        F: FnOnce(&Item) -> R,
    {
        let storage = self.storage.read();
        let item = storage.get(key).unwrap_or_else(|| {
            panic!(
                "amethystate-arena: Attempted to access a dropped {}",
                type_name
            )
        });
        let target = item
            .downcast_ref::<Item>()
            .unwrap_or_else(|| panic!("amethystate-arena: Type mismatch for {}", type_name));
        f(target)
    }

    pub fn register_field<T, M>(&self, field: Field<T, S, M>) -> FieldHandle<T, M>
    where
        T: Send + Sync + 'static,
        M: AccessMode,
    {
        let key = self.storage.write().insert(Box::new(field));
        FieldHandle {
            key,
            _marker: PhantomData,
        }
    }

    pub fn get_field<T, M>(&self, handle: FieldHandle<T, M>) -> T
    where
        T: DeserializeOwned + Serialize + Clone + Send + Sync + 'static,
        M: AccessMode,
    {
        self.with_item::<Field<T, S, M>, _, _>(handle.key, "Field", |field| field.get())
    }

    pub fn set_field<T>(&self, handle: WritableHandle<T>, value: T) -> ReactiveFieldResult<()>
    where
        T: DeserializeOwned + Serialize + Clone + Send + Sync + 'static,
    {
        self.with_item::<Field<T, S, WritableMode>, _, _>(handle.key, "Field", |field| {
            field.set(value)
        })
    }

    pub fn subscribe_external_field<T, M, F>(
        &self,
        handle: FieldHandle<T, M>,
        callback: F,
    ) -> SignalSubscription
    where
        T: DeserializeOwned + Serialize + Clone + Send + Sync + 'static,
        M: AccessMode,
        F: for<'a> Fn(&'a T) + Send + Sync + 'static,
    {
        self.with_item::<Field<T, S, M>, _, _>(handle.key, "Field", |field| {
            field.subscribe_external(callback)
        })
    }

    pub fn subscribe_field<T, M, F>(
        &self,
        handle: FieldHandle<T, M>,
        callback: F,
    ) -> SignalSubscription
    where
        T: DeserializeOwned + Serialize + Clone + Send + Sync + 'static,
        M: AccessMode,
        F: for<'a> Fn(&'a T) + Send + Sync + 'static,
    {
        self.with_item::<Field<T, S, M>, _, _>(handle.key, "Field", |field| {
            field.subscribe(callback)
        })
    }
    pub fn subscribe_field_with_source<T, M, F>(
        &self,
        handle: FieldHandle<T, M>,
        callback: F,
    ) -> SignalSubscription
    where
        T: DeserializeOwned + Serialize + Clone + Send + Sync + 'static,
        M: AccessMode,
        F: for<'a> Fn(&'a T, Option<Uuid>) + Send + Sync + 'static,
    {
        self.with_item::<Field<T, S, M>, _, _>(handle.key, "Field", |field| {
            field.subscribe_with_source(callback)
        })
    }

    pub fn register_pipeline<T>(&self, pipeline: Pipeline<T>) -> PipelineHandle<T>
    where
        T: Send + Sync + 'static,
    {
        let key = self.storage.write().insert(Box::new(pipeline));
        PipelineHandle {
            key,
            _marker: PhantomData,
        }
    }

    pub fn get_pipeline<T>(&self, handle: PipelineHandle<T>) -> T
    where
        T: Clone + Send + Sync + 'static,
    {
        self.with_item::<Pipeline<T>, _, _>(handle.key, "Pipeline", |pipe| pipe.get())
    }

    pub fn subscribe_pipeline<T, F>(
        &self,
        handle: PipelineHandle<T>,
        callback: F,
    ) -> SignalSubscription
    where
        T: Clone + Send + Sync + 'static,
        F: for<'a> Fn(&'a T) + Send + Sync + 'static,
    {
        self.with_item::<Pipeline<T>, _, _>(handle.key, "Pipeline", |pipe| pipe.subscribe(callback))
    }

    pub fn register_map<K, V, M>(&self, map: ReactiveMap<K, V, S, M>) -> MapHandle<K, V, M>
    where
        K: ReactiveMapKey,
        V: ReactiveMapValue,
        M: AccessMode,
    {
        let key = self.storage.write().insert(Box::new(map));
        MapHandle {
            key,
            _marker: PhantomData,
        }
    }

    pub fn get_map_entry<K, V, M>(
        &self,
        handle: MapHandle<K, V, M>,
        key: &K,
    ) -> ReactiveMapResult<Option<V>>
    where
        K: ReactiveMapKey,
        V: ReactiveMapValue,
        M: AccessMode,
    {
        self.with_item::<ReactiveMap<K, V, S, M>, _, _>(handle.key, "ReactiveMap", |map| {
            map.get(key)
        })
    }

    pub fn set_map_entry<K, V>(
        &self,
        handle: WritableMapHandle<K, V>,
        key: K,
        value: V,
    ) -> ReactiveMapResult<()>
    where
        K: ReactiveMapKey,
        V: ReactiveMapValue,
    {
        self.with_item::<ReactiveMap<K, V, S, WritableMode>, _, _>(
            handle.key,
            "ReactiveMap",
            |map| map.set_or_create(key, &value),
        )
    }

    pub fn subscribe_map_any<K, V, M, F>(
        &self,
        handle: MapHandle<K, V, M>,
        callback: F,
    ) -> SignalSubscription
    where
        K: ReactiveMapKey,
        V: ReactiveMapValue,
        M: AccessMode,
        F: Fn(&MapChange<K, V>) + Send + Sync + 'static,
    {
        self.with_item::<ReactiveMap<K, V, S, M>, _, _>(handle.key, "ReactiveMap", |map| {
            map.subscribe_any(callback)
        })
    }

    pub fn subscribe_map_any_external<K, V, M, F>(
        &self,
        handle: MapHandle<K, V, M>,
        callback: F,
    ) -> SignalSubscription
    where
        K: ReactiveMapKey,
        V: ReactiveMapValue,
        M: AccessMode,
        F: Fn(&MapChange<K, V>) + Send + Sync + 'static,
    {
        self.with_item::<ReactiveMap<K, V, S, M>, _, _>(handle.key, "ReactiveMap", |map| {
            map.subscribe_any_external(callback)
        })
    }

    pub fn subscribe_map_key_external<K, V, M, F>(
        &self,
        handle: MapHandle<K, V, M>,
        key: K,
        callback: F,
    ) -> SignalSubscription
    where
        K: ReactiveMapKey,
        V: ReactiveMapValue,
        M: AccessMode,
        F: Fn(&MapChange<K, V>) + Send + Sync + 'static,
    {
        self.with_item::<ReactiveMap<K, V, S, M>, _, _>(handle.key, "ReactiveMap", |map| {
            map.subscribe_key_external(key, callback)
        })
    }

    pub fn subscribe_map_key<K, V, M, F>(
        &self,
        handle: MapHandle<K, V, M>,
        key: K,
        callback: F,
    ) -> SignalSubscription
    where
        K: ReactiveMapKey,
        V: ReactiveMapValue,
        M: AccessMode,
        F: Fn(&MapChange<K, V>) + Send + Sync + 'static,
    {
        self.with_item::<ReactiveMap<K, V, S, M>, _, _>(handle.key, "ReactiveMap", |map| {
            map.subscribe_key(key, callback)
        })
    }

    pub fn remove_pipeline<T>(&self, handle: PipelineHandle<T>) {
        self.storage.write().remove(handle.key);
    }

    pub fn get_map_entries<K, V, M>(
        &self,
        handle: MapHandle<K, V, M>,
    ) -> ReactiveMapResult<Vec<(K, V)>>
    where
        K: ReactiveMapKey,
        V: ReactiveMapValue,
        M: AccessMode,
    {
        self.with_item::<ReactiveMap<K, V, S, M>, _, _>(handle.key, "ReactiveMap", |map| {
            map.entries().map(|vec| vec.into_iter().collect())
        })
    }

    pub fn remove_map_entry<K, V>(
        &self,
        handle: WritableMapHandle<K, V>,
        key: K,
    ) -> ReactiveMapResult<Option<V>>
    where
        K: ReactiveMapKey,
        V: ReactiveMapValue,
    {
        self.with_item::<ReactiveMap<K, V, S, WritableMode>, _, _>(
            handle.key,
            "ReactiveMap",
            |map| map.remove(key),
        )
    }

    pub fn clear_map<K, V>(&self, handle: WritableMapHandle<K, V>) -> ReactiveMapResult<()>
    where
        K: ReactiveMapKey,
        V: ReactiveMapValue,
    {
        self.with_item::<ReactiveMap<K, V, S, WritableMode>, _, _>(
            handle.key,
            "ReactiveMap",
            |map| map.clear(),
        )
    }
}

impl<S: Store> PartialEq for Arena<S> {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.storage, &other.storage)
    }
}

impl<S: Store> Eq for Arena<S> {}
