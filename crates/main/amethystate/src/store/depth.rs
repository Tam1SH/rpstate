//! How deep a value is, learned by watching it go past.
//!
//! Every codec here reads less deeply than it writes. `serde_json` stops at
//! 128 on the way in and has no limit on the way out; `ron` stops at 64;
//! `rmp_serde` has no limit at all and the stack runs out around three
//! thousand instead, which kills the process rather than returning an error -
//! and does so on every later start, because the value is already committed.
//!
//! So a write past the reader's ceiling is accepted and cannot be read back,
//! which is the worst shape a defect can have: no error anywhere, and the file
//! is gone. Learning the depth first is the only way to refuse it.
//!
//! The value cannot be inspected - by the time it reaches a store it is a
//! `&dyn erased_serde::Serialize`, and a five-level struct is indistinguishable
//! from a five-level tree. Nor can it be built and walked: building is the
//! dangerous act, and on redb it is what overflows the stack.
//!
//! Serde is a push protocol and the store is on the receiving end, so it counts
//! what arrives. Nothing is allocated, no node is built, no type is needed, and
//! the stack at the point of refusal is `limit` frames deep by construction.
//! `serde_json` does exactly this on the read side; this is the same thing on
//! the write side, where nobody had put it.

use crate::store::builder::Backend;
use crate::store::config::WriteLimits;
use crate::store::{CodecFormat, StorageError, StorageResult};
use amethystate_core::path::StorePath;
use error_stack::Report;
use serde::ser::{
    self, Serialize, SerializeMap, SerializeSeq, SerializeStruct, SerializeStructVariant,
    SerializeTuple, SerializeTupleStruct, SerializeTupleVariant, Serializer,
};
use std::cell::Cell;
use std::fmt::{self, Display};

/// Why a value could not be measured.
#[derive(Debug)]
pub enum DepthError {
    /// It nests further than the limit, and stopped being followed there.
    TooDeep { limit: usize },

    /// The value's own `Serialize` refused, before depth came into it.
    Refused(String),
}

impl Display for DepthError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DepthError::TooDeep { limit } => {
                write!(f, "the value nests deeper than {limit} levels")
            }
            DepthError::Refused(why) => f.write_str(why),
        }
    }
}

impl std::error::Error for DepthError {}

impl ser::Error for DepthError {
    fn custom<T: Display>(msg: T) -> Self {
        DepthError::Refused(msg.to_string())
    }
}

/// How many levels `value` nests, or that it goes past `limit`.
///
/// A scalar is zero: it opens nothing. A `Vec<u8>` is one, a `Vec<Vec<u8>>` two.
/// Counting stops at `limit + 1`, so a value far deeper than the limit costs no
/// more to refuse than one just past it.
pub fn depth_of<T>(value: &T, limit: usize, human_readable: bool) -> Result<usize, DepthError>
where
    T: Serialize + ?Sized,
{
    let state = State::new(limit, human_readable);
    value.serialize(state.counter())?;
    Ok(state.deepest.get())
}

/// The same for a value that has already been erased, which is how one reaches
/// a store.
pub fn depth_of_erased(
    value: &dyn erased_serde::Serialize,
    limit: usize,
    human_readable: bool,
) -> Result<usize, DepthError> {
    let state = State::new(limit, human_readable);
    let mut erased = <dyn erased_serde::Serializer>::erase(state.counter());

    match value.erased_serialize(&mut erased) {
        Ok(()) => Ok(state.deepest.get()),
        // `erased_serde::Error` does not downcast, so the refusal cannot be
        // recognised by its type once it has been through the erasure. The
        // counter records it on the way out instead.
        Err(_) if state.too_deep.get() => Err(DepthError::TooDeep { limit }),
        Err(other) => Err(DepthError::Refused(other.to_string())),
    }
}

/// What the codec behind `engine` answers to `is_human_readable`.
///
/// Only `rmp_serde` says no. The three text codecs and sonic-rs under sqlite
/// all say yes, and a `Serialize` that branches on it - which is ordinary -
/// hands each of them a different shape.
const fn human_readable(engine: Backend) -> bool {
    match engine {
        #[cfg(feature = "redb")]
        Backend::Redb => false,
        #[cfg(feature = "json")]
        Backend::Json => true,
        #[cfg(feature = "toml")]
        Backend::Toml => true,
        #[cfg(feature = "ron")]
        Backend::Ron => true,
        #[cfg(feature = "sqlite")]
        Backend::Sqlite => true,
    }
}

