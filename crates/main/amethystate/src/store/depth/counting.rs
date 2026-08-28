//! A serializer that counts what goes through it and hands everything on.
//!
//! The store used to measure a value in a pass of its own and then let the
//! codec serialize it again. That is the same work twice on every write, and
//! worse than wasteful: the two passes were not the same pass. The counter
//! answered `is_human_readable` for itself, so a value whose `Serialize`
//! branches on it - `uuid` writes a string to json and sixteen bytes to
//! msgpack, and it is far from alone - showed one shape to the count and
//! another to the file.
//!
//! Wrapping the codec's own serializer settles both. There is one pass,
//! `is_human_readable` is the codec's own answer because the question is
//! forwarded to it, and a sixth engine cannot be added to one of the two halves
//! and forgotten in the other.
//!
//! A decorator over `Serializer` alone would not do it. `serialize_some` and
//! the compound methods take the nested value and hand it to *their* inner
//! serializer, so everything below the first level would go past uncounted. So
//! each nested value is paired with the budget and re-wraps whatever serializer
//! it is given - the shape `serde_stacker` uses to grow a stack at every
//! recursion point, counting instead of growing.

use serde::ser::{
    self, Serialize, SerializeMap, SerializeSeq, SerializeStruct, SerializeStructVariant,
    SerializeTuple, SerializeTupleStruct, SerializeTupleVariant, Serializer,
};
use std::cell::Cell;

/// How deep one serialization has gone, and how deep it may go.
///
/// Shared by every level of one pass: serde hands the serializer over by value
/// at each level, so the running count cannot live in it.
pub struct Depth {
    depth: Cell<usize>,
    deepest: Cell<usize>,
    overflowed: Cell<bool>,
    limit: usize,
}

impl Depth {
    /// A budget of `limit` levels.
    pub fn new(limit: usize) -> Self {
        Self {
            depth: Cell::new(0),
            deepest: Cell::new(0),
            overflowed: Cell::new(false),
            limit,
        }
    }

    /// A budget nothing can exhaust, for the store's own writes.
    ///
    /// The schema snapshot, the migration log and the initialization markers
    /// are shapes this crate declares rather than shapes an application hands
    /// over, and a schema deep enough to matter would have had its data refused
    /// first. Spelled out so that a write with no limit is a decision someone
    /// made rather than a parameter someone forgot.
    pub fn unlimited() -> Self {
        Self::new(usize::MAX)
    }

    /// `serializer`, counted.
    pub fn wrap<S>(&self, serializer: S) -> Counting<'_, S> {
        Counting {
            inner: serializer,
            depth: self,
        }
    }

    /// `value`, counted by whatever serializer it is eventually handed to.
    ///
    /// The way in that a call site actually wants: a codec's entry point takes
    /// the value and builds its own serializer inside, so there is nothing to
    /// wrap from outside. Wrapping the value instead works whatever the codec
    /// does with it, and `is_human_readable` is still the codec's own answer,
    /// because the question reaches it through the wrapper rather than around
    /// it.
    pub fn count<'v, T>(&self, value: &'v T) -> Counted<'v, '_, T>
    where
        T: Serialize + ?Sized,
    {
        Counted { value, depth: self }
    }

    /// Whether the pass stopped because the value went past the limit.
    ///
    /// The refusal reaches the caller as the codec's own error type, since that
    /// is all a `Serializer` may return, so it cannot be recognised by its
    /// type. This says whether that error was ours.
    pub fn overflowed(&self) -> bool {
        self.overflowed.get()
    }

    /// The deepest level reached, once a pass has finished.
    pub fn deepest(&self) -> usize {
        self.deepest.get()
    }

    fn enter<E: ser::Error>(&self) -> Result<(), E> {
        let now = self.depth.get() + 1;
        if now > self.limit {
            self.overflowed.set(true);
            return Err(E::custom(format_args!(
                "the value nests deeper than {} levels",
                self.limit
            )));
        }
        self.depth.set(now);
        if now > self.deepest.get() {
            self.deepest.set(now);
        }
        Ok(())
    }

    fn leave(&self) {
        self.depth.set(self.depth.get().saturating_sub(1));
    }
}

/// A serializer that counts levels and forwards everything else.
pub struct Counting<'a, S> {
    inner: S,
    depth: &'a Depth,
}

/// A value carrying the budget with it.
///
/// The inner serializer reaches for a nested value on its own - through
/// `serialize_some`, `serialize_element`, `serialize_field` - and hands it a
/// serializer of its own making. Pairing the value with the budget is what puts
/// the counter back in the way at every level rather than only the first.
pub struct Counted<'v, 'd, T: ?Sized> {
    value: &'v T,
    depth: &'d Depth,
}

impl<T> Serialize for Counted<'_, '_, T>
where
    T: Serialize + ?Sized,
{
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.value.serialize(Counting {
            inner: serializer,
            depth: self.depth,
        })
    }
}

/// Methods that open nothing: the value goes straight through.
macro_rules! forward {
    ($($method:ident($($arg:ident: $ty:ty),*);)+) => {
        $(fn $method(self $(, $arg: $ty)*) -> Result<S::Ok, S::Error> {
            self.inner.$method($($arg),*)
        })+
    };
}

impl<'a, S: Serializer> Serializer for Counting<'a, S> {
    type Ok = S::Ok;
    type Error = S::Error;
    type SerializeSeq = Counting<'a, S::SerializeSeq>;
    type SerializeTuple = Counting<'a, S::SerializeTuple>;
    type SerializeTupleStruct = Counting<'a, S::SerializeTupleStruct>;
    type SerializeTupleVariant = Counting<'a, S::SerializeTupleVariant>;
    type SerializeMap = Counting<'a, S::SerializeMap>;
    type SerializeStruct = Counting<'a, S::SerializeStruct>;
    type SerializeStructVariant = Counting<'a, S::SerializeStructVariant>;

