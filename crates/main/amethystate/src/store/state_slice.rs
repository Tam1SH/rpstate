use crate::Store;
use crate::store::error::StorageResult;
use amethystate_core::ReactiveScope;

pub trait StateScope {
    const PREFIX: &'static str;
}

pub trait AmeStateSlice: Sized {
    fn load_slice(store: &Store) -> StorageResult<Self>;

    fn subscribe_all<F>(&self, callback: F) -> ReactiveScope
    where
        F: Fn() + Send + Sync + 'static;

    fn subscribe_all_external<F>(&self, callback: F) -> ReactiveScope
    where
        F: Fn() + Send + Sync + 'static;
}