/// What one store may spend, worked out once when it opens.
///
/// The ceiling is the running codec's, lowered by anything the store promised
/// to stay readable on. `key_depth` is the store's own cap on paths, which is a
/// setting rather than a fact.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DepthBudget {
    pub ceiling: usize,
    pub key_depth: Option<usize>,
    /// What the running codec answers to `is_human_readable`, so the count is
    /// of the shape that is actually going to be written.
    pub human_readable: bool,
}

impl DepthBudget {
    /// The budget for a store running on `engine` under `limits`.
    pub fn resolve(limits: &WriteLimits, engine: Backend) -> Self {
        Self {
            ceiling: limits.ceiling(engine),
            key_depth: limits.key_depth,
            human_readable: human_readable(engine),
        }
    }

    /// The same for an engine known only by the codec it runs, which is how a
    /// text store knows itself - it is generic over the document, not over the
    /// backend that chose it.
    pub fn for_codec(limits: &WriteLimits, codec: CodecFormat) -> Self {
        let engine = match codec {
            #[cfg(feature = "redb")]
            CodecFormat::MessagePack => Backend::Redb,
            #[cfg(feature = "json")]
            CodecFormat::Json => Backend::Json,
            #[cfg(feature = "sqlite")]
            CodecFormat::SonicJson => Backend::Sqlite,
            #[cfg(feature = "toml")]
            CodecFormat::Toml => Backend::Toml,
            #[cfg(feature = "ron")]
            CodecFormat::Ron => Backend::Ron,
            #[cfg(test)]
            CodecFormat::Default => {
                return Self {
                    ceiling: usize::MAX,
                    key_depth: limits.key_depth,
                    human_readable: true,
                };
            }
        };
        Self::resolve(limits, engine)
    }

    /// Whether `value` may be written at `path`.
    ///
    /// The path is counted with the value because the budget is shared: on
    /// every text engine the path's levels become the document's, so a shallow
    /// value at a deep path is exactly as unreadable as the reverse. sqlite is
    /// the exception - its path is a `TEXT` key - and paying the path there
    /// costs a few levels out of 254, which is not worth a second rule.
    pub fn check(
        &self,
        path: &StorePath,
        value: &dyn erased_serde::Serialize,
    ) -> StorageResult<()> {
        let levels = path.segments().count();

        if let Some(cap) = self.key_depth
            && levels > cap
        {
            return Err(Report::new(StorageError::Path)
                .attach(format!("path: {path}"))
                .attach(format!("levels: {levels}, and the limit is {cap}"))
                .attach("set by: limits(|l| l.key_depth(..))")
                .attach(format!(
                    "what is stored here spends the same budget - this store reads {} levels in all",
                    self.ceiling
                )));
        }

        let left = self.ceiling.saturating_sub(levels);
        match depth_of_erased(value, left, self.human_readable) {
            Ok(_) => Ok(()),
            Err(DepthError::TooDeep { .. }) => Err(Report::new(StorageError::Codec)
                .attach(format!("path: {path}"))
                .attach(format!(
                    "the path spends {levels} levels and the value goes past the {left} that are left"
                ))
                .attach(format!("this store reads at most {} levels", self.ceiling))
                .attach(
                    "a value deeper than the reader accepts is written without complaint and \
                     cannot be read back",
                )),
            // The value's own `Serialize` refused. That is not this check's
            // business - the write path will meet the same refusal and report
            // it where it belongs, with the codec's own words.
            Err(DepthError::Refused(_)) => Ok(()),
        }
    }
}

/// What one measurement owns, so the `Copy` counter can borrow it.
struct State {
    depth: Cell<usize>,
    deepest: Cell<usize>,
    too_deep: Cell<bool>,
    limit: usize,
    /// The answer the running codec would give.
    ///
    /// Branching on `is_human_readable` is an ordinary thing for a `Serialize`
    /// to do - `uuid` writes a string to json and sixteen bytes to msgpack, and
    /// it is far from alone. A counter that answered for itself would be shown
    /// a shape the codec is not going to write, and would measure the wrong
    /// one: on redb, where the codec says `false`, that let a value through
    /// with room to spare and then killed the process reading it back.
    human_readable: bool,
}

