use crate::SubscriptionKind;
use crate::store::error::{StorageError, StorageResult};
use crate::store::{StoreEvent, SubscriptionEntry};
#[cfg(feature = "sqlite")]
use amethystate_core::path::SEPARATOR;
use amethystate_core::path::StorePath;
use error_stack::ResultExt;
use parking_lot::RwLock;
use std::path::Path;

#[cfg(any(feature = "redb", feature = "sqlite"))]
pub use buffered::*;

pub trait Attempted: ResultExt {
    fn doing(self, what: StorageError, file: &Path) -> StorageResult<Self::Ok>;
}

impl<R: ResultExt> Attempted for R {
    fn doing(self, what: StorageError, file: &Path) -> StorageResult<Self::Ok> {
        self.change_context(what)
            .attach_with(|| format!("file: {}", file.display()))
    }
}

/// Reports what a store's closing flush did, from the `Drop` where nothing
/// else can.
///
/// That flush is the one a short-lived process depends on, and the one whose
/// failure nobody is in a position to see: a locked file, a full disk, a
/// permission error on the way out, and the process ends reporting success
/// with the data not written. `Drop` cannot return an error and cannot be
/// given a caller to hand one to, so a log line is the whole of what the loss
/// can leave behind - which is why it is at `error` rather than `warn`. A
/// caller that would rather find out while it can still act calls `save_now`
/// or `close` and reads the result.
pub fn report_closing_flush(outcome: StorageResult<()>, file: &Path) {
    if let Err(report) = outcome {
        tracing::error!(
            target: "amethystate",
            file = %file.display(),
            error = ?report,
            "the store's closing flush failed: what it still held is not on disk",
        );
    }
}

/// Lays the write buffer over what the engine holds, both already sorted.
///
/// One pass down two lists rather than a tree built from one and searched by
/// the other: a buffered write replaces the committed value at its key, a
/// buffered delete leaves nothing there, and everything keeps the order the
/// engine ranges in - which is the order a scan promises.
#[cfg(any(feature = "redb", feature = "sqlite"))]
pub fn merge_buffered(
    committed: Vec<(StorePath, Vec<u8>)>,
    buffered: Vec<(StorePath, Option<Vec<u8>>)>,
) -> Vec<(StorePath, Vec<u8>)> {
    let mut out = Vec::with_capacity(committed.len() + buffered.len());
    let mut left = committed.into_iter().peekable();
    let mut right = buffered.into_iter().peekable();

    loop {
        let take_left = match (left.peek(), right.peek()) {
            (Some((a, _)), Some((b, _))) => a <= b,
            (Some(_), None) => true,
            (None, Some(_)) => false,
            (None, None) => break,
        };

        if take_left {
            let (key, value) = left.next().expect("peeked");
            // A buffered op at the same key wins, and is taken on its own turn.
            if right.peek().is_some_and(|(b, _)| *b == key) {
                continue;
            }
            out.push((key, value));
        } else {
            let (key, value) = right.next().expect("peeked");
            if let Some(value) = value {
                out.push((key, value));
            }
        }
    }

    out
}

/// Where a subtree stops, for engines that store a key whole.
///
/// A key belongs to `prefix` when it is `prefix` itself or begins with it
/// followed by a separator - comparing the strings alone puts `uix.width` under
/// `ui`. At the root there is no bound to spell, since no key can begin with a
/// separator, and everything is under it.
#[cfg(any(feature = "redb", feature = "sqlite"))]
pub fn subtree_bound(prefix: &StorePath) -> Option<String> {
    (!prefix.is_root()).then(|| format!("{}.", prefix.as_str()))
}

/// The half-open key range a subtree occupies, for an engine that queries by
/// comparison rather than by iterating.
///
/// A pattern would have to escape whatever the pattern language treats as
/// special, and a name is allowed to hold any of it - `panel[0]` is a name.
/// Comparison has no such vocabulary, and an index can serve it.
///
/// The upper bound is the separator's byte successor rather than a high
/// character after it. `prefix.\u{10FFFF}` reads as "surely nothing sorts
/// above that", and a child named `\u{10FFFF}z` does: it was written fine,
/// read fine by its own path, and was invisible to every scan of the level
/// above it - so it also survived a delete of its own subtree. `prefix/`
/// admits every `prefix.` and nothing else, whatever the name after it.
///
/// Neither bound is exact on its own. The low end is the prefix itself, so a
/// sibling whose next character sorts below the separator falls inside the
/// range; [`is_under`] is what settles that, and every caller has to apply it
/// to the rows as well as to the buffer.
#[cfg(feature = "sqlite")]
pub fn key_range(prefix: &StorePath) -> (String, String) {
    if prefix.is_root() {
        return (String::new(), "\u{10FFFF}".to_string());
    }

    let low = prefix.as_str().to_string();
    let mut high = low.clone();
    high.push((SEPARATOR as u8 + 1) as char);
    (low, high)
}

