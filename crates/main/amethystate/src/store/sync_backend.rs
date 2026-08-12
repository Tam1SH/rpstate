use crate::Store;

use crate::store::StorageError;
use amethystate_core::AmeBackendSync;
use serde::Serialize;
use serde::de::DeserializeOwned;
use std::sync::Arc;
use uuid::Uuid;

pub(crate) struct StoreBackend<S> {
    pub(crate) store: S,
}

impl<S> StoreBackend<S> {
    pub(crate) fn new(store: S) -> Self {
        Self { store }
    }
}

impl<S> AmeBackendSync for StoreBackend<S>
where
    S: Store,
{
    type Error = StorageError;
    type Raw = Vec<u8>;
    type Borrowed = [u8];

    fn get<T>(&self, path: &str) -> Result<Option<T>, Self::Error>
    where
        T: DeserializeOwned,
    {
        self.store.get(path)
    }

    fn set_with_source<T: Serialize>(
        &self,
        path: &str,
        value: &T,
        source: Option<Uuid>,
    ) -> Result<(), Self::Error> {
        self.store.set_with_source(path, value, source)
    }

    fn set_owned_with_source<T: Serialize>(
        &self,
        path: Arc<str>,
        value: &T,
        source: Option<Uuid>,
    ) -> Result<(), Self::Error> {
        self.store.set_owned_with_source(path, value, source)
    }

    fn set<T>(&self, path: &str, value: &T) -> Result<(), Self::Error>
    where
        T: Serialize,
    {
        self.store.set(path, value)
    }

    fn delete(&self, path: &str) -> Result<(), Self::Error> {
        self.store.delete(path)
    }

    fn delete_with_source(&self, path: &str, source: Option<Uuid>) -> Result<(), Self::Error> {
        self.store.delete_with_source(path, source)
    }

    fn delete_prefix(&self, prefix: &str, source: Option<Uuid>) -> Result<(), Self::Error> {
        self.store.delete_prefix_with_source(prefix, source)
    }

    fn scan_prefix(&self, prefix: &str) -> Result<Vec<(String, Self::Raw)>, Self::Error> {
        self.store.scan_prefix(prefix)
    }

    fn scan_keys(&self, prefix: &str) -> Result<Vec<String>, Self::Error> {
        self.store.scan_keys(prefix)
    }

    fn decode<T>(&self, raw: &[u8]) -> Result<T, Self::Error>
    where
        T: DeserializeOwned + Default,
    {
        self.store.decode(raw)
    }
}
