use super::cell::ReactiveCell;
use super::error::{FieldError, ReactiveFieldResult};
use crate::reactive::cell::CellCommit;
use crate::reactive::watch::{Immediate, Watch, Watchable};
use crate::store::Durable;
use crate::store::sync_backend::SyncBridge;
use crate::store::{StoreBackend, SubscriptionId};
use crate::{AccessMode, ReadOnlyMode, Store, WritableMode};
use amethystate_core::{Change, FieldCore, InterceptDisposer, SignalSubscription};
use std::sync::Arc;
use uuid::Uuid;

pub use amethystate_core::primitives::field_core::FieldValue;

pub struct StoreSubscription {
    pub store: Store,
    pub id: SubscriptionId,
}

impl Drop for StoreSubscription {
    fn drop(&mut self) {
        self.store.unsubscribe(self.id);
    }
}

/// A single persisted value with a live signal behind it.
///
/// ```
/// # use amethystate::{StoreBuilder, WritableMode};
/// # use amethystate::store::field_with_path;
/// # use std::sync::Arc;
/// # let path = amethystate_core::test_utils::TempPath::new("doc");
/// # let store = StoreBuilder::new(&*path).build().unwrap();
/// let port = field_with_path::<u16, WritableMode>(
///     &store,
///     Arc::from("net.port"),
///     8080,
///     amethystate::uuid::Uuid::new_v4(),
/// ).unwrap();
///
/// assert_eq!(port.get(), 8080);
/// port.set(9090).unwrap();
/// assert_eq!(port.get(), 9090);
/// ```
pub(crate) struct FieldInner<TValue> {
    pub(crate) core: FieldCore<TValue>,
    pub(crate) path: Arc<str>,
    pub(crate) instance_id: Uuid,
    pub(crate) store_sub: Option<Arc<StoreSubscription>>,
}

pub struct Field<TValue, M: AccessMode = ReadOnlyMode> {
    pub(crate) inner: Arc<FieldInner<TValue>>,
    pub(crate) _mode: std::marker::PhantomData<M>,
}

pub type ReadOnlyField<TValue> = Field<TValue, ReadOnlyMode>;
pub type WritableField<TValue> = Field<TValue, WritableMode>;

impl<TValue, M> std::fmt::Debug for Field<TValue, M>
where
    TValue: FieldValue + std::fmt::Debug,
    M: AccessMode,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Field")
            .field("path", &self.inner.path)
            .field("value", &self.get())
            .finish_non_exhaustive()
    }
}

/// Another handle on the same value **and the same instance id**, so writes
/// through it are indistinguishable from writes through the original.
///
/// [`Field::fork`] is the one that gives a new id, which is what a
/// subscription uses to tell whose write it is looking at - see [clone vs fork](https://uniproc-dev.github.io/amethystate/concepts/fields-and-subscriptions#clone-vs-fork).
impl<TValue, M: AccessMode> Clone for Field<TValue, M> {
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
            _mode: std::marker::PhantomData,
        }
    }
}

