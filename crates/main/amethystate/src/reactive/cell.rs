use crate::reactive::error::WriteResult;
use crate::reactive::watch::{Immediate, Watch, Watchable};
use crate::store::Durable;
use amethystate_core::{Signal, SignalSubscription};
use std::sync::Arc;
use uuid::Uuid;

type Writer<T> = Arc<dyn Fn(T) -> WriteResult<()> + Send + Sync>;

/// How a cell commits, for the primitives that have somewhere to commit to.
/// A cell holding a value in memory has none, and needs none.
#[derive(Clone)]
pub(crate) struct CellCommit {
    pub(crate) now: Arc<dyn Fn() -> WriteResult<()> + Send + Sync>,
    pub(crate) start: Arc<dyn Fn() -> crate::store::Commit + Send + Sync>,
}

/// A reactive value you can read, write and subscribe to, with the primitive
/// behind it erased - along with its type parameters, so fields, map entries
/// and in-memory values share one type and one collection.
///
/// Writes always reach where the value lives.
pub struct ReactiveCell<T> {
    cache: Signal<T>,
    writer: Writer<T>,
    origin: Uuid,
    commit: Option<CellCommit>,
    _keepalive: Option<Arc<dyn Send + Sync>>,
}

impl<T> Clone for ReactiveCell<T> {
    fn clone(&self) -> Self {
        Self {
            cache: self.cache.clone(),
            writer: Arc::clone(&self.writer),
            origin: self.origin,
            commit: self.commit.clone(),
            _keepalive: self._keepalive.clone(),
        }
    }
}

impl<T> ReactiveCell<T>
where
    T: Clone + Send + Sync + 'static,
{
    /// `keepalive` must hold the subscription feeding `cache` unless something
    /// captured by `writer` already does.
    pub(crate) fn from_parts(
        cache: Signal<T>,
        writer: Writer<T>,
        origin: Uuid,
        commit: Option<CellCommit>,
        keepalive: Option<Arc<dyn Send + Sync>>,
    ) -> Self {
        Self {
            cache,
            writer,
            origin,
            commit,
            _keepalive: keepalive,
        }
    }

    /// An in-memory cell, for reactive values that are not persisted.
    pub fn new(initial: T) -> Self {
        let cache = Signal::new(initial);
        let sink = cache.clone();

        Self {
            cache,
            writer: Arc::new(move |value| {
                sink.set(value);
                Ok(())
            }),
            origin: Uuid::new_v4(),
            commit: None,
            _keepalive: None,
        }
    }

    /// The current value, read from the cache rather than the store.
    ///
    /// For a cell over a persisted value the cache is kept current by the
    /// store subscription, so this sees writes made anywhere - including from
    /// another process editing the file.
    ///
    /// ```
    /// # use amethystate::ReactiveCell;
    /// let mode = ReactiveCell::new("dark".to_string());
    ///
    /// assert_eq!(mode.get(), "dark");
    /// mode.set("light".to_string()).unwrap();
    /// assert_eq!(mode.get(), "light");
    /// ```
    pub fn get(&self) -> T {
        self.cache.get()
    }

    /// The same writes, each returning only once the value is on disk.
    ///
    /// A cell over a value held in memory has nothing to commit to, and its
    /// writes here behave as the plain ones do.
    pub fn durable(&self) -> Durable<'_, Self> {
        Durable(self)
    }

    /// Fails if the write is refused - by an interceptor, or by the store.
    /// The cache is left to the store subscription, so a refused write never
    /// shows up in `get()`.
    pub fn set(&self, value: T) -> WriteResult<()> {
        (self.writer)(value)
    }

    /// Read-modify-write, not atomic.
    /// Reads the value, writes back what `f` returns, and yields the new
    /// value.
    ///
    /// ```
    /// # use amethystate::ReactiveCell;
    /// let hits = ReactiveCell::new(0u32);
    ///
    /// assert_eq!(hits.update(|n| n + 1).unwrap(), 1);
    /// assert_eq!(hits.get(), 1);
    /// ```
    pub fn update<F>(&self, f: F) -> WriteResult<T>
    where
        F: FnOnce(T) -> T,
    {
        let next = f(self.get());
        self.set(next.clone())?;
        Ok(next)
    }

    /// Same caveat as [`ReactiveCell::update`]: read-modify-write, not atomic.
    /// Reads the value, lets `f` change it in place, and writes it back.
    pub fn modify<F>(&self, f: F) -> WriteResult<()>
    where
        F: FnOnce(&mut T),
    {
        let mut value = self.get();
        f(&mut value);
        self.set(value)
    }

    ///
    /// A callback sees the change while it is still buffered, before it
    /// reaches disk - see [the module docs](crate::reactive).
    #[track_caller]
    pub fn subscribe<F>(&self, callback: F) -> SignalSubscription
    where
        F: Fn(&T) + Send + Sync + 'static,
    {
        self.cache.subscribe(callback)
    }

    /// Subscribes with the provenance of each change, so a subscriber can tell
    /// its own writes from everyone else's.
    ///
    /// A callback sees the change while it is still buffered, before it
    /// reaches disk - see [the module docs](crate::reactive).
    #[track_caller]
    pub fn subscribe_with_source<F>(&self, callback: F) -> SignalSubscription
    where
        F: Fn(&T, Option<Uuid>) + Send + Sync + 'static,
    {
        self.cache.subscribe_with_source(callback)
    }

    /// Configures a subscription: filtering, provenance, and where the callback
    /// runs. See [`Watch`].
    /// Configures a subscription: filtering, provenance, and where the
    /// callback runs. See [`Watch`].
    pub fn subscription_with(&self) -> Watch<Self, Immediate> {
        Watch::new(self.clone())
    }
}

