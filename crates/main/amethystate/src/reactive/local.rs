use amethystate_core::SignalSubscription;
use std::future::Future;
use std::marker::PhantomData;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Waker};

type Pump = Box<dyn FnMut()>;

/// Crosses the thread boundary: the writer flags it, the draining thread waits
/// on it. Carries no value - the queues do that.
#[derive(Default)]
pub(crate) struct Wake {
    ready: AtomicBool,
    waker: Mutex<Option<Waker>>,
}

impl Wake {
    pub(crate) fn signal(&self) {
        self.ready.store(true, Ordering::Release);
        if let Some(waker) = self.waker.lock().unwrap().take() {
            waker.wake();
        }
    }

    fn take(&self) -> bool {
        self.ready.swap(false, Ordering::AcqRel)
    }

    /// Registers `cx`'s waker. Callers must re-check their queue afterwards: a
    /// signal landing between their check and this one would otherwise be
    /// missed, with nothing left to wake them.
    pub(crate) fn park(&self, cx: &Context<'_>) {
        *self.waker.lock().unwrap() = Some(cx.waker().clone());
    }
}

/// Resolves once there is something to [`LocalScope::drain`].
pub struct Changed<'a> {
    wake: &'a Arc<Wake>,
}

impl Future for Changed<'_> {
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        if self.wake.take() {
            return Poll::Ready(());
        }

        *self.wake.waker.lock().unwrap() = Some(cx.waker().clone());

        if self.wake.take() {
            return Poll::Ready(());
        }

        Poll::Pending
    }
}

/// Holds subscriptions whose callbacks run on this thread, when you pump.
///
/// Ordinary subscriptions hand their callback to the library, which may call it
/// from the file watcher's thread - so the callback has to be `Send + Sync`,
/// which rules out `Rc` state and most GUI context handles. A local
/// subscription instead queues the value and calls the callback from
/// [`LocalScope::drain`], on whichever thread drains.
///
/// The scope is neither `Send` nor `Sync`, so a callback registered here cannot
/// reach another thread at all - not by convention, but because the type will
/// not move.
///
/// Dropping the scope ends every subscription in it.
#[derive(Default)]
pub struct LocalScope {
    subs: Vec<SignalSubscription>,
    pumps: Vec<Pump>,
    wake: Arc<Wake>,
    _not_send: PhantomData<*const ()>,
}

impl LocalScope {
    /// An empty scope. Subscriptions join it through [`Watch::local`](crate::reactive::watch::Watch::local).
    pub fn new() -> Self {
        Self::default()
    }

    /// Delivers everything queued since the last call.
    ///
    /// Changes a callback causes while draining are picked up by the next
    /// drain, not this one, so a callback that writes what it listens to cannot
    /// spin here.
    pub fn drain(&mut self) {
        for pump in &mut self.pumps {
            pump();
        }
    }

    /// How many values are queued, waiting for the next
    /// [`LocalScope::drain`].
    pub fn len(&self) -> usize {
        self.subs.len()
    }

    /// Whether nothing is queued.
    pub fn is_empty(&self) -> bool {
        self.subs.is_empty()
    }

    /// Drops the queued values without delivering them, leaving the
    /// subscriptions in place.
    pub fn clear(&mut self) {
        self.subs.clear();
        self.pumps.clear();
    }

