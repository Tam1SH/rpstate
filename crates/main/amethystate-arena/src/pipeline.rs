use crate::{DefaultArena, FieldHandle};
use amethystate::{AccessMode, Pipeline, Signal, SignalSubscription};
use serde::{Serialize, de::DeserializeOwned};
use std::cell::RefCell;
use std::sync::Arc;
use uuid::Uuid;

thread_local! {
    pub static PIPELINE_ARENA: RefCell<Option<DefaultArena>> = const { RefCell::new(None) };
}

pub fn pipeline_arena() -> DefaultArena {
    PIPELINE_ARENA.with(|a| a.borrow().clone().expect("called outside use_pipeline"))
}

pub trait ArenaReactive<T>: Clone + Send + Sync + 'static
where
    T: Clone + Send + Sync + 'static,
{
    fn get_with(&self, arena: &DefaultArena) -> T;

    fn get(&self) -> T {
        self.get_with(&pipeline_arena())
    }

    fn subscribe_with_source<F>(&self, arena: &DefaultArena, callback: F) -> SignalSubscription
    where
        F: Fn(T, Option<Uuid>) + Send + Sync + 'static;

    fn subscribe<F>(&self, arena: &DefaultArena, callback: F) -> SignalSubscription
    where
        F: Fn(T) + Send + Sync + 'static,
    {
        self.subscribe_with_source(arena, move |v, _src| callback(v))
    }
}

impl<T, M> ArenaReactive<T> for FieldHandle<T, M>
where
    T: DeserializeOwned + Serialize + Clone + Send + Sync + PartialEq + 'static,
    M: AccessMode,
{
    fn get_with(&self, arena: &DefaultArena) -> T {
        arena.get_field(*self)
    }

    fn subscribe_with_source<F>(&self, arena: &DefaultArena, callback: F) -> SignalSubscription
    where
        F: Fn(T, Option<Uuid>) + Send + Sync + 'static,
    {
        arena.subscribe_field_with_source(*self, callback)
    }
}

pub trait IntoArenaPipeline<T>
where
    T: Clone + Send + Sync + 'static,
{
    fn pipe(self) -> Pipeline<T>;
}

impl<R, T> IntoArenaPipeline<T> for R
where
    R: ArenaReactive<T>,
    T: Clone + Send + Sync + 'static,
{
    fn pipe(self) -> Pipeline<T> {
        let initial = self.get();
        let signal = Arc::new(Signal::new(initial));
        let target = Arc::clone(&signal);
        let sub = self.subscribe_with_source(&pipeline_arena(), move |val, source| {
            target.set_forwarded(val, source);
        });
        Pipeline::from_signal(signal, vec![sub], vec![])
    }
}

macro_rules! impl_tuple_pipeline {
    ($(($source_ty:ident, $source:ident, $value_ty:ident, $value:ident)),+ $(,)?) => {
        impl<$($source_ty, $value_ty),+> IntoArenaPipeline<($($value_ty,)+)> for ($($source_ty,)+)
        where
            $(
                $source_ty: ArenaReactive<$value_ty>,
                $value_ty: Clone + Send + Sync + 'static,
            )+
        {
            #[allow(non_snake_case)]
            fn pipe(self) -> Pipeline<($($value_ty,)+)> {
                let ($($source,)+) = self;
                let initial = ($($source.get(),)+);
                let signal = Arc::new(Signal::new(initial));
                let mut source_subs = Vec::new();

                let refresh: Arc<dyn Fn() -> ($($value_ty,)+) + Send + Sync> = {
                    let arena = pipeline_arena();
                    $(let $value = $source.clone();)+
                    Arc::new(move || ($($value.get_with(&arena),)+))
                };

                $(
                    let target = Arc::clone(&signal);
                    let refresh_cb = Arc::clone(&refresh);
                    source_subs.push($source.subscribe_with_source(&pipeline_arena(), move |_, src| {
                        target.set_forwarded(refresh_cb(), src);
                    }));
                )+

                Pipeline::from_signal(signal, source_subs, vec![])
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
