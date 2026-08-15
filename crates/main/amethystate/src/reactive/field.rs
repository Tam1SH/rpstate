use super::cell::ReactiveCell;
use super::error::{FieldError, ReactiveFieldResult};
use crate::reactive::watch::{Immediate, Watch, Watchable};
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

pub struct Field<TValue, M: AccessMode = ReadOnlyMode> {
    pub(crate) core: FieldCore<TValue>,
    pub(crate) path: Arc<str>,
    pub(crate) instance_id: Uuid,
    pub(crate) store_sub: Option<Arc<StoreSubscription>>,
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
            .field("path", &self.path)
            .field("value", &self.get())
            .finish_non_exhaustive()
    }
}

impl<TValue, M: AccessMode> Clone for Field<TValue, M> {
    fn clone(&self) -> Self {
        Self {
            core: self.core.clone(),
            path: Arc::clone(&self.path),
            instance_id: self.instance_id,
            store_sub: self.store_sub.clone(),
            _mode: std::marker::PhantomData,
        }
    }
}

impl<TValue, M> Field<TValue, M>
where
    TValue: FieldValue,
    M: AccessMode,
{
    pub fn fork(&self) -> Self {
        self.fork_with_id(Uuid::new_v4())
    }

    pub fn fork_with_id(&self, new_instance_id: Uuid) -> Self {
        Self {
            core: self.core.clone(),
            path: Arc::clone(&self.path),
            instance_id: new_instance_id,
            store_sub: self.store_sub.clone(),
            _mode: std::marker::PhantomData,
        }
    }

    pub fn get(&self) -> TValue {
        self.core.get()
    }

    pub fn path(&self) -> Arc<str> {
        self.path.clone()
    }

    /// Subscribes to value changes.
    ///
    /// # Thread safety
    ///
    /// The `Send + Sync` bound exists because external changes (e.g. file modified
    /// outside the process) are delivered from a background watcher thread.
    ///
    /// For frameworks that do not support `Send + Sync` callbacks, the recommended
    /// workaround is to bridge via a channel:
    ///
    /// ```rust,ignore
    /// let (tx, rx) = std::sync::mpsc::channel();
    ///
    /// field.subscribe(move |val| {
    ///     let _ = tx.send(val.clone());
    /// });
    ///
    /// // drain rx in your framework's event loop
    /// ```
    #[track_caller]
    pub fn subscribe<F>(&self, callback: F) -> SignalSubscription
    where
        F: for<'a> Fn(&'a TValue) + Send + Sync + 'static,
    {
        self.core.subscribe(callback)
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
        self.instance_id
    }

    fn watch_raw<F>(&self, callback: F) -> SignalSubscription
    where
        F: Fn(&TValue, Option<Uuid>) + Send + Sync + 'static,
    {
        self.core.subscribe_with_source(callback)
    }
}