    forward! {
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
        serialize_none();
        serialize_unit();
        serialize_unit_struct(name: &'static str);
        serialize_unit_variant(name: &'static str, index: u32, variant: &'static str);
    }

    /// `Some` is not a level: no format spends nesting on it, and counting it
    /// would make `Option<T>` measure deeper than the `T` inside.
    fn serialize_some<T>(self, value: &T) -> Result<S::Ok, S::Error>
    where
        T: Serialize + ?Sized,
    {
        let Counting { inner, depth } = self;
        inner.serialize_some(&Counted { value, depth })
    }

    fn serialize_newtype_struct<T>(self, name: &'static str, value: &T) -> Result<S::Ok, S::Error>
    where
        T: Serialize + ?Sized,
    {
        let Counting { inner, depth } = self;
        inner.serialize_newtype_struct(name, &Counted { value, depth })
    }

    /// A newtype variant is a level: every format spells it as a wrapper around
    /// the value, whether that is `{"V": x}` or `V(x)`.
    fn serialize_newtype_variant<T>(
        self,
        name: &'static str,
        index: u32,
        variant: &'static str,
        value: &T,
    ) -> Result<S::Ok, S::Error>
    where
        T: Serialize + ?Sized,
    {
        let Counting { inner, depth } = self;
        depth.enter()?;
        let out = inner.serialize_newtype_variant(name, index, variant, &Counted { value, depth });
        depth.leave();
        out
    }

    fn serialize_seq(self, len: Option<usize>) -> Result<Self::SerializeSeq, S::Error> {
        self.depth.enter()?;
        Ok(Counting {
            inner: self.inner.serialize_seq(len)?,
            depth: self.depth,
        })
    }

    fn serialize_tuple(self, len: usize) -> Result<Self::SerializeTuple, S::Error> {
        self.depth.enter()?;
        Ok(Counting {
            inner: self.inner.serialize_tuple(len)?,
            depth: self.depth,
        })
    }

    fn serialize_tuple_struct(
        self,
        name: &'static str,
        len: usize,
    ) -> Result<Self::SerializeTupleStruct, S::Error> {
        self.depth.enter()?;
        Ok(Counting {
            inner: self.inner.serialize_tuple_struct(name, len)?,
            depth: self.depth,
        })
    }

    fn serialize_tuple_variant(
        self,
        name: &'static str,
        index: u32,
        variant: &'static str,
        len: usize,
    ) -> Result<Self::SerializeTupleVariant, S::Error> {
        self.depth.enter()?;
        Ok(Counting {
            inner: self
                .inner
                .serialize_tuple_variant(name, index, variant, len)?,
            depth: self.depth,
        })
    }

    fn serialize_map(self, len: Option<usize>) -> Result<Self::SerializeMap, S::Error> {
        self.depth.enter()?;
        Ok(Counting {
            inner: self.inner.serialize_map(len)?,
            depth: self.depth,
        })
    }

    fn serialize_struct(
        self,
        name: &'static str,
        len: usize,
    ) -> Result<Self::SerializeStruct, S::Error> {
        self.depth.enter()?;
        Ok(Counting {
            inner: self.inner.serialize_struct(name, len)?,
            depth: self.depth,
        })
    }

    fn serialize_struct_variant(
        self,
        name: &'static str,
        index: u32,
        variant: &'static str,
        len: usize,
    ) -> Result<Self::SerializeStructVariant, S::Error> {
        self.depth.enter()?;
        Ok(Counting {
            inner: self
                .inner
                .serialize_struct_variant(name, index, variant, len)?,
            depth: self.depth,
        })
    }

    /// The codec's own answer, which is the whole reason for wrapping rather
    /// than counting alongside: a `Serialize` that branches on this must be
    /// shown the shape that is going to be written.
    fn is_human_readable(&self) -> bool {
        self.inner.is_human_readable()
    }

    fn collect_str<T>(self, value: &T) -> Result<S::Ok, S::Error>
    where
        T: std::fmt::Display + ?Sized,
    {
        self.inner.collect_str(value)
    }
}

/// The compound halves, which differ only in the name of the method that takes
/// a value and whether it takes a key first.
macro_rules! compound {
    ($($trait_name:ident { $($method:ident($($arg:ident: $ty:ty),*)),+ $(,)? })+) => {
        $(impl<S: $trait_name> $trait_name for Counting<'_, S> {
            type Ok = S::Ok;
            type Error = S::Error;

            $(fn $method<T>(&mut self $(, $arg: $ty)*, value: &T) -> Result<(), S::Error>
            where
                T: Serialize + ?Sized,
            {
                let nested = Counted { value, depth: self.depth };
                self.inner.$method($($arg,)* &nested)
            })+

            fn end(self) -> Result<S::Ok, S::Error> {
                self.depth.leave();
                self.inner.end()
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

impl<S: SerializeMap> SerializeMap for Counting<'_, S> {
    type Ok = S::Ok;
    type Error = S::Error;

    fn serialize_key<T>(&mut self, key: &T) -> Result<(), S::Error>
    where
        T: Serialize + ?Sized,
    {
        let nested = Counted {
            value: key,
            depth: self.depth,
        };
        self.inner.serialize_key(&nested)
    }

    fn serialize_value<T>(&mut self, value: &T) -> Result<(), S::Error>
    where
        T: Serialize + ?Sized,
    {
        let nested = Counted {
            value,
            depth: self.depth,
        };
        self.inner.serialize_value(&nested)
    }

    fn end(self) -> Result<S::Ok, S::Error> {
        self.depth.leave();
        self.inner.end()
    }
}
