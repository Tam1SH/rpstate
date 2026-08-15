use crate::Store;
use crate::migration::fields::AmeStateFields;
use crate::store::StorageResult;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub trait AmeStateNode: Sized {
    fn new_node(store: &Store, path: &str) -> StorageResult<Self>;
    fn new_node_with_id(store: &Store, path: &str, instance_id: Uuid) -> StorageResult<Self>;
}

pub trait AmeState {
    type Data: AmeStateFields
        + Serialize
        + for<'de> Deserialize<'de>
        + Clone
        + Send
        + Sync
        + 'static;
}
