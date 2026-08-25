use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

/// A background flush's whole retry policy: how long to wait between
/// attempts, and how long a streak of failures may run before the store
/// says so out loud.
///
/// `budget` is not how long the store keeps trying - it keeps trying until
/// it lands or the store is dropped, since a full disk is usually someone
/// deleting something in a minute. It is how long it stays quiet about it
/// before escalating.
#[derive(Clone)]
pub struct RetryPolicy {
    pub interval: Duration,
    pub budget: Duration,
}

/// What the store does about a flush that has been failing for longer than
/// the retry budget. It keeps retrying either way; this is only about who is
/// told.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AfterGivingUp {
    /// Every later write fails with [`StorageError::CommitFailed`], naming
    /// the reason, until a flush lands again. Reads carry on and what is
    /// buffered stays buffered, so a disk that frees up heals the store
    /// without a restart.
    ///
    /// [`StorageError::CommitFailed`]: crate::store::StorageError::CommitFailed
    Fail,

    /// Say nothing to writers. The retry loop carries on and the buffer is
    /// kept, so this is the choice for an application that would rather
    /// handle it in the callback than have its writes start failing.
    Ignore,

    /// Poison the writer: every later write panics wherever it is made. For
    /// an application that would rather stop than run on with state it
    /// cannot persist.
    Poison,
}

/// Runs when a flush has been failing for longer than the retry budget -
/// once per streak, with the rendered reason, after anyone awaiting that
/// flush has already been told it failed. What it returns decides what
/// writers see next; without one the store defaults to
/// [`AfterGivingUp::Fail`].
pub type PersistFailureCallback = Arc<dyn Fn(&str) -> AfterGivingUp + Send + Sync>;

pub struct StoreConfig {
    pub path: PathBuf,
    pub save_debounce: Duration,
    pub watch_interval: Duration,
    pub retry_policy: RetryPolicy,
    pub on_persist_failure: Option<PersistFailureCallback>,

    /// Whether reading a large collection back may use more than one core.
    ///
    /// Parsing every stored key and decoding every value is around four
    /// hundred milliseconds of a million-entry open, and dividing them takes
    /// that to about eighty. Off by default: this is a thread pool inside a
    /// state library, and an application that already has one should say
    /// whether it wants a second. Nothing is spawned while this is false -
    /// rayon builds its pool on first use.
    ///
    /// Small collections are unaffected either way. Below roughly a thousand
    /// entries the handing out costs more than the work, and the split does
    /// not happen there.
    pub parallel_reads: bool,
}

impl StoreConfig {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            save_debounce: Duration::from_millis(300),
            watch_interval: Duration::from_millis(500),
            retry_policy: RetryPolicy {
                interval: Duration::from_secs(5),
                budget: Duration::from_secs(60),
            },
            on_persist_failure: None,
            parallel_reads: false,
        }
    }
}
