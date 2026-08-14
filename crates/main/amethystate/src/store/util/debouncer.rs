use crate::store::util::DeadNotifier;
use std::sync::mpsc;
use std::sync::mpsc::RecvTimeoutError;
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::Duration;
use tracing::debug;

/// Why the thread was woken: `Schedule` restarts the quiet period, `Now`
/// cuts it short for a caller that is waiting on the commit.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Trigger {
    Schedule,
    Now,
}

pub struct Debouncer {
    tx: Option<mpsc::Sender<Trigger>>,
    handle: Option<thread::JoinHandle<()>>,
    guard: Arc<Mutex<()>>,
    #[cfg(test)]
    dead: Arc<(Mutex<bool>, Condvar)>,
}

impl Debouncer {
    pub fn new<F>(interval: Duration, mut op: F) -> Self
    where
        F: FnMut() + Send + 'static,
    {
        let (tx, rx) = mpsc::channel::<Trigger>();
        let guard = Arc::new(Mutex::new(()));
        let dead = Arc::new((Mutex::new(false), Condvar::new()));
        let guard_inner = guard.clone();
        let dead_inner = dead.clone();

        let handle = thread::spawn(move || {
            // Hold the lock for the entire lifetime of the thread.
            // If op() panics, the guard is dropped and the mutex becomes
            // poisoned, which is detected in schedule() via is_poisoned().
            let _notify = DeadNotifier(dead_inner);
            let _hold = guard_inner.lock().unwrap();

            while let Ok(first) = rx.recv() {
                if first == Trigger::Now {
                    debug!("debouncer trigger: asked for immediately");
                    op();
                    continue;
                }

                loop {
                    match rx.recv_timeout(interval) {
                        Ok(Trigger::Schedule) => continue,
                        Ok(Trigger::Now) => break,
                        Err(RecvTimeoutError::Timeout) => break,
                        Err(RecvTimeoutError::Disconnected) => return,
                    }
                }

                debug!("debouncer trigger: interval elapsed");
                op();
            }

            debug!("debouncer thread exiting (channel closed)");
        });

        Self {
            tx: Some(tx),
            handle: Some(handle),
            guard,
            #[cfg(test)]
            dead,
        }
    }

    pub fn schedule(&self) {
        self.send(Trigger::Schedule);
    }

    /// Runs the operation without waiting out the quiet period.
    pub fn flush_now(&self) {
        self.send(Trigger::Now);
    }

    fn send(&self, trigger: Trigger) {
        if self.guard.is_poisoned() {
            panic!("debouncer is poisoned");
        }
        if let Some(ref tx) = self.tx
            && let Err(e) = tx.send(trigger)
        {
            panic!("failed to schedule debounced operation: channel closed ({e})");
        }
    }

    pub fn is_poisoned(&self) -> bool {
        self.guard.is_poisoned()
    }

    #[cfg(test)]
    pub fn wait_dead(&self) {
        let (lock, cvar) = &*self.dead;
        let _unused = cvar.wait_while(lock.lock().unwrap(), |dead| !*dead);
    }

    fn shutdown(&mut self) {
        self.tx.take();
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for Debouncer {
    fn drop(&mut self) {
        self.shutdown();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn test_poison_on_op_panic() {
        let call_count = Arc::new(AtomicUsize::new(0));
        let count_inner = call_count.clone();

        let d = Debouncer::new(Duration::from_millis(50), move || {
            let n = count_inner.fetch_add(1, Ordering::SeqCst);
            if n == 0 {
                panic!("simulated failure");
            }
        });

        assert!(!d.is_poisoned());

        d.schedule();
        d.wait_dead();

        assert!(d.is_poisoned());
        assert_eq!(call_count.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn test_schedule_panics_when_poisoned() {
        let d = Debouncer::new(Duration::from_millis(50), move || {
            panic!("simulated failure");
        });

        d.schedule();
        d.wait_dead();

        assert!(d.is_poisoned());

        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| d.schedule()));
        assert!(result.is_err());
    }
}
