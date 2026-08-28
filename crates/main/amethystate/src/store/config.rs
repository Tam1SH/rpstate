use crate::store::StorageError;
use crate::store::builder::Backend;
use error_stack::Report;
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

/// How hard one step of a file write is fought before it is given up on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WriteAttempts {
    /// How many times the step is tried, the first one included. One means no
    /// retry at all.
    pub attempts: usize,

    /// How long to wait between them.
    pub pause: Duration,
}

impl WriteAttempts {
    /// Tried this many times, back to back until [`apart`] says otherwise.
    ///
    /// [`apart`]: WriteAttempts::apart
    pub const fn times(attempts: usize) -> Self {
        Self {
            attempts,
            pause: Duration::ZERO,
        }
    }

    /// How long to wait between them.
    pub const fn apart(self, pause: Duration) -> Self {
        Self { pause, ..self }
    }

    /// Tried once, reported the moment it fails.
    pub const fn once() -> Self {
        Self::times(1)
    }

    /// The longest a step spends waiting before the failure is reported.
    ///
    /// There is one pause fewer than there are attempts: the last failure is
    /// not slept on.
    pub const fn budget(self) -> Duration {
        Duration::from_nanos(self.pause.as_nanos() as u64 * self.attempts.saturating_sub(1) as u64)
    }
}

/// The two ways writing a file fails, and what each is worth waiting out.
///
/// A text engine writes the whole document to a file of its own and then
/// replaces the target with it, so a reader meets either the old file or the
/// new one. Those two steps fail for unrelated reasons and deserve unrelated
/// budgets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileWritePolicy {
    /// Getting the bytes into a file of their own. This is ordinary I/O: a
    /// full disk or a dead device stays that way, so a few quick attempts are
    /// all it is worth.
    pub write: WriteAttempts,

    /// Replacing the target with it. An antivirus, an indexer or a cloud
    /// client holding the file lets go on its own, so the same call succeeds a
    /// moment later - which is why this budget is the longer of the two, and
    /// why raising it costs a caller nothing until something really is stuck.
    pub replace: WriteAttempts,
}

impl FileWritePolicy {
    /// How hard getting the bytes into a file of their own is fought.
    pub const fn writing(self, attempts: WriteAttempts) -> Self {
        Self {
            write: attempts,
            ..self
        }
    }

    /// How hard replacing the target with it is fought.
    pub const fn replacing(self, attempts: WriteAttempts) -> Self {
        Self {
            replace: attempts,
            ..self
        }
    }
}

impl Default for FileWritePolicy {
    fn default() -> Self {
        Self {
            write: WriteAttempts::times(3).apart(Duration::from_millis(15)),
            replace: WriteAttempts::times(5).apart(Duration::from_millis(100)),
        }
    }
}

/// What a store refuses to hold.
///
/// Three things get confused as one and are not:
///
/// - **The codec's ceiling is a fact.** `ron` will not read past 64 levels
///   whatever anyone configures. A write past it produces a file that codec
///   cannot read, so it is refused always, and there is nothing here to turn
///   that off. See [`Backend::depth_ceiling`].
/// - **Key depth is a setting**, and it is the one below. How deep a path may
///   go is the application's business - it knows the shape of its own paths -
///   and capping it also reserves the rest of the shared budget for values,
///   which is the half nobody thinks about: sixty levels of path on ron leaves
///   four for whatever is stored there.
/// - **Portability is a policy**, and depth is one row of it. What each engine
///   cannot hold is a table, not a number: non-finite floats on json, anything
///   past `i64` on toml, a unit enum variant on ron, a non-string map key on
///   every text engine.
///
/// [`Backend::depth_ceiling`]: crate::store::builder::Backend::depth_ceiling
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WriteLimits {
    /// How many levels a path may have, or no limit of the store's own.
    ///
    /// The codec's ceiling still applies either way, and the path spends the
    /// same budget the value does.
    pub key_depth: Option<usize>,

    /// Engines this store's contents must remain readable on, beyond the one
    /// actually running.
    ///
    /// Empty means no such claim, which is the default: a store nobody intends
    /// to move has no reason to pay for it.
    pub portable_across: Vec<Backend>,
}

impl WriteLimits {
    /// At most this many levels in a path.
    pub fn key_depth(mut self, levels: usize) -> Self {
        self.key_depth = Some(levels);
        self
    }

    /// Refuse whatever these engines could not give back, whichever one is
    /// running.
    ///
    /// The set rather than *all*, because "all" is a moving target - it changes
    /// under a store when an engine is added - and because the honest
    /// requirement is usually narrower: an application shipping json on the
    /// desktop and sqlite on a phone needs those two and has no opinion about
    /// ron.
    pub fn portable_across(mut self, engines: impl IntoIterator<Item = Backend>) -> Self {
        self.portable_across = engines.into_iter().collect();
        self
    }

    /// The deepest a path and its value may go together, given the engine that
    /// is running and whatever else this store promised to stay readable on.
    pub fn ceiling(&self, running: Backend) -> usize {
        self.portable_across
            .iter()
            .map(|engine| engine.depth_ceiling())
            .chain(std::iter::once(running.depth_ceiling()))
            .min()
            .unwrap_or_else(|| running.depth_ceiling())
    }
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
/// once per streak, after anyone awaiting that flush has already been told it
/// failed. What it returns decides what writers see next; without one the
/// store defaults to [`AfterGivingUp::Fail`].
///
/// It is handed the failure itself rather than a rendering of it, because the
/// decision usually turns on which failure it is. A full disk is someone
/// deleting something in a minute, and [`AfterGivingUp::Ignore`] rides it out;
/// a value the format cannot hold will never be writable, and retrying it
/// every interval for the life of the process is not waiting for anything.
/// `report.current_context()` says which, and `{report:#}` renders it when
/// that is what is wanted.
pub type PersistFailureCallback = Arc<dyn Fn(&Report<StorageError>) -> AfterGivingUp + Send + Sync>;

pub struct StoreConfig {
    pub path: PathBuf,
    pub save_debounce: Duration,
    pub watch_interval: Duration,
    pub retry_policy: RetryPolicy,

    /// What one write to one file is worth, as against [`retry_policy`], which
    /// is what a flush is worth once the write under it has already failed.
    ///
    /// [`retry_policy`]: StoreConfig::retry_policy
    pub file_write: FileWritePolicy,

    /// What this store refuses to hold, as against what its codec cannot.
    pub limits: WriteLimits,

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
            file_write: FileWritePolicy::default(),
            limits: WriteLimits::default(),
            on_persist_failure: None,
            parallel_reads: false,
        }
    }
}
