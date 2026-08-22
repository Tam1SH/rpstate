use crate::primitives::signal::{Signal, SignalSubscription};
use std::sync::{Arc, Mutex};
use uuid::Uuid;

/// Something you can read and subscribe to.
///
/// Subscribers are handed `&T`, not an owned value. Delivery is synchronous and
/// the emitter holds the value for the whole dispatch, so the reference is
/// sound by construction; taking it by value only meant every implementation
/// cloned for every subscriber on every emit, whether or not anyone needed to
/// own the value. Callers who do need one call `.clone()` themselves.
pub trait Reactive<T>: Clone + Send + Sync + 'static
where
    T: Clone + Send + Sync + 'static,
{
    fn get(&self) -> T;

    fn subscribe_with_source<F>(&self, callback: F) -> SignalSubscription
    where
        F: for<'a> Fn(&'a T, Option<Uuid>) + Send + Sync + 'static;

    fn subscribe<F>(&self, callback: F) -> SignalSubscription
    where
        F: for<'a> Fn(&'a T) + Send + Sync + 'static,
    {
        self.subscribe_with_source(move |v, _src| callback(v))
    }

    fn keepalive(&self) -> Option<Arc<dyn Send + Sync>> {
        None
    }
}

pub trait IntoPipeline<T>
where
    T: Clone + Send + Sync + 'static,
{
    fn pipe(self) -> Pipeline<T>;
}

struct PipelineInner<T> {
    signal: Arc<Signal<T>>,
    _source_subs: Vec<SignalSubscription>,
    _keepalive: Vec<Arc<dyn Send + Sync>>,
}

pub struct Pipeline<T> {
    inner: Arc<PipelineInner<T>>,
}

impl<T> Clone for Pipeline<T> {
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
        }
    }
}

impl<T> Pipeline<T>
where
    T: Clone + Send + Sync + 'static,
{
    pub fn from_signal(
        signal: Arc<Signal<T>>,
        source_subs: Vec<SignalSubscription>,
        keepalive: Vec<Arc<dyn Send + Sync>>,
    ) -> Self {
        Self {
            inner: Arc::new(PipelineInner {
                signal,
                _source_subs: source_subs,
                _keepalive: keepalive,
            }),
        }
    }

    pub fn get(&self) -> T {
        self.inner.signal.get()
    }

    pub fn subscribe_with_source<F>(&self, callback: F) -> SignalSubscription
    where
        F: for<'a> Fn(&'a T, Option<Uuid>) + Send + Sync + 'static,
    {
        self.inner.signal.subscribe_with_source(callback)
    }

    pub fn subscribe<F>(&self, callback: F) -> SignalSubscription
    where
        F: for<'a> Fn(&'a T) + Send + Sync + 'static,
    {
        self.subscribe_with_source(move |val, _src| callback(val))
    }

    pub fn map<U, F>(self, f: F) -> Pipeline<U>
    where
        U: Clone + Send + Sync + 'static,
        F: Fn(T) -> U + Send + Sync + 'static,
    {
        let f = Arc::new(f);
        let initial = f(self.get());
        let signal = Arc::new(Signal::new(initial));
        let target = Arc::clone(&signal);
        let mapper = Arc::clone(&f);

        let sub = self.subscribe_with_source(move |value, source| {
            target.set_forwarded(mapper(value.clone()), source);
        });

        Pipeline {
            inner: Arc::new(PipelineInner {
                signal,
                _source_subs: vec![sub],
                _keepalive: vec![self.inner],
            }),
        }
    }

    pub fn filter_map<U, F>(self, f: F) -> Pipeline<U>
    where
        U: Default + Clone + Send + Sync + 'static,
        F: Fn(T) -> Option<U> + Send + Sync + 'static,
    {
        let f = Arc::new(f);
        let initial = f(self.get()).unwrap_or_default();
        let signal = Arc::new(Signal::new(initial));
        let target = Arc::clone(&signal);
        let mapper = Arc::clone(&f);

        let sub = self.subscribe_with_source(move |value, source| {
            if let Some(mapped) = mapper(value.clone()) {
                target.set_forwarded(mapped, source);
            }
        });

        Pipeline {
            inner: Arc::new(PipelineInner {
                signal,
                _source_subs: vec![sub],
                _keepalive: vec![self.inner],
            }),
        }
    }

    pub fn inspect<F>(self, f: F) -> Pipeline<T>
    where
        F: Fn(&T) + Send + Sync + 'static,
    {
        let f = Arc::new(f);
        let initial = self.get();
        f(&initial);

        let signal = Arc::new(Signal::new(initial));
        let target = Arc::clone(&signal);
        let inspector = Arc::clone(&f);

        let sub = self.subscribe_with_source(move |value, source| {
            inspector(value);
            target.set_forwarded(value.clone(), source);
        });

        Pipeline {
            inner: Arc::new(PipelineInner {
                signal,
                _source_subs: vec![sub],
                _keepalive: vec![self.inner],
            }),
        }
    }

    pub fn dedupe(self) -> Pipeline<T>
    where
        T: PartialEq,
    {
        let initial = self.get();
        let last = Arc::new(Mutex::new(initial.clone()));
        let signal = Arc::new(Signal::new(initial));
        let target = Arc::clone(&signal);
        let last_seen = Arc::clone(&last);

        let sub = self.subscribe_with_source(move |value, source| {
            let mut last = last_seen.lock().unwrap();
            if *last != *value {
                *last = value.clone();
                target.set_forwarded(value.clone(), source);
            }
        });

        Pipeline {
            inner: Arc::new(PipelineInner {
                signal,
                _source_subs: vec![sub],
                _keepalive: vec![self.inner],
            }),
        }
    }
}

