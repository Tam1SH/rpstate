use std::sync::Arc;
use uuid::Uuid;

pub type SubscriptionId = u64;
pub type StoreCallback = Arc<dyn Fn(&StoreEvent) + Send + Sync + 'static>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoreOp {
    Set,
    Delete,

    /// Everything under a prefix went away as one operation. The event path is
    /// the prefix.
    DeletePrefix,
}

#[derive(Debug, Clone)]
pub struct StoreEvent {
    pub path: Arc<str>,
    pub op: StoreOp,
    pub old: Option<Vec<u8>>,
    pub new: Option<Vec<u8>>,
    pub source: Option<Uuid>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SubscriptionKind {
    Any,
    ExactPath(Arc<str>),
    Prefix(Arc<str>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodecFormat {
    #[cfg(test)]
    Default,

    #[cfg(feature = "redb")]
    MessagePack,

    #[cfg(feature = "json")]
    Json,

    #[cfg(feature = "sqlite")]
    SonicJson,

    #[cfg(feature = "toml")]
    Toml,

    #[cfg(feature = "ron")]
    Ron,
}

#[derive(Clone)]
pub struct SubscriptionEntry {
    pub id: SubscriptionId,
    pub kind: SubscriptionKind,
    pub callback: StoreCallback,
}

/// A store subscription that ends when this is dropped.
///
/// Both fields are the store's to keep, not the holder's: `id` is the key the
/// store removes the entry by, and `store` is which store it is removed from.
/// While they were public, changing either turned the drop into something else
/// entirely - an id nobody registered removes nothing and leaks the callback, a
/// colliding one removes a stranger's subscription, and another store's handle
/// unsubscribes from the wrong place.
///
/// Here rather than beside `Field` because it is about a store: a map holds one
/// too, and the primitives factory is what builds them.
pub struct StoreSubscription {
    store: crate::Store,
    id: SubscriptionId,
}

impl StoreSubscription {
    pub(crate) fn new(store: crate::Store, id: SubscriptionId) -> Self {
        Self { store, id }
    }

    /// Which subscription this is, for a caller that wants to say so.
    pub fn id(&self) -> SubscriptionId {
        self.id
    }

    /// The store it is on, for a durable write that has to flush it.
    ///
    /// Lent rather than handed over: what a holder must not be able to do is
    /// *replace* it, since that is what the drop unsubscribes from.
    pub(crate) fn store(&self) -> &crate::Store {
        &self.store
    }
}

impl Drop for StoreSubscription {
    fn drop(&mut self) {
        self.store.unsubscribe(self.id);
    }
}
