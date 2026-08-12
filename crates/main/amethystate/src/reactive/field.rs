use super::cell::ReactiveCell;
use super::error::{FieldError, ReactiveFieldResult};
use crate::store::sync_backend::StoreBackend;
use crate::store::{Store, SubscriptionId};
use crate::{AccessMode, ReadOnlyMode, WritableMode};
use amethystate_core::{Change, FieldCore, InterceptDisposer, Signal, SignalSubscription};
use std::sync::Arc;
use uuid::Uuid;

pub use amethystate_core::primitives::field_core::FieldValue;

pub struct StoreSubscription<S: Store> {
    pub store: S,
    pub id: SubscriptionId,
}

impl<S: Store> Drop for StoreSubscription<S> {
    fn drop(&mut self) {
        self.store.unsubscribe(self.id);
    }
}

pub struct Field<TValue, S: Store, M: AccessMode = ReadOnlyMode> {
    pub(crate) core: FieldCore<TValue>,
    pub(crate) path: Arc<str>,
    pub(crate) instance_id: Uuid,
    pub(crate) store_sub: Option<Arc<StoreSubscription<S>>>,
    pub(crate) _mode: std::marker::PhantomData<M>,
}

pub type ReadOnlyField<TValue, S> = Field<TValue, S, ReadOnlyMode>;
pub type WritableField<TValue, S> = Field<TValue, S, WritableMode>;

impl<TValue, S: Store, M: AccessMode> Clone for Field<TValue, S, M> {
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

impl<TValue, S, M> Field<TValue, S, M>
where
    TValue: FieldValue,
    S: Store,
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

    pub fn as_signal(&self) -> Signal<TValue> {
        self.core.signal.clone()
    }

