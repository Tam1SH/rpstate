use crate::reactive::local::LocalScope;
use amethystate_core::SignalSubscription;
use std::sync::{Arc, Mutex};
use uuid::Uuid;

/// Something a [`Watch`] can be built over.
pub trait Watchable {
    /// What a subscriber is handed.
    type Item: Clone + Send + 'static;

    /// Identity of the handle, so `external` can tell its own writes apart.
    fn watch_id(&self) -> Uuid;

    /// Whether [`Watch::external`] may drop this item when it came from this
    /// handle.
    ///
    /// Maps say no for anything but `Update`: rewriting a value is the business
    /// of whoever wrote it, but a key appearing or disappearing changes what the
    /// map holds and goes to everyone, including the handle that caused it.
    fn filterable(_item: &Self::Item) -> bool {
        true
    }

    fn watch_raw<F>(&self, callback: F) -> SignalSubscription
    where
        F: Fn(&Self::Item, Option<Uuid>) + Send + Sync + 'static;
}

/// Delivery: the callback runs wherever the change is emitted.
pub struct Immediate;

/// Delivery: the change is queued and the callback runs on
/// [`LocalScope::drain`].
pub struct Local<'a> {
    scope: &'a mut LocalScope,
    coalesce: bool,
}

/// A subscription being configured.
///
/// Built by `on_change` on the primitive, finished by [`Watch::register`] or
/// [`Watch::register_with_source`]. Without a terminal call nothing is
/// subscribed.
#[must_use = "a Watch subscribes to nothing until register() is called"]
pub struct Watch<W, D> {
    source: W,
    external: bool,
    delivery: D,
}

impl<W: Watchable> Watch<W, Immediate> {
    pub(crate) fn new(source: W) -> Self {
        Self {
            source,
            external: false,
            delivery: Immediate,
        }
    }

    /// Queues changes instead of calling straight away, so the callback runs on
    /// the thread that drains `scope` and need not be `Send + Sync`.
    ///
    /// Changes coalesce by default: however many arrived since the last drain,
    /// the callback sees the newest once. Call [`Watch::every`] to keep them
    /// all, which is what you want when the item is an event rather than a
    /// state.
    pub fn local<'a>(self, scope: &'a mut LocalScope) -> Watch<W, Local<'a>> {
        Watch {
            source: self.source,
            external: self.external,
            delivery: Local {
                scope,
                coalesce: true,
            },
        }
    }

    pub fn register<F>(self, callback: F) -> SignalSubscription
    where
        F: Fn(&W::Item) + Send + Sync + 'static,
    {
        self.register_with_source(move |item, _| callback(item))
    }

    pub fn register_with_source<F>(self, callback: F) -> SignalSubscription
    where
        F: Fn(&W::Item, Option<Uuid>) + Send + Sync + 'static,
    {
        let mine = self.external.then(|| self.source.watch_id());

        self.source.watch_raw(move |item, source| {
            if mine.is_some() && source == mine && W::filterable(item) {
                return;
            }
            callback(item, source);
        })
    }
}

impl<W: Watchable> Watch<W, Local<'_>> {
    /// Keeps every change instead of coalescing to the newest.
    pub fn every(mut self) -> Self {
        self.delivery.coalesce = false;
        self
    }

    pub fn register<F>(self, mut callback: F)
    where
        F: FnMut(&W::Item) + 'static,
    {
        self.register_with_source(move |item, _| callback(item))
    }

    pub fn register_with_source<F>(self, mut callback: F)
    where
        F: FnMut(&W::Item, Option<Uuid>) + 'static,
    {
        let mine = self.external.then(|| self.source.watch_id());
        let coalesce = self.delivery.coalesce;

        type Queued<I> = Vec<(I, Option<Uuid>)>;
        let queue: Arc<Mutex<Queued<W::Item>>> = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&queue);

        let sub = self.source.watch_raw(move |item, source| {
            if mine.is_some() && source == mine && W::filterable(item) {
                return;
            }
            let mut queued = sink.lock().unwrap();
            if coalesce {
                queued.clear();
            }
            queued.push((item.clone(), source));
        });

        self.delivery.scope.add(
            sub,
            Box::new(move || {
                let batch = std::mem::take(&mut *queue.lock().unwrap());
                for (item, source) in batch {
                    callback(&item, source);
                }
            }),
        );
    }
}

impl<W, D> Watch<W, D> {
    /// Skips changes this handle made itself.
    pub fn external(mut self) -> Self {
        self.external = true;
        self
    }
}

impl<K, V, S, M, D> Watch<crate::ReactiveMap<K, V, S, M>, D>
where
    K: crate::ReactiveMapKey,
    V: crate::ReactiveMapValue,
    S: crate::store::Store,
    M: crate::AccessMode,
{
    /// Narrows to one key instead of every change in the map.
    pub fn key(self, key: K) -> Watch<crate::reactive::map::KeyOf<K, V, S, M>, D> {
        Watch {
            source: crate::reactive::map::KeyOf::new(self.source, key),
            external: self.external,
            delivery: self.delivery,
        }
    }
}
