use amethystate_core::SignalSubscription;
use std::marker::PhantomData;

type Pump = Box<dyn FnMut()>;

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
    _not_send: PhantomData<*const ()>,
}

impl LocalScope {
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

    pub fn len(&self) -> usize {
        self.subs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.subs.is_empty()
    }

    pub fn clear(&mut self) {
        self.subs.clear();
        self.pumps.clear();
    }

    pub(crate) fn add(&mut self, sub: SignalSubscription, pump: Pump) {
        self.subs.push(sub);
        self.pumps.push(pump);
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
        cell.subscribe_local(&mut scope, move |v: &u64| cap.borrow_mut().push(*v));

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
        cell.subscribe_local(&mut scope, move |v: &u64| cap.borrow_mut().push(*v));

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
        cell.subscribe_local(&mut scope, move |v: &u64| cap.borrow_mut().push(*v));

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
        cell.subscribe_local(&mut scope, move |_: &u64| *cap.borrow_mut() += 1);

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

        cell.subscribe_local(&mut scope, move |v: &u64| {
            *cap.borrow_mut() += 1;
            let _ = writer.set(v + 1);
        });

        cell.set(1).unwrap();
        scope.drain();

        assert_eq!(*calls.borrow(), 1, "its own write waits for the next drain");

        scope.drain();
        assert_eq!(*calls.borrow(), 2);
    }

    #[test]
    fn dropping_the_scope_ends_its_subscriptions() {
        let cell = ReactiveCell::new(0u64);
        let seen = Rc::new(RefCell::new(Vec::new()));

        {
            let mut scope = LocalScope::new();
            let cap = Rc::clone(&seen);
            cell.subscribe_local(&mut scope, move |v: &u64| cap.borrow_mut().push(*v));
            assert_eq!(scope.len(), 1);
        }

        cell.set(9).unwrap();
        assert!(seen.borrow().is_empty());
    }
}