impl State {
    fn new(limit: usize, human_readable: bool) -> Self {
        Self {
            depth: Cell::new(0),
            deepest: Cell::new(0),
            too_deep: Cell::new(false),
            limit,
            human_readable,
        }
    }

    fn counter(&self) -> Counter<'_> {
        Counter { state: self }
    }
}

/// Counts levels and forgets everything else.
///
/// `Copy`, because serde hands the serializer over by value at every level and
/// the state behind it is shared - the depth is one running number, not a
/// stack.
#[derive(Clone, Copy)]
struct Counter<'a> {
    state: &'a State,
}

impl<'a> Counter<'a> {
    /// One level further in, or a refusal if that is past the limit.
    fn enter(self) -> Result<Self, DepthError> {
        let now = self.state.depth.get() + 1;
        if now > self.state.limit {
            self.state.too_deep.set(true);
            return Err(DepthError::TooDeep {
                limit: self.state.limit,
            });
        }
        self.state.depth.set(now);
        if now > self.state.deepest.get() {
            self.state.deepest.set(now);
        }
        Ok(self)
    }

    fn leave(&self) {
        self.state.depth.set(self.state.depth.get().saturating_sub(1));
    }
}

macro_rules! flat {
    ($($method:ident($($arg:ident: $ty:ty),*);)+) => {
        $(fn $method(self $(, $arg: $ty)*) -> Result<(), DepthError> {
            $(let _ = $arg;)*
            Ok(())
        })+
    };
}

impl<'a> Serializer for Counter<'a> {
    type Ok = ();
    type Error = DepthError;
    type SerializeSeq = Self;
    type SerializeTuple = Self;
    type SerializeTupleStruct = Self;
    type SerializeTupleVariant = Self;
    type SerializeMap = Self;
    type SerializeStruct = Self;
    type SerializeStructVariant = Self;

