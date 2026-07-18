use crate::SubscriptionKind;
#[cfg(any(feature = "redb", feature = "sqlite"))]
use crate::store::util::debouncer::Debouncer;
use crate::store::{StoreEvent, SubscriptionEntry};
#[cfg(any(feature = "redb", feature = "sqlite"))]
use crate::{StorageResult, StoreOp};
#[cfg(any(feature = "redb", feature = "sqlite"))]
use parking_lot::Mutex;
use parking_lot::RwLock;
#[cfg(any(feature = "redb", feature = "sqlite"))]
use std::sync::Arc;

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

#[cfg(any(feature = "sqlite", feature = "redb"))]
pub fn drain_pending_prefix(
    pending: &mut std::collections::HashMap<std::sync::Arc<str>, Option<Vec<u8>>>,
    prefix: &str,
) -> std::collections::HashMap<std::sync::Arc<str>, Option<Vec<u8>>> {
    use std::collections::HashMap;
    use std::sync::Arc;

    if pending.is_empty() {
        return HashMap::new();
    }

    if prefix.is_empty() {
        std::mem::take(pending)
    } else {
        let prefix_dot = format!("{}.", prefix);
        let keys_to_remove: Vec<Arc<str>> = pending
            .keys()
            .filter(|k| k.starts_with(&prefix_dot) || &***k == prefix)
            .cloned()
            .collect();

        let mut matched = HashMap::with_capacity(keys_to_remove.len());
        for k in keys_to_remove {
            if let Some(v) = pending.remove(&k) {
                matched.insert(k, v);
            }
        }
        matched
    }
}

#[cfg(any(feature = "redb", feature = "sqlite"))]
pub fn set_raw_pending(
    pending: &Mutex<std::collections::HashMap<Arc<str>, Option<Vec<u8>>>>,
    subscriptions: &RwLock<Vec<SubscriptionEntry>>,
    debouncer: &Debouncer,
    key: &str,
    value: &[u8],
) -> StorageResult<()> {
    let key_arc: Arc<str> = Arc::from(key);
    let old_bytes = {
        let lock = pending.lock();
        lock.get(&*key_arc).cloned().flatten()
    };
    {
        let mut lock = pending.lock();
        lock.insert(key_arc.clone(), Some(value.to_vec()));
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
