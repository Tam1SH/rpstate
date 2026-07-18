use crate::migration::fields::AmeStateFields;
use crate::store::StorageResult;
use crate::store::Store;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub trait AmeStateNode<S: Store>: Sized {
    fn new_node(store: &S, path: &str) -> StorageResult<Self>;
    fn new_node_with_id(store: &S, path: &str, instance_id: Uuid) -> StorageResult<Self>;
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