    /// Waits until something is queued.
    ///
    /// Delivery still happens in [`LocalScope::drain`] - this only wakes the
    /// loop that calls it:
    ///
    /// ```rust,ignore
    /// loop {
    ///     ui.changed().await;
    ///     ui.drain();
    /// }
    /// ```
    pub fn changed(&self) -> Changed<'_> {
        Changed { wake: &self.wake }
    }

    pub(crate) fn add(&mut self, sub: SignalSubscription, pump: Pump) {
        self.subs.push(sub);
        self.pumps.push(pump);
    }

    pub(crate) fn wake_handle(&self) -> Arc<Wake> {
        Arc::clone(&self.wake)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ReactiveCell;
    use std::cell::RefCell;
    use std::rc::Rc;

    #[test]
    fn local_callback_need_not_be_send() {
        let cell = ReactiveCell::new(1u64);
        let mut scope = LocalScope::new();

        let seen = Rc::new(RefCell::new(Vec::new()));
        let cap = Rc::clone(&seen);
        cell.subscription_with()
            .local(&mut scope)
            .register(move |v: &Option<u64>| cap.borrow_mut().push(v.unwrap()));

        cell.set(2).unwrap();
        scope.drain();

        assert_eq!(*seen.borrow(), vec![2]);
    }

    #[test]
    fn nothing_runs_until_drained() {
        let cell = ReactiveCell::new(1u64);
        let mut scope = LocalScope::new();

        let seen = Rc::new(RefCell::new(Vec::new()));
        let cap = Rc::clone(&seen);
        cell.subscription_with()
            .local(&mut scope)
            .register(move |v: &Option<u64>| cap.borrow_mut().push(v.unwrap()));

        cell.set(2).unwrap();
        assert!(seen.borrow().is_empty());

        scope.drain();
        assert_eq!(*seen.borrow(), vec![2]);
    }

    #[test]
    fn changes_coalesce_to_the_newest() {
        let cell = ReactiveCell::new(0u64);
        let mut scope = LocalScope::new();

        let seen = Rc::new(RefCell::new(Vec::new()));
        let cap = Rc::clone(&seen);
        cell.subscription_with()
            .local(&mut scope)
            .register(move |v: &Option<u64>| cap.borrow_mut().push(v.unwrap()));

        for n in 1..=5 {
            cell.set(n).unwrap();
        }
        scope.drain();

        assert_eq!(*seen.borrow(), vec![5], "a state, not a stream of events");
    }

    #[test]
    fn drain_without_changes_calls_nothing() {
        let cell = ReactiveCell::new(0u64);
        let mut scope = LocalScope::new();

        let calls = Rc::new(RefCell::new(0usize));
        let cap = Rc::clone(&calls);
        cell.subscription_with()
            .local(&mut scope)
            .register(move |_: &Option<u64>| *cap.borrow_mut() += 1);

        scope.drain();
        scope.drain();

        assert_eq!(*calls.borrow(), 0);
    }

    #[test]
    fn a_callback_that_writes_cannot_spin() {
        let cell = ReactiveCell::new(0u64);
        let mut scope = LocalScope::new();

        let writer = cell.clone();
        let calls = Rc::new(RefCell::new(0usize));
        let cap = Rc::clone(&calls);

        cell.subscription_with()
            .local(&mut scope)
            .register(move |v: &Option<u64>| {
                *cap.borrow_mut() += 1;
                let _ = writer.set(v.unwrap_or(0) + 1);
            });

        cell.set(1).unwrap();
        scope.drain();

        assert_eq!(*calls.borrow(), 1, "its own write waits for the next drain");

        scope.drain();
        assert_eq!(*calls.borrow(), 2);
    }

    fn block_on<F: Future>(mut future: F) -> F::Output {
        struct ParkWaker(std::thread::Thread);

        impl std::task::Wake for ParkWaker {
            fn wake(self: Arc<Self>) {
                self.0.unpark();
            }
        }

        let waker = Waker::from(Arc::new(ParkWaker(std::thread::current())));
        let mut cx = Context::from_waker(&waker);
        let mut future = unsafe { Pin::new_unchecked(&mut future) };

        loop {
            if let Poll::Ready(out) = future.as_mut().poll(&mut cx) {
                return out;
            }
            std::thread::park();
        }
    }

    #[test]
    fn changed_resolves_once_something_is_queued() {
        let cell = ReactiveCell::new(0u64);
        let mut scope = LocalScope::new();

        let seen = Rc::new(RefCell::new(Vec::new()));
        let cap = Rc::clone(&seen);
        cell.subscription_with()
            .local(&mut scope)
            .register(move |v: &Option<u64>| cap.borrow_mut().push(v.unwrap()));

        cell.set(7).unwrap();

        block_on(scope.changed());
        scope.drain();

        assert_eq!(*seen.borrow(), vec![7]);
    }

    #[test]
    fn changed_is_pending_while_nothing_happened() {
        let cell = ReactiveCell::new(0u64);
        let mut scope = LocalScope::new();
        cell.subscription_with()
            .local(&mut scope)
            .register(|_: &Option<u64>| {});

        let mut fut = scope.changed();
        let waker = Waker::noop();
        let mut cx = Context::from_waker(waker);

        assert!(
            unsafe { Pin::new_unchecked(&mut fut) }
                .poll(&mut cx)
                .is_pending()
        );
    }

    #[test]
    fn a_write_from_another_thread_wakes_the_drainer() {
        let cell = ReactiveCell::new(0u64);
        let mut scope = LocalScope::new();

        let seen = Rc::new(RefCell::new(Vec::new()));
        let cap = Rc::clone(&seen);
        cell.subscription_with()
            .local(&mut scope)
            .register(move |v: &Option<u64>| cap.borrow_mut().push(v.unwrap()));

        let writer = cell.clone();
        let handle = std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(20));
            writer.set(42).unwrap();
        });

        block_on(scope.changed());
        scope.drain();
        handle.join().unwrap();

        assert_eq!(*seen.borrow(), vec![42]);
    }

    #[test]
    fn dropping_the_scope_ends_its_subscriptions() {
        let cell = ReactiveCell::new(0u64);
        let seen = Rc::new(RefCell::new(Vec::new()));

        {
            let mut scope = LocalScope::new();
            let cap = Rc::clone(&seen);
            cell.subscription_with()
                .local(&mut scope)
                .register(move |v: &Option<u64>| cap.borrow_mut().push(v.unwrap()));
            assert_eq!(scope.len(), 1);
        }

        cell.set(9).unwrap();
        assert!(seen.borrow().is_empty());
    }
}
