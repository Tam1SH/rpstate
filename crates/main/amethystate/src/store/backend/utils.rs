use crate::SubscriptionKind;
use crate::store::{StoreEvent, SubscriptionEntry};
use parking_lot::RwLock;

#[cfg(any(feature = "redb", feature = "sqlite"))]
pub use buffered::*;

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
    /// `MarkInit` targets the metadata table rather than the data one; keeping it
    /// in the same buffer is what makes a namespace flag land in the same
    /// transaction as the values it vouches for.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub enum PendingOp {
        Set(Vec<u8>),
        Delete,
        MarkInit,
    }

    impl PendingOp {
        /// The value a reader should see, or `None` where the key is gone.
        pub fn value(&self) -> Option<&[u8]> {
            match self {
                Self::Set(bytes) => Some(bytes),
                Self::Delete | Self::MarkInit => None,
            }
        }

        pub fn is_data(&self) -> bool {
            matches!(self, Self::Set(_) | Self::Delete)
        }
    }

    pub type Pending = std::collections::HashMap<std::sync::Arc<str>, PendingOp>;

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
