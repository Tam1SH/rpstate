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
