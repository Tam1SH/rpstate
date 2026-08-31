//! How deep a value is, learned while the codec writes it.
//!
//! Every codec here reads less deeply than it writes. `serde_json` stops at
//! 128 on the way in and has no limit on the way out; `ron` stops at 64;
//! `rmp_serde` has no limit at all and the stack runs out around three
//! thousand instead, which kills the process rather than returning an error -
//! and does so on every later start, because the value is already committed.
//!
//! So a write past the reader's ceiling is accepted and cannot be read back,
//! which is the worst shape a defect can have: no error anywhere, and the file
//! is gone.
//!
//! By the time a value reaches a store it is a `&dyn erased_serde::Serialize`,
//! so the depth has to be learned from the write itself: building the value out
//! to walk it is the dangerous act, and on redb it is what overflows the stack.
//!
//! Serde is a push protocol and the store is on the receiving end, so it counts
//! what goes past during the codec's own pass, through [`Counting`].
//! `serde_json` does the same thing on the read side.

mod counting;

pub use counting::{Counted, Counting, Depth};

use crate::store::builder::Backend;
use crate::store::config::WriteLimits;
use crate::store::{CodecFormat, StorageError, StorageResult};
use amethystate_core::path::StorePath;
use error_stack::Report;

/// What one store may spend, worked out once when it opens.
///
/// The ceiling is the running codec's, lowered by anything the store promised
/// to stay readable on. `key_depth` is the store's own cap on paths, which is a
/// setting rather than a fact.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DepthBudget {
    pub ceiling: usize,
    pub key_depth: Option<usize>,

    /// Whether a `NaN` or an infinity survives on the running engine and on
    /// every engine this store promised to stay readable on.
    pub non_finite_floats: bool,

    /// The same for an enum of any shape.
    pub enums: bool,
}

impl DepthBudget {
    /// The budget for a store running on `engine` under `limits`.
    pub fn resolve(limits: &WriteLimits, engine: Backend) -> Self {
        Self {
            ceiling: limits.ceiling(engine),
            key_depth: limits.key_depth,
            non_finite_floats: limits.holds_non_finite_floats(engine),
            enums: limits.holds_enums(engine),
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
                    non_finite_floats: true,
                    enums: true,
                };
            }
        };
        Self::resolve(limits, engine)
    }

    /// Whether a path is within the store's own cap on how deep a key may go.
    pub fn check_path(&self, path: &StorePath) -> StorageResult<()> {
        let levels = path.segments().count();

        if let Some(cap) = self.key_depth
            && levels > cap
        {
            return Err(Report::new(StorageError::Depth)
                .attach(format!("path: {path}"))
                .attach(format!("levels: {levels}, and the limit is {cap}"))
                .attach("set by: limits(|l| l.key_depth(..))")
                .attach(format!(
                    "what is stored here spends the same budget - this store reads {} levels in all",
                    self.ceiling
                )));
        }

        Ok(())
    }

    /// What a value at `path` has left to spend, to be carried through the
    /// codec's own pass.
    ///
    /// The path is counted with the value because the budget is shared: on
    /// every text engine the path's levels become the document's, so a shallow
    /// value at a deep path is exactly as unreadable as a deep value at a
    /// shallow one. The flat engines keep the path as one key - `&str` on redb,
    /// `TEXT` on sqlite - and pay for it here anyway, which costs a handful of
    /// levels out of 512 and 254 and saves a second rule.
    pub fn for_value(&self, path: &StorePath) -> Depth {
        Depth::new(self.ceiling.saturating_sub(path.segments().count()))
    }

    /// Says what went wrong, once a codec's error turns out to have been the
    /// count's.
    ///
    /// A `Serializer` may only return its own error type, so the refusal
    /// reaches the caller wearing the codec's clothes and cannot be recognised
    /// by its type - [`Depth::overflowed`] is how the caller asks whether it
    /// was this.
    pub fn too_deep(&self, path: &StorePath) -> Report<StorageError> {
        let levels = path.segments().count();
        let left = self.ceiling.saturating_sub(levels);

        Report::new(StorageError::Codec)
            .attach(format!("path: {path}"))
            .attach(format!(
                "the path spends {levels} levels and the value goes past the {left} that are left"
            ))
            .attach(format!("this store reads at most {} levels", self.ceiling))
            .attach(
                "a value deeper than the reader accepts is written without complaint and \
                 cannot be read back",
            )
    }

    /// Whether a pass that has finished wrote something this store cannot read
    /// back, or cannot promise elsewhere.
    ///
    /// Asked after a *successful* write rather than after a failed one: a codec
    /// with no spelling for a `NaN` writes `null` and reports success, so there
    /// is no error to inspect. That is the whole reason the value has to be
    /// refused here - left alone it lands as `null`, the write says `Ok`, and
    /// the field goes on reporting the number it held before while the file
    /// holds nothing of the sort.
    pub fn refused(&self, depth: &Depth, path: &StorePath) -> Option<Report<StorageError>> {
        if !self.non_finite_floats && depth.saw_a_non_finite_float() {
            return Some(
                Report::new(StorageError::Codec)
                    .attach(format!("path: {path}"))
                    .attach("a NaN or an infinity, which this store cannot read back")
                    .attach(
                        "JSON has no spelling for either, so the codec writes `null` and \
                         decoding it as a float fails - on json, and on sqlite, which encodes \
                         with the same JSON",
                    ),
            );
        }

        if !self.enums && depth.saw_an_enum() {
            return Some(
                Report::new(StorageError::Codec)
                    .attach(format!("path: {path}"))
                    .attach("an enum, which this store cannot read back")
                    .attach(
                        "ron writes one as `On(3)` and parses it back into a `ron::value::Value`, \
                         which has no variant to put it in - the name is dropped there and the \
                         next read is handed a sequence. See https://github.com/ron-rs/ron/issues/140",
                    ),
            );
        }

        None
    }
}