impl<T> Watchable for ReactiveCell<T>
where
    T: Clone + Send + Sync + 'static,
{
    type Item = T;

    fn watch_id(&self) -> Uuid {
        self.origin
    }

    fn watch_raw<F>(&self, callback: F) -> SignalSubscription
    where
        F: Fn(&T, Option<Uuid>) + Send + Sync + 'static,
    {
        self.cache.subscribe_with_source(callback)
    }
}

impl<T> amethystate_core::pipeline::Reactive<T> for ReactiveCell<T>
where
    T: Clone + Send + Sync + 'static,
{
    fn get(&self) -> T {
        self.get()
    }

    fn subscribe_with_source<F>(&self, callback: F) -> SignalSubscription
    where
        F: for<'a> Fn(&'a T, Option<Uuid>) + Send + Sync + 'static,
    {
        self.cache.subscribe_with_source(callback)
    }

    fn keepalive(&self) -> Option<Arc<dyn Send + Sync>> {
        self._keepalive.clone()
    }
}

impl<T: std::fmt::Debug + Clone + Send + Sync + 'static> std::fmt::Debug for ReactiveCell<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ReactiveCell")
            .field("value", &self.get())
            .finish_non_exhaustive()
    }
}

impl<T> Durable<'_, ReactiveCell<T>>
where
    T: Clone + Send + Sync + 'static,
{
    /// Writes the value.
    ///
    /// Returns only once it is on disk rather than buffered - or straight
    /// away for an in-memory cell, which has nothing to commit to.
    pub fn set(&self, value: T) -> WriteResult<()> {
        self.0.set(value)?;
        self.commit()
    }

    /// Like every future, this does nothing until awaited - the write
    /// included. See [`Durable::set_async`].
    pub async fn set_async(&self, value: T) -> WriteResult<()> {
        self.0.set(value)?;
        self.commit_async().await
    }

    /// Reads the value, writes back what `f` returns, and yields it.
    ///
    /// Returns only once it is on disk rather than buffered.
    pub fn update<F>(&self, f: F) -> WriteResult<T>
    where
        F: FnOnce(T) -> T,
    {
        let value = self.0.update(f)?;
        self.commit()?;
        Ok(value)
    }

    /// Like every future, this does nothing until awaited - the write
    /// included. See [`Durable::set_async`].
    pub async fn update_async<F>(&self, f: F) -> WriteResult<T>
    where
        F: FnOnce(T) -> T,
    {
        let value = self.0.update(f)?;
        self.commit_async().await?;
        Ok(value)
    }

    /// Reads the value, lets `f` change it in place, and writes it back.
    ///
    /// Returns only once it is on disk rather than buffered.
    pub fn modify<F>(&self, f: F) -> WriteResult<()>
    where
        F: FnOnce(&mut T),
    {
        self.0.modify(f)?;
        self.commit()
    }

    /// Like every future, this does nothing until awaited - the write
    /// included. See [`Durable::set_async`].
    pub async fn modify_async<F>(&self, f: F) -> WriteResult<()>
    where
        F: FnOnce(&mut T),
    {
        self.0.modify(f)?;
        self.commit_async().await
    }

    fn commit(&self) -> WriteResult<()> {
        match &self.0.commit {
            Some(commit) => (commit.now)(),
            None => Ok(()),
        }
    }

    async fn commit_async(&self) -> WriteResult<()> {
        let pending = self.0.commit.as_ref().map(|commit| (commit.start)());
        match pending {
            Some(commit) => Ok(commit.await?),
            None => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Store;
    use crate::store::StateScope;
    use crate::test_utils::unique_store;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct UiScope;
    impl StateScope for UiScope {
        const PREFIX: &'static str = "ui";
    }

    fn stored_field(store: &Store, name: &'static str, default: u64) -> crate::WritableField<u64> {
        crate::store::field::<UiScope, u64>(store, name, default, Uuid::new_v4()).expect("field")
    }

    #[test]
    fn volatile_cell_round_trips() {
        let cell = ReactiveCell::new(7u64);
        assert_eq!(cell.get(), 7);

        cell.set(9).expect("volatile write cannot fail");
        assert_eq!(cell.get(), 9);
    }

    #[test]
    fn field_cell_writes_reach_the_store() {
        let store = unique_store("write-through");
        let field = stored_field(&store, "width", 110);
        let cell = field.cell();

        cell.set(200).expect("write should reach the store");

        assert_eq!(store.get::<u64>("ui.width").unwrap(), Some(200));
        assert_eq!(cell.get(), 200, "cache must reflect the committed value");
        assert_eq!(field.get(), 200, "field and cell share one cache");
    }

    #[test]
    fn cell_outlives_the_field_it_came_from() {
        // The cache is fed by the store subscription, which the field owns. If
        // the cell did not keep that alive, `get()` would go on answering with
        // a value that stopped being updated - a silent lie rather than a
        // failure. Here the writer holds the field, so dropping our handle
        // changes nothing.
        let store = unique_store("outlive");
        let cell = {
            let field = stored_field(&store, "height", 10);
            field.cell()
        };

        store.set("ui.height", &42u64).expect("external write");

        assert_eq!(
            cell.get(),
            42,
            "cache must still be fed after the field is dropped"
        );
    }

    #[test]
    fn subscribers_see_writes_from_anywhere() {
        let store = unique_store("subscribe");
        let field = stored_field(&store, "depth", 1);
        let cell = field.cell();

        let seen = Arc::new(Mutex::new(Vec::new()));
        let cap = seen.clone();
        let _sub = cell.subscribe(move |v: &u64| cap.lock().unwrap().push(*v));

        cell.set(2).unwrap();
        field.set(3).unwrap();
        store.set("ui.depth", &4u64).unwrap();

        assert_eq!(*seen.lock().unwrap(), vec![2, 3, 4]);
    }

    #[test]
    fn write_through_cell_fires_once_per_write() {
        // The bug that started this refactor: a cell with two writers - a local
        // optimistic set plus the store round-trip - raised subscribers twice
        // for one write. With the store as the only writer, one write is one
        // notification.
        let store = unique_store("single-fire");
        let cell = stored_field(&store, "fires", 0).cell();

        let fires = Arc::new(AtomicUsize::new(0));
        let cap = fires.clone();
        let _sub = cell.subscribe(move |_: &u64| {
            cap.fetch_add(1, Ordering::SeqCst);
        });

        cell.set(1).unwrap();

        assert_eq!(fires.load(Ordering::SeqCst), 1, "one write, one fire");
    }

    #[test]
    fn rejected_write_reports_and_leaves_the_cache_alone() {
        // An interceptor refusing the write must surface as an error, and the
        // cell must keep reporting what is actually stored - not the value
        // somebody tried to write.
        let store = unique_store("rejected");
        let field = stored_field(&store, "guarded", 5);
        let _guard = field.intercept(|_change| None);
        let cell = field.cell();

        let result = cell.set(99);

        assert!(result.is_err(), "rejected write must not report success");
        assert_eq!(
            cell.get(),
            5,
            "cache must not hold a value the store refused"
        );
        assert_eq!(store.get::<u64>("ui.guarded").unwrap(), Some(5));
    }

    #[test]
    fn clones_share_one_value() {
        let cell = ReactiveCell::new(1u64);
        let twin = cell.clone();

        twin.set(2).unwrap();

        assert_eq!(cell.get(), 2, "clones are handles onto the same cell");
    }
}
