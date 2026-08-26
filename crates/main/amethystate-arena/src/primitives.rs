use slotmap::DefaultKey;
use std::marker::PhantomData;

pub struct FieldHandle<T> {
    pub key: DefaultKey,
    pub _marker: PhantomData<T>,
}

impl<T> Copy for FieldHandle<T> {}
impl<T> Clone for FieldHandle<T> {
    fn clone(&self) -> Self {
        *self
    }
}

pub struct PipelineHandle<T> {
    pub key: DefaultKey,
    pub _marker: PhantomData<T>,
}

impl<T> Copy for PipelineHandle<T> {}
impl<T> Clone for PipelineHandle<T> {
    fn clone(&self) -> Self {
        *self
    }
}

pub struct MapHandle<K, V> {
    pub key: DefaultKey,
    pub _marker: PhantomData<(K, V)>,
}

impl<K, V> Copy for MapHandle<K, V> {}
impl<K, V> Clone for MapHandle<K, V> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> PartialEq for FieldHandle<T> {
    fn eq(&self, other: &Self) -> bool {
        self.key == other.key
    }
}

impl<K, V> PartialEq for MapHandle<K, V> {
    fn eq(&self, other: &Self) -> bool {
        self.key == other.key
    }
}

impl<T> PartialEq for PipelineHandle<T> {
    fn eq(&self, other: &Self) -> bool {
        self.key == other.key
    }
}