#[cfg(any(feature = "redb", feature = "sqlite"))]
pub fn is_under(key: &str, prefix: &str, bound: &Option<String>) -> bool {
    match bound {
        Some(bound) => key == prefix || key.starts_with(bound.as_str()),
        None => true,
    }
}

/// A key read back out of storage, as the path it claims to be.
///
/// Every key a scan hands back is one this library could have written, so this
/// fails only where something else did the writing - an older build, or a hand
/// edit. Failing names the key rather than dropping it, since a key nothing can
/// address is worse unsaid.
pub fn stored_path(key: &str) -> StorageResult<StorePath> {
    StorePath::parse_joined(key)
        .change_context(StorageError::Scan)
        .attach_with(|| format!("stored key: {key}"))
        .attach("the store holds a key this library could not have written")
}

/// The key a namespace's initialization marker is stored under, in the same
/// table as data - redb and sqlite keep no table of their own for it.
#[cfg(any(feature = "redb", feature = "sqlite"))]
pub fn init_key(namespace: &str) -> String {
    format!("__init::{namespace}")
}

pub fn emit_events(subs_lock: &RwLock<Vec<SubscriptionEntry>>, event: StoreEvent) {
    let callbacks = {
        let guard = subs_lock.read();
        guard
            .iter()
            .filter(|s| matches_kind(&s.kind, &event.path))
            .map(|s| s.callback.clone())
            .collect::<Vec<_>>()
    };
    for cb in callbacks {
        cb(&event);
    }
}

fn matches_kind(kind: &SubscriptionKind, path: &str) -> bool {
    match kind {
        SubscriptionKind::Any => true,
        SubscriptionKind::ExactPath(p) => **p == *path,
        SubscriptionKind::Prefix(prefix) => {
            *path == **prefix
                || path
                    .strip_prefix(&**prefix)
                    .is_some_and(|t| t.starts_with('.'))
        }
    }
}

#[cfg(any(feature = "redb", feature = "sqlite"))]
mod buffered {
    use super::{SubscriptionEntry, emit_events};
    use crate::store::StoreEvent;
    use crate::store::util::debouncer::Debouncer;
    use crate::{StorageResult, StoreOp};
    use parking_lot::{Mutex, RwLock};
    use std::sync::Arc;

    /// One buffered write, waiting for the next flush.
    ///
    /// `Init` targets the metadata table rather than the data one; keeping it
    /// in the same buffer is what makes a namespace flag land in the same
    /// transaction as the values it vouches for.
    ///
    /// It carries the flag rather than there being one variant per direction,
    /// so setting and clearing it stay one branch wherever it is handled - and
    /// there are four of those, two per flat engine.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub enum PendingOp {
        Set(Vec<u8>),
        Delete,
        Init(bool),
    }

    impl PendingOp {
        /// The value a reader should see, or `None` where the key is gone.
        pub fn value(&self) -> Option<&[u8]> {
            match self {
                Self::Set(bytes) => Some(bytes),
                Self::Delete | Self::Init(_) => None,
            }
        }

        pub fn is_data(&self) -> bool {
            matches!(self, Self::Set(_) | Self::Delete)
        }
    }

    pub type Pending = std::collections::HashMap<Arc<str>, PendingOp>;

    /// Everything buffered under `prefix`, left in place.
    ///
    /// The buffer is only cleared once the write has actually landed, by
    /// [`clear_committed`]. Taking entries out first meant any error below lost
    /// them: not on disk, not in memory, and nothing left to retry.
    pub fn pending_prefix(pending: &Pending, prefix: &str) -> Pending {
        if pending.is_empty() {
            return Pending::new();
        }

        if prefix.is_empty() {
            return pending.clone();
        }

        let prefix_dot = format!("{}.", prefix);
        pending
            .iter()
            .filter(|(k, _)| k.starts_with(&prefix_dot) || &***k == prefix)
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect()
    }

    /// Drops from the buffer exactly what was committed.
    ///
    /// A key whose buffered value has changed since is a write that landed while
    /// the commit was in flight; it is not on disk, so it stays for the next one.
    pub fn clear_committed(pending: &mut Pending, committed: &Pending) {
        for (key, value) in committed {
            if pending.get(key) == Some(value) {
                pending.remove(key);
            }
        }
    }

    pub fn set_raw_pending(
        pending: &Mutex<Pending>,
        subscriptions: &RwLock<Vec<SubscriptionEntry>>,
        debouncer: &Debouncer,
        key: &str,
        value: &[u8],
    ) -> StorageResult<()> {
        let key_arc: Arc<str> = Arc::from(key);
        let old_bytes = {
            let lock = pending.lock();
            lock.get(&*key_arc).and_then(|op| op.value().map(Vec::from))
        };
        {
            let mut lock = pending.lock();
            lock.insert(key_arc.clone(), PendingOp::Set(value.to_vec()));
        }
        emit_events(
            subscriptions,
            StoreEvent {
                path: key_arc,
                op: StoreOp::Set,
                old: old_bytes,
                new: Some(value.to_vec()),
                source: None,
            },
        );
        debouncer.schedule();
        Ok(())
    }
}