    flat! {
        serialize_bool(v: bool);
        serialize_i8(v: i8);
        serialize_i16(v: i16);
        serialize_i32(v: i32);
        serialize_i64(v: i64);
        serialize_i128(v: i128);
        serialize_u8(v: u8);
        serialize_u16(v: u16);
        serialize_u32(v: u32);
        serialize_u64(v: u64);
        serialize_u128(v: u128);
        serialize_f32(v: f32);
        serialize_f64(v: f64);
        serialize_char(v: char);
        serialize_str(v: &str);
        serialize_bytes(v: &[u8]);
        serialize_unit();
        serialize_unit_struct(name: &'static str);
    }

    fn serialize_unit_variant(
        self,
        _name: &'static str,
        _index: u32,
        _variant: &'static str,
    ) -> Result<(), DepthError> {
        Ok(())
    }

    fn serialize_none(self) -> Result<(), DepthError> {
        Ok(())
    }

    /// `Some` is not a level: no format spends nesting on it, and counting it
    /// would make `Option<T>` measure deeper than the `T` inside.
    fn serialize_some<T>(self, value: &T) -> Result<(), DepthError>
    where
        T: Serialize + ?Sized,
    {
        value.serialize(self)
    }

    fn serialize_newtype_struct<T>(
        self,
        _name: &'static str,
        value: &T,
    ) -> Result<(), DepthError>
    where
        T: Serialize + ?Sized,
    {
        value.serialize(self)
    }

    /// A newtype variant is a level: every format spells it as a wrapper around
    /// the value, whether that is `{"V": x}` or `V(x)`.
    fn serialize_newtype_variant<T>(
        self,
        _name: &'static str,
        _index: u32,
        _variant: &'static str,
        value: &T,
    ) -> Result<(), DepthError>
    where
        T: Serialize + ?Sized,
    {
        let inner = self.enter()?;
        value.serialize(inner)?;
        inner.leave();
        Ok(())
    }

    fn serialize_seq(self, _len: Option<usize>) -> Result<Self, DepthError> {
        self.enter()
    }

    fn serialize_tuple(self, _len: usize) -> Result<Self, DepthError> {
        self.enter()
    }

    fn serialize_tuple_struct(
        self,
        _name: &'static str,
        _len: usize,
    ) -> Result<Self, DepthError> {
        self.enter()
    }

    fn serialize_tuple_variant(
        self,
        _name: &'static str,
        _index: u32,
        _variant: &'static str,
        _len: usize,
    ) -> Result<Self, DepthError> {
        self.enter()
    }

    fn serialize_map(self, _len: Option<usize>) -> Result<Self, DepthError> {
        self.enter()
    }

    fn serialize_struct(self, _name: &'static str, _len: usize) -> Result<Self, DepthError> {
        self.enter()
    }

    fn serialize_struct_variant(
        self,
        _name: &'static str,
        _index: u32,
        _variant: &'static str,
        _len: usize,
    ) -> Result<Self, DepthError> {
        self.enter()
    }

    fn is_human_readable(&self) -> bool {
        self.state.human_readable
    }
}

macro_rules! compound {
    ($($trait_name:ident { $($method:ident($($arg:ident: $ty:ty),*)),+ $(,)? })+) => {
        $(impl<'a> $trait_name for Counter<'a> {
            type Ok = ();
            type Error = DepthError;

            $(fn $method<T>(&mut self $(, $arg: $ty)*, value: &T) -> Result<(), DepthError>
            where
                T: Serialize + ?Sized,
            {
                $(let _ = $arg;)*
                value.serialize(*self)
            })+

            fn end(self) -> Result<(), DepthError> {
                self.leave();
                Ok(())
            }
        })+
    };
}

compound! {
    SerializeSeq { serialize_element() }
    SerializeTuple { serialize_element() }
    SerializeTupleStruct { serialize_field() }
    SerializeTupleVariant { serialize_field() }
    SerializeStruct { serialize_field(key: &'static str) }
    SerializeStructVariant { serialize_field(key: &'static str) }
}

impl<'a> SerializeMap for Counter<'a> {
    type Ok = ();
    type Error = DepthError;

    fn serialize_key<T>(&mut self, key: &T) -> Result<(), DepthError>
    where
        T: Serialize + ?Sized,
    {
        key.serialize(*self)
    }

    fn serialize_value<T>(&mut self, value: &T) -> Result<(), DepthError>
    where
        T: Serialize + ?Sized,
    {
        value.serialize(*self)
    }

    fn end(self) -> Result<(), DepthError> {
        self.leave();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[derive(serde::Serialize)]
    struct Flat {
        a: u32,
        b: String,
    }

    #[derive(serde::Serialize)]
    struct Nested {
        inner: Flat,
    }

    #[derive(serde::Serialize)]
    enum Shape {
        Unit,
        Newtype(u32),
        Tuple(u32, u32),
        Struct { a: u32 },
    }

    /// Nests `n` deep and no further, so a measurement can be checked against a
    /// number rather than against another measurement.
    struct Ladder(usize);

    impl Serialize for Ladder {
        fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
            use serde::ser::SerializeSeq;
            if self.0 == 0 {
                return s.serialize_u32(0);
            }
            let mut seq = s.serialize_seq(Some(1))?;
            seq.serialize_element(&Ladder(self.0 - 1))?;
            seq.end()
        }
    }

    #[test]
    fn a_scalar_opens_nothing() {
        assert_eq!(depth_of(&1u32, 10, true).unwrap(), 0);
        assert_eq!(depth_of("text", 10, true).unwrap(), 0);
        assert_eq!(depth_of(&(), 10, true).unwrap(), 0);
    }

    #[test]
    fn a_level_is_a_level_whatever_opens_it() {
        assert_eq!(depth_of(&vec![1u32, 2], 10, true).unwrap(), 1);
        assert_eq!(depth_of(&Flat::default_for_test(), 10, true).unwrap(), 1);
        assert_eq!(depth_of(&(1u32, 2u32), 10, true).unwrap(), 1);
        assert_eq!(
            depth_of(&HashMap::from([("a".to_string(), 1u32)]), 10, true).unwrap(),
            1
        );
    }

    #[test]
    fn nesting_adds_up() {
        assert_eq!(depth_of(&Nested::default_for_test(), 10, true).unwrap(), 2);
        assert_eq!(depth_of(&vec![vec![1u32]], 10, true).unwrap(), 2);
        assert_eq!(depth_of(&Ladder(7), 20, true).unwrap(), 7);
    }

    /// The deepest branch is what a reader has to follow, not the last one.
    #[test]
    fn the_answer_is_the_deepest_branch_not_the_final_one() {
        let uneven = (Ladder(5), Ladder(1));
        assert_eq!(depth_of(&uneven, 20, true).unwrap(), 6);
    }

    /// An `Option` is not a level, or `Option<T>` would measure deeper than the
    /// `T` it holds and every optional field would cost one for nothing.
    #[test]
    fn an_option_costs_nothing() {
        assert_eq!(depth_of(&Some(vec![1u32]), 10, true).unwrap(), 1);
        assert_eq!(depth_of(&None::<Vec<u32>>, 10, true).unwrap(), 0);
        assert_eq!(depth_of(&Some(Some(1u32)), 10, true).unwrap(), 0);
    }

    #[test]
    fn a_variant_costs_what_it_wraps() {
        assert_eq!(depth_of(&Shape::Unit, 10, true).unwrap(), 0);
        assert_eq!(depth_of(&Shape::Newtype(1), 10, true).unwrap(), 1);
        assert_eq!(depth_of(&Shape::Tuple(1, 2), 10, true).unwrap(), 1);
        assert_eq!(depth_of(&Shape::Struct { a: 1 }, 10, true).unwrap(), 1);
    }

    /// The refusal arrives without following the value to the bottom, which is
    /// the whole reason for counting rather than building.
    #[test]
    fn a_value_past_the_limit_is_refused_rather_than_followed() {
        let err = depth_of(&Ladder(5_000), 8, true).unwrap_err();
        assert!(
            matches!(err, DepthError::TooDeep { limit: 8 }),
            "expected a depth refusal naming the limit, got {err}"
        );
    }

    #[test]
    fn the_limit_itself_is_allowed() {
        assert_eq!(depth_of(&Ladder(8), 8, true).unwrap(), 8);
        assert!(depth_of(&Ladder(9), 8, true).is_err());
    }

    /// The erased path is the one a store actually uses.
    #[test]
    fn an_erased_value_measures_the_same() {
        let value: &dyn erased_serde::Serialize = &vec![vec![1u32]];
        assert_eq!(depth_of_erased(value, 10, true).unwrap(), 2);

        let deep: &dyn erased_serde::Serialize = &Ladder(20);
        assert!(matches!(
            depth_of_erased(deep, 4, true).unwrap_err(),
            DepthError::TooDeep { limit: 4 }
        ));
    }

    /// Branching on `is_human_readable` is ordinary - `uuid` writes a string to
    /// json and sixteen bytes to msgpack, and it is far from alone - so a
    /// counter that answered for itself would be shown a shape the codec is not
    /// going to write.
    ///
    /// On redb the codec says no, and the value below is shallow in the form
    /// the guard used to see and deep in the one that actually lands. It went
    /// through with room to spare and then killed every process that opened the
    /// file.
    #[test]
    fn the_count_is_of_the_shape_this_codec_will_write() {
        struct TwoFaced;

        impl Serialize for TwoFaced {
            fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
                if s.is_human_readable() {
                    s.serialize_u32(0)
                } else {
                    Ladder(30).serialize(s)
                }
            }
        }

        assert_eq!(depth_of(&TwoFaced, 40, true).unwrap(), 0);
        assert_eq!(depth_of(&TwoFaced, 40, false).unwrap(), 30);

        assert!(
            depth_of(&TwoFaced, 8, true).is_ok(),
            "the human-readable form is a scalar and fits anywhere"
        );
        assert!(
            matches!(
                depth_of(&TwoFaced, 8, false).unwrap_err(),
                DepthError::TooDeep { limit: 8 }
            ),
            "the binary form is thirty levels and must be refused at eight"
        );
    }

    impl Flat {
        fn default_for_test() -> Self {
            Self {
                a: 1,
                b: "b".to_string(),
            }
        }
    }

    impl Nested {
        fn default_for_test() -> Self {
            Self {
                inner: Flat::default_for_test(),
            }
        }
    }
}