impl<T> Reactive<T> for Pipeline<T>
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
        self.subscribe_with_source(callback)
    }

    fn keepalive(&self) -> Option<Arc<dyn Send + Sync>> {
        Some(self.inner.clone())
    }
}

/// Subscriptions held together, ending when the scope does.
///
/// Not `Clone`, for the reason [`SignalSubscription`] is not: a copy of the
/// scope would be a second chance to end every subscription in it, and the
/// first copy dropped would take them all.
#[derive(Default)]
pub struct ReactiveScope {
    subs: Vec<SignalSubscription>,
}

impl ReactiveScope {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn watch(&mut self, sub: SignalSubscription) {
        self.subs.push(sub);
    }

    pub fn watch_scope(&mut self, mut other: Self) {
        self.subs.append(&mut other.subs);
    }

    pub fn clear(&mut self) {
        self.subs.clear();
    }

    pub fn len(&self) -> usize {
        self.subs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.subs.is_empty()
    }
}

impl<R, T> IntoPipeline<T> for R
where
    R: Reactive<T>,
    T: Clone + Send + Sync + 'static,
{
    fn pipe(self) -> Pipeline<T> {
        let initial = self.get();
        let signal = Arc::new(Signal::new(initial));
        let target = Arc::clone(&signal);
        let sub = self.subscribe_with_source(move |value, source| {
            target.set_forwarded(value.clone(), source);
        });

        Pipeline {
            inner: Arc::new(PipelineInner {
                signal,
                _source_subs: vec![sub],
                _keepalive: self.keepalive().into_iter().collect(),
            }),
        }
    }
}

macro_rules! impl_tuple_pipeline {
    ($(($source_ty:ident, $source:ident, $value_ty:ident, $value:ident)),+ $(,)?) => {
        impl<$($source_ty, $value_ty),+> IntoPipeline<($($value_ty,)+)> for ($($source_ty,)+)
        where
            $(
                $source_ty: Reactive<$value_ty>,
                $value_ty: Clone + Send + Sync + 'static,
            )+
        {
            #[allow(non_snake_case)]
            fn pipe(self) -> Pipeline<($($value_ty,)+)> {
                let ($($source,)+) = self;
                let initial = ($($source.get(),)+);
                let signal = Arc::new(Signal::new(initial));
                let mut source_subs = Vec::new();
                let mut keepalive = Vec::new();

                let refresh: Arc<dyn Fn() -> ($($value_ty,)+) + Send + Sync> = {
                    $(let $value = $source.clone();)+
                    Arc::new(move || ($($value.get(),)+))
                };

                $(
                    if let Some(inner) = $source.keepalive() {
                        keepalive.push(inner);
                    }

                    let target = Arc::clone(&signal);
                    let refresh_cb = Arc::clone(&refresh);
                    source_subs.push($source.subscribe_with_source(move |_, src| {
                        target.set_forwarded(refresh_cb(), src);
                    }));
                )+

                Pipeline::from_signal(signal, source_subs, keepalive)
            }
        }
    };
}

impl_tuple_pipeline!((RA, ra, A, a), (RB, rb, B, b));
impl_tuple_pipeline!((RA, ra, A, a), (RB, rb, B, b), (RC, rc, C, c));
impl_tuple_pipeline!(
    (RA, ra, A, a),
    (RB, rb, B, b),
    (RC, rc, C, c),
    (RD, rd, D, d)
);
