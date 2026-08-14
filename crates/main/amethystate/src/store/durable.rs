use crate::StorageResult;
use crate::reactive::local::Wake;
use crate::store::traits::Store;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};

/// Resolves once the branch it was asked about is on disk.
///
/// The flush runs on its own thread rather than through the debouncer, which
/// only ever fires on its timer: a caller asking for durability wants it now,
/// not within the next interval. No executor is involved, so this stays usable
/// under any runtime.
pub struct Commit {
    wake: Arc<Wake>,
    slot: Arc<Mutex<Option<StorageResult<()>>>>,
}

impl Future for Commit {
    type Output = StorageResult<()>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        if let Some(result) = self.slot.lock().unwrap().take() {
            return Poll::Ready(result);
        }

        self.wake.park(cx);

        match self.slot.lock().unwrap().take() {
            Some(result) => Poll::Ready(result),
            None => Poll::Pending,
        }
    }
}

/// Writes the branch at `path` to disk, off the calling thread.
pub(crate) fn commit_branch_async<S: Store>(store: &S, path: Arc<str>) -> Commit {
    let wake = Arc::new(Wake::default());
    let slot = Arc::new(Mutex::new(None));

    let store = store.clone();
    let wake_worker = Arc::clone(&wake);
    let slot_worker = Arc::clone(&slot);

    std::thread::spawn(move || {
        let result = store.flush_prefix(&path);
        *slot_worker.lock().unwrap() = Some(result);
        wake_worker.signal();
    });

    Commit { wake, slot }
}

/// A branch that was never backed by a store is already as durable as it gets.
pub(crate) fn already_durable() -> Commit {
    let wake = Arc::new(Wake::default());
    let slot = Arc::new(Mutex::new(Some(Ok(()))));
    Commit { wake, slot }
}