impl<TValue, M> Field<TValue, M>
where
    TValue: FieldValue,
    M: AccessMode,
{
    /// The same value under a new instance id.
    ///
    /// Provenance travels with the id: a subscription can tell its own writes
    /// from someone else's, and a fork is how you deliberately look like
    /// someone else. [`Clone`] does the opposite and keeps the id, so a clone
    /// stays the same actor.
    ///
    /// The difference only shows through [`Watch::external`], which drops
    /// changes the watching handle made itself:
    ///
    /// ```
    /// # use amethystate::{StoreBuilder, WritableMode};
    /// # use amethystate::store::field_with_path;
    /// # use std::sync::{Arc, Mutex};
    /// # let path = amethystate_core::test_utils::TempPath::new("doc");
    /// # let store = StoreBuilder::new(&*path).build().unwrap();
    /// let port = field_with_path::<u16, WritableMode>(
    ///     &store, Arc::from("net.port"), 8080, amethystate::uuid::Uuid::new_v4(),
    /// ).unwrap();
    ///
    /// let twin = port.clone();   // same instance id
    /// let other = port.fork();   // a new one
    ///
    /// let seen = Arc::new(Mutex::new(Vec::new()));
    /// let sink = Arc::clone(&seen);
    /// let _sub = port.subscription_with().external().register(move |v| {
    ///     sink.lock().unwrap().push(*v);
    /// });
    ///
    /// port.set(1).unwrap();    // our own write
    /// twin.set(2).unwrap();    // a clone is still us
    /// other.set(3).unwrap();   // a fork is somebody else
    ///
    /// assert_eq!(*seen.lock().unwrap(), [3], "only the fork's write got through");
    /// ```
    ///
    /// Without `external` all three would arrive; the id is what tells them
    /// apart. The book works through the pair in
    /// [clone vs fork](https://uniproc-dev.github.io/amethystate/concepts/fields-and-subscriptions#clone-vs-fork).
    pub fn fork(&self) -> Self {
        self.fork_with_id(Uuid::new_v4())
    }

    /// [`Field::fork`] with the instance id chosen rather than generated.
    pub fn fork_with_id(&self, new_instance_id: Uuid) -> Self {
        Self {
            inner: Arc::new(FieldInner {
                core: self.inner.core.clone(),
                path: Arc::clone(&self.inner.path),
                instance_id: new_instance_id,
                store_sub: self.inner.store_sub.clone(),
            }),
            _mode: std::marker::PhantomData,
        }
    }

    /// The current value, read from memory.
    ///
    /// This never touches the disk: the signal holds what was last loaded or
    /// written, and the file watcher keeps it current when something outside
    /// the process edits the store.
    ///
    /// ```
    /// # use amethystate::{StoreBuilder, WritableMode};
    /// # use amethystate::store::field_with_path;
    /// # use std::sync::Arc;
    /// # let path = amethystate_core::test_utils::TempPath::new("doc");
    /// # let store = StoreBuilder::new(&*path).build().unwrap();
    /// let port = field_with_path::<u16, WritableMode>(
    ///     &store, Arc::from("net.port"), 8080, amethystate::uuid::Uuid::new_v4(),
    /// ).unwrap();
    ///
    /// assert_eq!(port.get(), 8080);
    /// ```
    pub fn get(&self) -> TValue {
        self.inner.core.get()
    }

    /// The full path this field is stored under, prefix included.
    ///
    /// The prefix and the key are joined by [`join_path`](crate::join_path),
    /// which trims a trailing dot from one and a leading dot from the other.
    /// Which prefix and which key a declared field ends up with is the
    /// macro's business - see [`macro@crate::amethystate`].
    ///
    /// ```
    /// # use amethystate::{StoreBuilder, WritableMode};
    /// # use amethystate::store::field_with_path;
    /// # use std::sync::Arc;
    /// # let path = amethystate_core::test_utils::TempPath::new("doc");
    /// # let store = StoreBuilder::new(&*path).build().unwrap();
    /// assert_eq!(amethystate::join_path("net", "port"), "net.port");
    /// assert_eq!(amethystate::join_path("sys.db", "host"), "sys.db.host");
    /// assert_eq!(amethystate::join_path("net.", ".port"), "net.port", "dots are trimmed");
    /// assert_eq!(amethystate::join_path("", "port"), "port", "no prefix, no dot");
    ///
    /// let port = field_with_path::<u16, WritableMode>(
    ///     &store, Arc::from("net.port"), 8080, amethystate::uuid::Uuid::new_v4(),
    /// ).unwrap();
    /// assert_eq!(&*port.path(), "net.port");
    ///
    /// // The same path addresses it through `Kv`, one segment at a time.
    /// let net = store.kv().namespace("net").unwrap();
    /// assert_eq!(net.get::<u16>("port").unwrap(), Some(8080));
    /// ```
    pub fn path(&self) -> Arc<str> {
        self.inner.path.clone()
    }

    /// Calls `callback` whenever the value changes, and keeps doing so until
    /// the returned handle is dropped.
    ///
    /// A callback sees the value while it is still buffered, before it
    /// reaches disk - see [the module docs](crate::reactive) for what that
    /// costs and when it matters.
    ///
    /// # Why `Send + Sync`
    ///
    /// A change can arrive from the file watcher's thread, when something
    /// outside the process edits the store, so the callback has to be able to
    /// run there. That rules out `Rc` state and most GUI context handles.
    ///
    /// [`LocalScope`](crate::LocalScope) is the way around it, and the right one: a subscription
    /// registered there queues the value instead of calling immediately, and
    /// runs the callback from [`LocalScope::drain`](crate::LocalScope::drain) on
    /// whichever thread
    /// drains. The scope is neither `Send` nor `Sync`, so the callback cannot
    /// reach another thread at all - enforced by the type, not by convention.
    /// Reach it through [`Field::subscription_with`] and [`Watch::local`].
    ///
    /// ```
    /// # use amethystate::{StoreBuilder, WritableMode};
    /// # use amethystate::store::field_with_path;
    /// # use std::sync::Arc;
    /// # let path = amethystate_core::test_utils::TempPath::new("doc");
    /// # let store = StoreBuilder::new(&*path).build().unwrap();
    /// # let host = field_with_path::<String, WritableMode>(
    /// #     &store, Arc::from("net.host"), "localhost".to_string(),
    /// #     amethystate::uuid::Uuid::new_v4(),
    /// # ).unwrap();
    /// let (tx, rx) = std::sync::mpsc::channel();
    ///
    /// let _sub = host.subscribe(move |val| {
    ///     let _ = tx.send(val.clone());
    /// });
    ///
    /// host.set("10.0.0.1".to_string()).unwrap();
    /// assert_eq!(rx.recv().unwrap(), "10.0.0.1");
    /// ```
    ///
    /// Binding the handle to `_` rather than `_sub` would end the
    /// subscription immediately, before the write it was meant to see.
    #[track_caller]
    pub fn subscribe<F>(&self, callback: F) -> SignalSubscription
    where
        F: for<'a> Fn(&'a TValue) + Send + Sync + 'static,
    {
        self.inner.core.subscribe(callback)
    }

    /// Configures a subscription: filtering, provenance, and where the callback
    /// runs. See [`Watch`].
    pub fn subscription_with(&self) -> Watch<Self, Immediate> {
        Watch::new(self.clone())
    }
}