impl<TValue> Field<TValue, WritableMode>
where
    TValue: FieldValue,
{
    /// This field as a [`ReactiveCell`], with the store backend and access
    /// mode erased. Writes go through [`Field::set`], keeping this field's
    /// provenance.
    pub fn cell(&self) -> ReactiveCell<TValue> {
        let me = self.clone();

        let commit = self.store_sub.as_ref().map(|sub| {
            let flush_store = sub.store.clone();
            let flush_path = self.path.clone();
            let start_store = sub.store.clone();

            crate::reactive::cell::CellCommit {
                now: Arc::new(move || Ok(flush_store.flush_prefix(&flush_path)?)),
                start: Arc::new(move || start_store.flush_async()),
            }
        });

        ReactiveCell::from_parts(
            self.core.signal.clone(),
            Arc::new(move |value| me.set(value)),
            self.instance_id,
            commit,
            None,
        )
    }

    pub fn update<F>(&self, f: F) -> ReactiveFieldResult<TValue>
    where
        F: FnOnce(TValue) -> TValue,
    {
        let val = self.get();
        let new_val = f(val);
        self.set(new_val.clone())?;
        Ok(new_val)
    }

    pub fn modify<F>(&self, f: F) -> ReactiveFieldResult<()>
    where
        F: FnOnce(&mut TValue),
    {
        let mut val = self.get();
        f(&mut val);
        self.set(val)
    }

    pub fn set(&self, value: TValue) -> ReactiveFieldResult<()> {
        tracing::trace!(
            target: "amethystate",
            path = %self.path,
            source = crate::observability::resolve_instance_short(self.instance_id).unwrap_or("external"),
            "field write",
        );

        if let Some(sub) = &self.store_sub {
            let backend = SyncBridge::new(sub.store.clone());
            amethystate_core::field_set(
                &backend,
                &self.core,
                self.path.clone(),
                value,
                Some(self.instance_id),
            )?;
        } else {
            let change = self
                .core
                .run_interceptors(self.path.clone(), value, Some(self.instance_id))
                .map_err(|_| FieldError::Intercepted)?;
            self.core
                .signal
                .set_forwarded(change.new_value, change.source);
        }
        Ok(())
    }

    /// The same writes, each returning only once the value is on disk.
    ///
    /// `set` and friends leave the value in the write buffer, where a crash
    /// loses it; these pay a commit to close that window.
    pub fn durable(&self) -> crate::store::Durable<'_, Self> {
        crate::store::Durable(self)
    }

    pub fn intercept<F>(&self, callback: F) -> InterceptDisposer
    where
        F: Fn(Change<TValue>) -> Option<Change<TValue>> + Send + Sync + 'static,
    {
        self.core.intercept(self.path.clone(), callback)
    }

    pub fn new_volatile(path: Arc<str>, default: TValue) -> Self {
        Self::new_volatile_with_id(path, default, Uuid::new_v4())
    }

    pub fn new_volatile_with_id(path: Arc<str>, default: TValue, instance_id: Uuid) -> Self {
        Self {
            core: FieldCore::new(default),
            path,
            instance_id,
            store_sub: None,
            _mode: std::marker::PhantomData,
        }
    }
}

impl<TValue, M: AccessMode> PartialEq for Field<TValue, M> {
    fn eq(&self, other: &Self) -> bool {
        self.path == other.path
            && self.instance_id == other.instance_id
            && Arc::ptr_eq(&self.core.signal.value, &other.core.signal.value)
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
        self.core.subscribe_with_source(callback)
    }

    fn subscribe<F>(&self, callback: F) -> SignalSubscription
    where
        F: for<'a> Fn(&'a TValue) + Send + Sync + 'static,
    {
        self.subscribe(callback)
    }
}

impl<TValue> crate::store::Durable<'_, Field<TValue, WritableMode>>
where
    TValue: FieldValue,
{
    fn commit(&self) -> ReactiveFieldResult<()> {
        if let Some(sub) = &self.0.store_sub {
            sub.store.flush_prefix(&self.0.path)?;
        }
        Ok(())
    }

    async fn commit_async(&self) -> ReactiveFieldResult<()> {
        if let Some(sub) = &self.0.store_sub {
            sub.store.flush_async().await?;
        }
        Ok(())
    }

    pub fn set(&self, value: TValue) -> ReactiveFieldResult<()> {
        self.0.set(value)?;
        self.commit()
    }

    pub async fn set_async(&self, value: TValue) -> ReactiveFieldResult<()> {
        self.0.set(value)?;
        self.commit_async().await
    }

    pub fn update<F>(&self, f: F) -> ReactiveFieldResult<TValue>
    where
        F: FnOnce(TValue) -> TValue,
    {
        let value = self.0.update(f)?;
        self.commit()?;
        Ok(value)
    }

    pub async fn update_async<F>(&self, f: F) -> ReactiveFieldResult<TValue>
    where
        F: FnOnce(TValue) -> TValue,
    {
        let value = self.0.update(f)?;
        self.commit_async().await?;
        Ok(value)
    }

    pub fn modify<F>(&self, f: F) -> ReactiveFieldResult<()>
    where
        F: FnOnce(&mut TValue),
    {
        self.0.modify(f)?;
        self.commit()
    }

    pub async fn modify_async<F>(&self, f: F) -> ReactiveFieldResult<()>
    where
        F: FnOnce(&mut TValue),
    {
        self.0.modify(f)?;
        self.commit_async().await
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

        field.core.signal.set(22);
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
                core,
                path: Arc::from("test.field"),
                store_sub: Some(Arc::new(StoreSubscription {
                    store: store.clone(),
                    id: sub_id,
                })),
                instance_id: Default::default(),
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

        assert_eq!(cell.get(), 7);
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

        field.core.intercept_depth.store(100, Ordering::SeqCst);

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

        field.core.signal.set(3);
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
