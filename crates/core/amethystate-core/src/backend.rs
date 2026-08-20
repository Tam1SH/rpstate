use crate::path::StorePath;
use serde::Serialize;
use serde::de::DeserializeOwned;
use std::borrow::Borrow;
use uuid::Uuid;

pub trait AmeBackendSync {
    type Error;
    type Raw: Borrow<Self::Borrowed>;
    type Borrowed: ?Sized;

    fn get<T>(&self, path: &StorePath) -> Result<Option<T>, Self::Error>
    where
        T: DeserializeOwned;

    fn set_owned<T: Serialize>(&self, path: StorePath, value: &T) -> Result<(), Self::Error> {
        self.set(&path, value)
    }

    fn set_with_source<T: Serialize>(
        &self,
        path: &StorePath,
        value: &T,
        source: Option<Uuid>,
    ) -> Result<(), Self::Error>;
    fn set_owned_with_source<T: Serialize>(
        &self,
        path: StorePath,
        value: &T,
        source: Option<Uuid>,
    ) -> Result<(), Self::Error>;

    fn set<T>(&self, path: &StorePath, value: &T) -> Result<(), Self::Error>
    where
        T: Serialize;

    fn delete(&self, path: &StorePath) -> Result<(), Self::Error>;

    fn delete_with_source(&self, path: &StorePath, source: Option<Uuid>)
    -> Result<(), Self::Error>;

    /// Removes every key under `prefix` as one operation, emitting a single
    /// event rather than one per key.
    fn delete_prefix(&self, prefix: &StorePath, source: Option<Uuid>) -> Result<(), Self::Error>;

    fn scan_prefix(&self, prefix: &StorePath) -> Result<Vec<(String, Self::Raw)>, Self::Error>;

    /// The keys under `prefix`, sorted, without reading their values.
    fn scan_keys(&self, prefix: &StorePath) -> Result<Vec<String>, Self::Error>;

    fn decode<T>(&self, raw: &Self::Borrowed) -> Result<T, Self::Error>
    where
        T: DeserializeOwned + Default;
}
#[cfg(feature = "async")]
#[allow(async_fn_in_trait)]
pub trait AmeBackendAsync {
    type Error;
    type Raw;

    async fn get<T>(&self, path: &StorePath) -> Result<Option<T>, Self::Error>
    where
        T: DeserializeOwned;

    async fn set<T>(&self, path: &StorePath, value: &T) -> Result<(), Self::Error>
    where
        T: Serialize;
    async fn set_with_source<T: Serialize>(
        &self,
        path: &StorePath,
        value: &T,
        source: Option<Uuid>,
    ) -> Result<(), Self::Error>;
    async fn set_owned_with_source<T: Serialize>(
        &self,
        path: StorePath,
        value: &T,
        source: Option<Uuid>,
    ) -> Result<(), Self::Error>;

    async fn delete(&self, path: &StorePath) -> Result<(), Self::Error>;
    async fn delete_with_source(
        &self,
        path: &StorePath,
        source: Option<Uuid>,
    ) -> Result<(), Self::Error>;

    async fn scan_prefix(
        &self,
        prefix: &StorePath,
    ) -> Result<Vec<(String, Self::Raw)>, Self::Error>;

    fn decode<T>(&self, raw: &Self::Raw) -> Result<T, Self::Error>
    where
        T: DeserializeOwned + Default;
}