impl<TValue, M> Watchable for Field<TValue, M>
where
    TValue: FieldValue,
    M: AccessMode,
{
    type Item = TValue;

    fn watch_id(&self) -> Uuid {
        self.inner.instance_id
    }

    fn watch_raw<F>(&self, callback: F) -> SignalSubscription
    where
        F: Fn(&TValue, Option<Uuid>) + Send + Sync + 'static,
    {
        self.inner.core.subscribe_with_source(callback)
    }
}

impl<TValue> Field<TValue, WritableMode>
where
    TValue: FieldValue,
{
    /// A cell that keeps the field alive, for handing the value somewhere and
    /// letting go of the field.
    ///
    /// [`Field::cell`] is a view and holds the field weakly, which is the wrong
    /// default when the cell is the only handle that survives: build a struct,
    /// pass one of its fields to a widget, drop the struct, and a view would go
    /// empty. This one owns what it views, so it stays readable and writable
    /// for as long as it is held - and keeps the store open for that long too.
    ///
    /// ```
    /// # use amethystate::{StoreBuilder, WritableMode};
    /// # use amethystate::store::field_with_path;
    /// # use std::sync::Arc;
    /// # let path = amethystate_core::test_utils::TempPath::new("doc");
    /// # let store = StoreBuilder::new(&*path).build().unwrap();
    /// let width = {
    ///     let field = field_with_path::<u64, WritableMode>(
    ///         &store,
    ///         Arc::from("ui.width"),
    ///         800,
    ///         amethystate::uuid::Uuid::new_v4(),
    ///     ).unwrap();
    ///     field.into_cell()
    /// };
    ///
    /// assert_eq!(width.get(), Some(800));
    /// width.set(1024).unwrap();
    /// assert_eq!(width.get(), Some(1024));
    /// ```
    pub fn into_cell(self) -> ReactiveCell<TValue> {
        let cell = self.cell();
        cell.owning(Arc::new(self))
    }

    /// This field as a [`ReactiveCell`], with the store backend and access
    /// mode erased. Writes go through [`Field::set`], keeping this field's
    /// provenance.
    ///
    /// The cell is a view: it holds the field weakly, so it never keeps the
    /// store file open, and it reads `None` once the field is dropped. Where
    /// the cell is the only handle that survives, use [`Field::into_cell`].
    pub fn cell(&self) -> ReactiveCell<TValue> {
        let cache = amethystate_core::Signal::new(Some(self.get()));

        let sink = cache.clone();
        let read = self
            .inner
            .core
            .signal
            .subscribe_with_source(move |value, source| {
                sink.set_forwarded(Some(value.clone()), source);
            });

        let weak = Arc::downgrade(&self.inner);

        let commit = self.inner.store_sub.as_ref().map(|_| {
            let flush = weak.clone();
            let start = weak.clone();

            CellCommit {
                now: Arc::new(move || {
                    let inner = flush.upgrade().ok_or(FieldError::SourceGone)?;
                    let sub = inner.store_sub.as_ref().ok_or(FieldError::SourceGone)?;
                    Ok(sub.store.flush_prefix(&inner.path)?)
                }),
                start: Arc::new(move || match start.upgrade() {
                    Some(inner) => match inner.store_sub.as_ref() {
                        Some(sub) => sub.store.flush_async(),
                        None => crate::store::Commit::gone(),
                    },
                    None => crate::store::Commit::gone(),
                }),
            }
        });

        let alive = weak.clone();
        ReactiveCell::from_parts(
            cache,
            Arc::new(move |value| {
                let inner = weak.upgrade().ok_or(FieldError::SourceGone)?;
                let field = Field::<TValue, WritableMode> {
                    inner,
                    _mode: std::marker::PhantomData,
                };
                field.set(value)
            }),
            Some(Arc::new(move || alive.strong_count() > 0)),
            self.inner.instance_id,
            commit,
            Some(Arc::new(read)),
        )
    }

    /// Reads the value, hands it to `f` and writes back what it returns,
    /// then yields the new value.
    ///
    /// [`Field::modify`] does the same through `&mut` when the value is
    /// cheaper to adjust in place than to rebuild.
    ///
    /// ```
    /// # use amethystate::{StoreBuilder, WritableMode};
    /// # use amethystate::store::field_with_path;
    /// # use std::sync::Arc;
    /// # let path = amethystate_core::test_utils::TempPath::new("doc");
    /// # let store = StoreBuilder::new(&*path).build().unwrap();
    /// let hits = field_with_path::<u32, WritableMode>(
    ///     &store, Arc::from("stats.hits"), 0, amethystate::uuid::Uuid::new_v4(),
    /// ).unwrap();
    ///
    /// assert_eq!(hits.update(|n| n + 1).unwrap(), 1);
    /// assert_eq!(hits.get(), 1);
    /// ```
    pub fn update<F>(&self, f: F) -> ReactiveFieldResult<TValue>
    where
        F: FnOnce(TValue) -> TValue,
    {
        let val = self.get();
        let new_val = f(val);
        self.set(new_val.clone())?;
        Ok(new_val)
    }

    /// Reads the value, lets `f` change it in place, and writes it back.
    ///
    /// The same write as [`Field::update`], reached without rebuilding the
    /// value - which matters when it is a large struct or a collection.
    pub fn modify<F>(&self, f: F) -> ReactiveFieldResult<()>
    where
        F: FnOnce(&mut TValue),
    {
        let mut val = self.get();
        f(&mut val);
        self.set(val)
    }

    /// Writes the value and notifies subscribers.
    ///
    /// The write lands in the store's buffer, not on disk: it survives the
    /// process reading it back, but not a crash before the debouncer flushes.
    /// [`Field::durable`] is the variant that waits for disk.
    ///
    /// Returns [`FieldError::Intercepted`] if an interceptor rejected the
    /// change.
    pub fn set(&self, value: TValue) -> ReactiveFieldResult<()> {
        tracing::trace!(
            target: "amethystate",
            path = %self.inner.path,
            source = crate::observability::resolve_instance_short(self.inner.instance_id).unwrap_or("external"),
            "field write",
        );

        if let Some(sub) = &self.inner.store_sub {
            let backend = SyncBridge::new(sub.store.clone());
            amethystate_core::field_set(
                &backend,
                &self.inner.core,
                self.inner.path.clone(),
                value,
                Some(self.inner.instance_id),
            )?;
        } else {
            let change = self
                .inner
                .core
                .run_interceptors(self.inner.path.clone(), value, Some(self.inner.instance_id))
                .map_err(|_| FieldError::Intercepted)?;
            self.inner
                .core
                .signal
                .set_forwarded(change.new_value, change.source);
        }
        Ok(())
    }

    /// The same writes, each returning only once the value is on disk.
    ///
    /// `set` and friends leave the value in the write buffer, where a crash
    /// loses it; these pay a commit to close that window. [Durability](https://uniproc-dev.github.io/amethystate/concepts/durability) in the book
    /// covers when that window matters and what it costs to close.
    pub fn durable(&self) -> Durable<'_, Self> {
        Durable(self)
    }

    /// Installs a callback that sees every write before it lands and may
    /// rewrite or reject it.
    ///
    /// Returning `None` drops the write and gives the caller
    /// [`FieldError::Intercepted`]; returning a changed [`Change`] stores that
    /// instead of what was written.
    ///
    /// The interceptor stays installed until [`InterceptDisposer::remove`] is
    /// called; dropping the disposer only forgets the handle. A subscription
    /// goes the other way and ends on drop, because it belongs to whoever is
    /// listening. An interceptor is a rule about the value itself, so it does
    /// not end merely because nobody kept the receipt.
    ///
    /// ```
    /// # use amethystate::{StoreBuilder, WritableMode};
    /// # use amethystate::store::field_with_path;
    /// # use std::sync::Arc;
    /// # let path = amethystate_core::test_utils::TempPath::new("doc");
    /// # let store = StoreBuilder::new(&*path).build().unwrap();
    /// let volume = field_with_path::<u8, WritableMode>(
    ///     &store, Arc::from("audio.volume"), 50, amethystate::uuid::Uuid::new_v4(),
    /// ).unwrap();
    ///
    /// let guard = volume.intercept(|mut change| {
    ///     if change.new_value > 100 {
    ///         return None;                            // refuse it outright
    ///     }
    ///     change.new_value -= change.new_value % 10;  // or round it down
    ///     Some(change)
    /// });
    ///
    /// volume.set(77).unwrap();
    /// assert_eq!(volume.get(), 70, "the interceptor rewrote the write");
    ///
    /// assert!(volume.set(200).is_err(), "and refused this one");
    /// assert_eq!(volume.get(), 70, "so the old value stands");
    ///
    /// guard.remove();
    /// volume.set(200).unwrap();
    /// assert_eq!(volume.get(), 200, "with it removed, nothing filters");
    /// ```
    ///
    /// Interceptors run before the value reaches the buffer at all - see
    /// [the module docs](crate::reactive) for where that sits relative to disk.
    pub fn intercept<F>(&self, callback: F) -> InterceptDisposer
    where
        F: Fn(Change<TValue>) -> Option<Change<TValue>> + Send + Sync + 'static,
    {
        self.inner.core.intercept(self.inner.path.clone(), callback)
    }

    /// A field with no store behind it: reactive, but never read from or
    /// written to disk.
    ///
    /// This is what `#[amestate(volatile)]` produces. Subscriptions and
    /// interceptors work as usual; the value simply starts at `default` on
    /// every run.
    ///
    /// The path is still carried, for tracing and for interceptors - but
    /// nothing is ever stored under it:
    ///
    /// ```
    /// # use amethystate::{Field, StoreBuilder, WritableMode};
    /// # use std::sync::Arc;
    /// # let path = amethystate_core::test_utils::TempPath::new("doc");
    /// # let store = StoreBuilder::new(&*path).build().unwrap();
    /// let session: Field<String, WritableMode> =
    ///     Field::new_volatile(Arc::from("app.session"), "anonymous".to_string());
    ///
    /// session.set("alice".to_string()).unwrap();
    /// assert_eq!(session.get(), "alice");
    ///
    /// // The store never heard about any of it.
    /// let app = store.kv().namespace("app").unwrap();
    /// assert_eq!(app.get::<String>("session").unwrap(), None);
    /// ```
    pub fn new_volatile(path: Arc<str>, default: TValue) -> Self {
        Self::new_volatile_with_id(path, default, Uuid::new_v4())
    }

    /// [`Field::new_volatile`] with the instance id chosen rather than
    /// generated.
    pub fn new_volatile_with_id(path: Arc<str>, default: TValue, instance_id: Uuid) -> Self {
        Self {
            inner: Arc::new(FieldInner {
                core: FieldCore::new(default),
                path,
                instance_id,
                store_sub: None,
            }),
            _mode: std::marker::PhantomData,
        }
    }
}