    #[track_caller]
    pub fn subscribe_external<F>(&self, callback: F) -> SignalSubscription
    where
        F: Fn(TValue) + Send + Sync + 'static,
    {
        let my_id = self.instance_id;
        self.core.subscribe_with_source(move |val, src| {
            if src != Some(my_id) {
                callback(val);
            }
        })
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
    ///     let _ = tx.send(val);
    /// });
    ///
    /// // drain rx in your framework's event loop
    /// ```
    #[track_caller]
    pub fn subscribe<F>(&self, callback: F) -> SignalSubscription
    where
        F: Fn(TValue) + Send + Sync + 'static,
    {
        self.core.subscribe(callback)
    }
}

impl<TValue, S> Field<TValue, S, WritableMode>
where
    TValue: FieldValue,
    S: Store,
{
    /// This field as a [`ReactiveCell`], with the store backend and access mode
    /// erased so it can be stored and passed around next to cells over other
    /// primitives.
    ///
    /// Writes through the cell go through [`Field::set`], so they reach the
    /// store and keep this field's provenance - `subscribe_external` and
    /// tracing still tell them apart from anyone else's writes.
    ///
    /// No `keepalive` is needed: the field captured by the writer already holds
    /// the store subscription that feeds the cache.
    pub fn cell(&self) -> ReactiveCell<TValue> {
        let me = self.clone();

        ReactiveCell::from_parts(
            self.core.signal.clone(),
            Arc::new(move |value| me.set(value)),
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
            let backend = StoreBackend::new(sub.store.clone());
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

impl<TValue, S: Store, M: AccessMode> PartialEq for Field<TValue, S, M> {
    fn eq(&self, other: &Self) -> bool {
        self.path == other.path
            && self.instance_id == other.instance_id
            && Arc::ptr_eq(&self.core.signal.value, &other.core.signal.value)
    }
}

impl<TValue, S: Store, M: AccessMode> Eq for Field<TValue, S, M> {}

impl<TValue, S, M> amethystate_core::pipeline::Reactive<TValue> for Field<TValue, S, M>
where
    TValue: FieldValue,
    S: Store,
    M: AccessMode,
{
    fn get(&self) -> TValue {
        self.get()
    }

    fn subscribe_with_source<F>(&self, callback: F) -> SignalSubscription
    where
        F: Fn(TValue, Option<Uuid>) + Send + Sync + 'static,
    {
        self.core.subscribe_with_source(callback)
    }

    fn subscribe<F>(&self, callback: F) -> SignalSubscription
    where
        F: Fn(TValue) + Send + Sync + 'static,
    {
        self.subscribe(callback)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{StateScope, Store};
    use crate::test_utils::unique_store;
    use crate::{DefaultStore, SubscriptionKind};
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
        let field = crate::store::field::<UiScope, i32, DefaultStore>(
            &store,
            "font_size",
            14,
            Uuid::new_v4(),
        )
        .expect("field should be created");

        assert_eq!(field.get(), 14);
        assert_eq!(field.path().as_ref(), "ui.font_size");

        field.set(18).expect("set should succeed");
        assert_eq!(store.get::<i32>("ui.font_size").unwrap(), Some(18));

        let callback_val = Arc::new(Mutex::new(0i32));
        let cap = callback_val.clone();
        let _sub = field.subscribe(move |v| {
            *cap.lock().unwrap() = v;
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

            let field: Field<String, DefaultStore, WritableMode> = Field {
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

    #[test]
    fn field_as_signal_returns_same_arc() {
        let store = unique_store("as-signal");
        let core = FieldCore::new(100i32);
        let sub_id = store.subscribe(crate::store::SubscriptionKind::Any, Arc::new(|_| {}));
        let field: Field<i32, DefaultStore> = Field {
            core: core.clone(),
            path: Arc::from("some.path"),
            store_sub: Some(Arc::new(StoreSubscription {
                store: store.clone(),
                id: sub_id,
            })),
            instance_id: Default::default(),
            _mode: Default::default(),
        };

        let extracted = field.as_signal();
        assert!(Arc::ptr_eq(&core.signal.value, &extracted.value));
    }

    #[test]
    fn test_volatile_field_behavior() {
        let store = unique_store("test_volatile_field_behavior");

        let field_path: Arc<str> = Arc::from("ui.temp_spinner");

        let field =
            Field::<bool, DefaultStore, WritableMode>::new_volatile(field_path.clone(), false);

        let call_count = Arc::new(Mutex::new(0));
        let last_val = Arc::new(Mutex::new(false));

        let c_count = call_count.clone();
        let l_val = last_val.clone();

        let _sub = field.subscribe(move |val| {
            *c_count.lock().unwrap() += 1;
            *l_val.lock().unwrap() = val;
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
        let field = Field::<i32, DefaultStore, WritableMode>::new_volatile(Arc::from("test"), 42);

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
        let field = Field::<i32, DefaultStore, WritableMode>::new_volatile(Arc::from("test"), 1);

        field.core.intercept_depth.store(100, Ordering::SeqCst);

        let _disp = field.intercept(|mut c| {
            c.new_value = 999;
            Some(c)
        });

        field.set(10).unwrap();
        assert_eq!(field.get(), 10);
    }

    #[test]
    #[traced_test]
    fn test_field_recursion_warning() {
        let field = Field::<i32, DefaultStore, WritableMode>::new_volatile(
            Arc::from("test.recursive_field"),
            0,
        );

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
        let field =
            Field::<i32, DefaultStore, WritableMode>::new_volatile(Arc::from("test.ext"), 0);
        let fork = field.fork();

        let calls = Arc::new(AtomicUsize::new(0));
        let c_clone = calls.clone();

        let _sub = field.subscribe_external(move |_| {
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

        let field = crate::store::field::<UiScope, i32, DefaultStore>(
            &store,
            "persistent_val",
            100,
            Uuid::new_v4(),
        )
        .expect("field should be created");

        let fork = field.fork();

        let calls = Arc::new(AtomicUsize::new(0));
        let c_clone = calls.clone();

        let _sub = field.subscribe_external(move |_| {
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
        let field = Field::<i32, DefaultStore, WritableMode>::new_volatile(
            Arc::from("test.update_modify"),
            10,
        );

        let updated = field.update(|val| val + 5).unwrap();
        assert_eq!(updated, 15);

        field.modify(|val| *val += 10).unwrap();
        assert_eq!(field.get(), 25);
    }
}