#[cfg(all(test, any(feature = "redb", feature = "sqlite")))]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn buffer(entries: &[(&str, Option<&[u8]>)]) -> Pending {
        entries
            .iter()
            .map(|(k, v)| {
                (
                    Arc::from(*k),
                    match v {
                        Some(b) => PendingOp::Set(b.to_vec()),
                        None => PendingOp::Delete,
                    },
                )
            })
            .collect()
    }

    #[test]
    fn collecting_leaves_the_buffer_alone() {
        let pending = buffer(&[("a.x", Some(b"1")), ("a.y", Some(b"2"))]);

        let taken = pending_prefix(&pending, "a");

        assert_eq!(taken.len(), 2);
        assert_eq!(
            pending.len(),
            2,
            "entries must survive until the write lands, or a failure below \
             loses them from memory and disk both"
        );
    }

    #[test]
    fn an_empty_prefix_means_everything() {
        let pending = buffer(&[("a.x", Some(b"1")), ("b.y", Some(b"2"))]);
        assert_eq!(pending_prefix(&pending, "").len(), 2);
    }

    #[test]
    fn a_prefix_matches_its_own_key_and_its_children() {
        let pending = buffer(&[
            ("a", Some(b"root")),
            ("a.x", Some(b"child")),
            ("ab", Some(b"sibling")),
        ]);

        let taken = pending_prefix(&pending, "a");

        assert!(taken.contains_key("a"));
        assert!(taken.contains_key("a.x"));
        assert!(!taken.contains_key("ab"), "a prefix is not a substring");
    }

    #[test]
    fn committed_entries_are_dropped() {
        let mut pending = buffer(&[("a.x", Some(b"1")), ("a.y", Some(b"2"))]);
        let committed = pending.clone();

        clear_committed(&mut pending, &committed);

        assert!(pending.is_empty());
    }

    #[test]
    fn a_value_that_changed_during_the_commit_survives() {
        let committed = buffer(&[("a.x", Some(b"old"))]);
        let mut pending = buffer(&[("a.x", Some(b"new"))]);

        clear_committed(&mut pending, &committed);

        assert_eq!(
            pending.get("a.x"),
            Some(&PendingOp::Set(b"new".to_vec())),
            "the newer write is not on disk, so dropping it would lose it"
        );
    }

    #[test]
    fn a_key_written_after_the_commit_survives() {
        let committed = buffer(&[("a.x", Some(b"1"))]);
        let mut pending = buffer(&[("a.x", Some(b"1")), ("a.z", Some(b"9"))]);

        clear_committed(&mut pending, &committed);

        assert!(!pending.contains_key("a.x"));
        assert!(pending.contains_key("a.z"), "it was never committed");
    }

    #[test]
    fn a_pending_delete_is_committed_like_any_other_entry() {
        let mut pending = buffer(&[("a.x", None)]);
        let committed = pending.clone();

        clear_committed(&mut pending, &committed);

        assert!(pending.is_empty());
    }
}