impl<TValue, M: AccessMode> PartialEq for Field<TValue, M> {
    fn eq(&self, other: &Self) -> bool {
        self.inner.path == other.inner.path
            && self.inner.instance_id == other.inner.instance_id
            && Arc::ptr_eq(
                &self.inner.core.signal.value,
                &other.inner.core.signal.value,
            )
    }
}

impl<TValue, M: AccessMode> Eq for Field<TValue, M> {}

impl<TValue, M> amethystate_core::pipeline::Reactive<TValue> for Field<TValue, M>
where
    TValue: FieldValue,
    M: AccessMode,
{
    fn get(&self) -> TValue {
        self.get()
    }

    fn subscribe_with_source<F>(&self, callback: F) -> SignalSubscription
    where
        F: for<'a> Fn(&'a TValue, Option<Uuid>) + Send + Sync + 'static,
    {
        self.inner.core.subscribe_with_source(callback)
    }

    fn subscribe<F>(&self, callback: F) -> SignalSubscription
    where
        F: for<'a> Fn(&'a TValue) + Send + Sync + 'static,
    {
        self.subscribe(callback)
    }
}

impl<TValue> Durable<'_, Field<TValue, WritableMode>>
where
    TValue: FieldValue,
{
    /// [`Field::set`], returning only once the value is on disk.
    ///
    /// The write itself is identical - same subscribers, same interceptors,
    /// same value. What differs is when the call returns: the plain one comes
    /// back with the change still in the buffer, this one waits for the
    /// commit.
    ///
    /// That only matters for an abrupt end. A graceful shutdown flushes the
    /// buffer, because the store commits when it is dropped, so both writes
    /// survive it; a crash in between loses the plain one and keeps this one.
    ///
    /// # What the three engine families commit
    ///
    /// The guarantee you can rely on everywhere is narrow: **this write** is
    /// on disk when the call returns. How much else goes with it differs.
    ///
    /// - **redb** and **sqlite** are transactional. The flush collects the
    ///   buffered writes under this field's prefix and commits those, leaving
    ///   the rest of the buffer where it was. Granularity is per prefix.
    /// - **json**, **toml** and **ron** share one document. There is nothing
    ///   partial to write, so the flush serialises the whole file - and every
    ///   buffered write in the store becomes durable as a side effect.
    ///
    /// So a text backend is accidentally stronger here. Do not build on that:
    /// the same code on redb commits only what sits under this prefix.
    ///
    /// # Seeing it happen
    ///
    /// From inside the process this needs a backend that tolerates a second
    /// reader while the store still holds the file. The text ones and sqlite
    /// do; redb keeps its lock for the life of the process and refuses, so
    /// suppressing the store's `Drop` is not enough to observe it either.
    ///
    /// `tests/set_durable.rs` reads the file back mid-run over the text
    /// backend, and `tests/durability_crash.rs` covers redb the only way it
    /// allows - a child process that writes and then aborts, skipping every
    /// destructor, after which the parent reopens the database and finds the
    /// durable write present and the plain one gone.
    ///
    /// ```
    /// # use amethystate::{StoreBuilder, WritableMode};
    /// # use amethystate::store::field_with_path;
    /// # use std::sync::Arc;
    /// # let path = amethystate_core::test_utils::TempPath::new("doc");
    /// // A debouncer a minute out: nothing reaches disk on its own.
    /// # let store = StoreBuilder::new(&*path).debounce(60_000).build().unwrap();
    /// let port = field_with_path::<u16, WritableMode>(
    ///     &store, Arc::from("net.port"), 8080, amethystate::uuid::Uuid::new_v4(),
    /// ).unwrap();
    ///
    /// port.durable().set(9090).unwrap();
    /// assert_eq!(port.get(), 9090);
    /// ```
    pub fn set(&self, value: TValue) -> ReactiveFieldResult<()> {
        self.0.set(value)?;
        self.commit()
    }

    /// [`Field::set`], resolving once the value is on disk.
    ///
    /// Waiters ride on the commit the store was going to make anyway, so
    /// several of them cost one commit rather than one each.
    ///
    /// # Nothing happens until it is awaited
    ///
    /// This is an `async fn`, so its whole body is lazy - the write included.
    /// The synchronous [`Durable::set`] stores the value and then waits for
    /// the commit; dropping this future without awaiting it does not write at
    /// all, and the field keeps its old value.
    ///
    /// ```
    /// # use amethystate::{StoreBuilder, WritableMode};
    /// # use amethystate::store::field_with_path;
    /// # use std::sync::Arc;
    /// # let path = amethystate_core::test_utils::TempPath::new("doc");
    /// # let store = StoreBuilder::new(&*path).build().unwrap();
    /// let port = field_with_path::<u16, WritableMode>(
    ///     &store, Arc::from("net.port"), 8080, amethystate::uuid::Uuid::new_v4(),
    /// ).unwrap();
    ///
    /// let durable = port.durable();
    /// let pending = durable.set_async(9090);
    /// assert_eq!(port.get(), 8080, "the future has not been polled yet");
    ///
    /// futures::executor::block_on(pending).unwrap();
    /// assert_eq!(port.get(), 9090);
    /// ```
    pub async fn set_async(&self, value: TValue) -> ReactiveFieldResult<()> {
        self.0.set(value)?;
        self.commit_async().await
    }

    /// Reads the value, writes back what `f` returns, and yields it.
    ///
    /// Returns only once the change is on disk rather than buffered.
    pub fn update<F>(&self, f: F) -> ReactiveFieldResult<TValue>
    where
        F: FnOnce(TValue) -> TValue,
    {
        let value = self.0.update(f)?;
        self.commit()?;
        Ok(value)
    }

    /// Reads the value, writes back what `f` returns, and yields it.
    ///
    /// Resolves once the change is on disk rather than buffered.
    /// Like every future, this does nothing until awaited - the write
    /// included. See [`Durable::set_async`].
    pub async fn update_async<F>(&self, f: F) -> ReactiveFieldResult<TValue>
    where
        F: FnOnce(TValue) -> TValue,
    {
        let value = self.0.update(f)?;
        self.commit_async().await?;
        Ok(value)
    }

    /// Reads the value, lets `f` change it in place, and writes it back.
    ///
    /// Returns only once the change is on disk rather than buffered.
    pub fn modify<F>(&self, f: F) -> ReactiveFieldResult<()>
    where
        F: FnOnce(&mut TValue),
    {
        self.0.modify(f)?;
        self.commit()
    }

    /// Reads the value, lets `f` change it in place, and writes it back.
    ///
    /// Resolves once the change is on disk rather than buffered.
    /// Like every future, this does nothing until awaited - the write
    /// included. See [`Durable::set_async`].
    pub async fn modify_async<F>(&self, f: F) -> ReactiveFieldResult<()>
    where
        F: FnOnce(&mut TValue),
    {
        self.0.modify(f)?;
        self.commit_async().await
    }

    fn commit(&self) -> ReactiveFieldResult<()> {
        if let Some(sub) = &self.0.inner.store_sub {
            sub.store.flush_prefix(&self.0.inner.path)?;
        }
        Ok(())
    }

    async fn commit_async(&self) -> ReactiveFieldResult<()> {
        if let Some(sub) = &self.0.inner.store_sub {
            sub.store.flush_async().await?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SubscriptionKind;
    use crate::store::{StateScope, StoreBackend};
    use crate::test_utils::unique_store;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tracing_test::traced_test;

    struct UiScope;
    impl StateScope for UiScope {
        const PREFIX: &'static str = "ui";
    }

    #[test]
    fn field_get_set_and_subscribe() {
        let store = unique_store("field-int");
        let field = crate::store::field::<UiScope, i32>(&store, "font_size", 14, Uuid::new_v4())
            .expect("field should be created");

        assert_eq!(field.get(), 14);
        assert_eq!(field.path().as_ref(), "ui.font_size");

        field.set(18).expect("set should succeed");
        assert_eq!(store.get::<i32>("ui.font_size").unwrap(), Some(18));

        let callback_val = Arc::new(Mutex::new(0i32));
        let cap = callback_val.clone();
        let _sub = field.subscribe(move |v| {
            *cap.lock().unwrap() = *v;
        });

        field.inner.core.signal.set(22);
        assert_eq!(*callback_val.lock().unwrap(), 22);
    }

    #[test]
    fn store_subscription_drop_unsubscribes() {
        let store = unique_store("drop-unsub");
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let core = FieldCore::new("test_val".to_string());

        let cap = calls.clone();

        {
            let sub_id = store.subscribe(
                SubscriptionKind::Prefix(Arc::from("test.field")),
                Arc::new(move |_| {
                    cap.fetch_add(1, Ordering::SeqCst);
                }),
            );

            let field: Field<String, WritableMode> = Field {
                inner: Arc::new(FieldInner {
                    core,
                    path: Arc::from("test.field"),
                    store_sub: Some(Arc::new(StoreSubscription {
                        store: store.clone(),
                        id: sub_id,
                    })),
                    instance_id: Default::default(),
                }),
                _mode: Default::default(),
            };

            field.set("hello".to_string()).unwrap();
            assert_eq!(calls.load(Ordering::SeqCst), 1);
            store.set("test.field", &"world").unwrap();
            assert_eq!(calls.load(Ordering::SeqCst), 2);
        }

        store.set("test.field", &"world").unwrap();
        assert_eq!(
            calls.load(Ordering::SeqCst),
            2,
            "callback must not fire after drop"
        );
    }

    /// A cell hands out the field's own cache rather than a copy of it, so a
    /// write landing in the field is visible through the cell.
    #[test]
    fn field_cell_shares_the_field_cache() {
        let store = unique_store("cell-shares-cache");
        let field = crate::store::field::<UiScope, i32>(&store, "shared", 1, Uuid::new_v4())
            .expect("field should be created");

        let cell = field.cell();
        field.set(7).unwrap();

        assert_eq!(cell.get(), Some(7));
    }

    #[test]
    fn test_volatile_field_behavior() {
        let store = unique_store("test_volatile_field_behavior");

        let field_path: Arc<str> = Arc::from("ui.temp_spinner");

        let field = Field::<bool, WritableMode>::new_volatile(field_path.clone(), false);

        let call_count = Arc::new(Mutex::new(0));
        let last_val = Arc::new(Mutex::new(false));

        let c_count = call_count.clone();
        let l_val = last_val.clone();

        let _sub = field.subscribe(move |val| {
            *c_count.lock().unwrap() += 1;
            *l_val.lock().unwrap() = *val;
        });

        field.set(true).expect("Volatile set should work");

        assert!(field.get());

        assert!(*call_count.lock().unwrap() >= 1);
        assert!(*last_val.lock().unwrap());

        let in_store: Option<bool> = store.get(&field_path).unwrap();
        assert!(
            in_store.is_none(),
            "Volatile data must NOT be persisted to store"
        );
    }

    #[test]
    fn test_field_additional_coverage() {
        let field = Field::<i32, WritableMode>::new_volatile(Arc::from("test"), 42);

        let disp = field.intercept(|mut change| {
            change.new_value *= 2;
            Some(change)
        });

        field.set(10).unwrap();
        assert_eq!(field.get(), 20);

        drop(disp);

        field.set(10).unwrap();
        assert_eq!(field.get(), 20, "Interceptor should survive manual drop");

        let disp2 = field.intercept(|mut change| {
            change.new_value += 1;
            Some(change)
        });

        field.set(5).unwrap();
        assert_eq!(field.get(), 11);

        disp2.remove();

        field.set(5).unwrap();
        assert_eq!(field.get(), 10);
    }

    #[test]
    fn test_field_depth_guard() {
        let field = Field::<i32, WritableMode>::new_volatile(Arc::from("test"), 1);

        field
            .inner
            .core
            .intercept_depth
            .store(100, Ordering::SeqCst);

        let _disp = field.intercept(|mut c| {
            c.new_value = 999;
            Some(c)
        });

        let result = field.set(10);

        assert!(
            result.is_err(),
            "past the depth limit the interceptor cannot run, so the write is \
             refused rather than let through unchecked"
        );
        assert_eq!(field.get(), 1, "and nothing is written");
    }

    #[test]
    #[traced_test]
    fn test_field_recursion_warning() {
        let field = Field::<i32, WritableMode>::new_volatile(Arc::from("test.recursive_field"), 0);

        let field_clone = field.clone();

        field.intercept(move |change| {
            let _ = field_clone.set(change.new_value + 1);
            Some(change)
        });

        let _ = field.set(1);

        assert!(logs_contain("maximum intercept depth reached"));
        assert!(logs_contain("path=test.recursive_field"));
    }

    #[test]
    fn test_field_subscribe_external() {
        let field = Field::<i32, WritableMode>::new_volatile(Arc::from("test.ext"), 0);
        let fork = field.fork();

        let calls = Arc::new(AtomicUsize::new(0));
        let c_clone = calls.clone();

        let _sub = field.subscription_with().external().register(move |_| {
            c_clone.fetch_add(1, Ordering::SeqCst);
        });

        field.set(1).unwrap();
        assert_eq!(
            calls.load(Ordering::SeqCst),
            0,
            "Own updates should be ignored"
        );

        fork.set(2).unwrap();
        assert_eq!(
            calls.load(Ordering::SeqCst),
            1,
            "Updates from fork should trigger"
        );

        field.inner.core.signal.set(3);
        assert_eq!(
            calls.load(Ordering::SeqCst),
            2,
            "Updates without source should trigger"
        );
    }

    #[test]
    fn test_field_subscribe_external_persistent() {
        let store = unique_store("field_external_persistent");

        let field =
            crate::store::field::<UiScope, i32>(&store, "persistent_val", 100, Uuid::new_v4())
                .expect("field should be created");

        let fork = field.fork();

        let calls = Arc::new(AtomicUsize::new(0));
        let c_clone = calls.clone();

        let _sub = field.subscription_with().external().register(move |_| {
            c_clone.fetch_add(1, Ordering::SeqCst);
        });

        field.set(200).unwrap();

        assert_eq!(
            calls.load(Ordering::SeqCst),
            0,
            "Own writes must be ignored, but without last_write_source they trigger subscribe_external!"
        );

        fork.set(300).unwrap();

        assert_eq!(
            calls.load(Ordering::SeqCst),
            1,
            "Fork updates should trigger"
        );
    }
    #[test]
    fn test_field_update_and_modify() {
        let field = Field::<i32, WritableMode>::new_volatile(Arc::from("test.update_modify"), 10);

        let updated = field.update(|val| val + 5).unwrap();
        assert_eq!(updated, 15);

        field.modify(|val| *val += 10).unwrap();
        assert_eq!(field.get(), 25);
    }
}
