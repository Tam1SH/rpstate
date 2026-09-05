//! Why a read would not answer.

use crate::store::StorageError;
use amethystate_core::failure::one_line;
use amethystate_core::path::{StorePath, StorePathError};
use error_stack::Report;
use std::fmt;

/// What stopped a value from being read back.
#[derive(Debug)]
pub enum ReadValue {
    /// The levels handed in do not make a path.
    NotAPath(StorePathError),

    /// Something is stored there, and it is not the type that was asked for.
    ///
    /// `why` is kept whole because what the codec choked on - the type asked
    /// for, the bytes it found, how many of them - is attached to it.
    WillNotRead {
        at: StorePath,
        why: Report<StorageError>,
    },

    /// The store has let go of its file, so it answers nothing.
    Closed { at: StorePath },

    /// The disk, in every sense: the file, the engine, the codec.
    Store(Report<StorageError>),
}

impl ReadValue {
    /// What the store said, told apart where a caller would act on it
    /// differently.
    ///
    /// A codec's refusal is a frame *under* the operation's rather than the
    /// outermost one - a read that found the wrong type says `Read` on top and
    /// `Codec` below - so this asks
    /// [`will_not_read`](crate::store::will_not_read), which is the one place
    /// that difference is decided.
    pub fn from_store(at: &StorePath, why: Report<StorageError>) -> Self {
        if *why.current_context() == StorageError::Closed {
            return Self::Closed { at: at.clone() };
        }

        match crate::store::rules::will_not_read(&why) {
            true => Self::WillNotRead {
                at: at.clone(),
                why,
            },
            false => Self::Store(why),
        }
    }
}

impl fmt::Display for ReadValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotAPath(why) => write!(f, "the read was given no path to look at: {why}"),
            Self::WillNotRead { at, why } => {
                write!(
                    f,
                    "what is stored at {at} will not read back: {}",
                    one_line(why)
                )
            }
            Self::Closed { at } => write!(f, "the store was closed, so {at} was not read"),
            Self::Store(why) => write!(f, "{}", why.current_context()),
        }
    }
}

impl std::error::Error for ReadValue {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::NotAPath(why) => Some(why),
            Self::Store(why) | Self::WillNotRead { why, .. } => Some(why.current_context()),
            Self::Closed { .. } => None,
        }
    }
}

impl From<StorePathError> for ReadValue {
    fn from(why: StorePathError) -> Self {
        Self::NotAPath(why)
    }
}

impl From<Report<StorageError>> for ReadValue {
    fn from(why: Report<StorageError>) -> Self {
        Self::Store(why)
    }
}

impl From<ReadValue> for Report<StorageError> {
    fn from(why: ReadValue) -> Self {
        match why {
            ReadValue::Store(report) | ReadValue::WillNotRead { why: report, .. } => report,
            ReadValue::NotAPath(why) => Report::new(why).change_context(StorageError::Path),
            ReadValue::Closed { at } => {
                Report::new(StorageError::Closed).attach(amethystate_core::facts::Key(at))
            }
        }
    }
}

/// What stopped a listing from coming back.
#[derive(Debug)]
pub enum ScanKeys {
    /// The levels handed in do not make a prefix.
    NotAPath(StorePathError),

    /// A key on disk will not read back as a path.
    ///
    /// Reachable only where something other than this library wrote it, so
    /// `why` carries the key as it sits on disk - which is the only state it
    /// is in when the reason for the failure is that it is not a path.
    KeyWillNotRead {
        under: StorePath,
        why: Report<StorageError>,
    },

    /// The store has let go of its file, so there is nothing to list.
    Closed { under: StorePath },

    /// The disk, in every sense.
    Store(Report<StorageError>),
}

impl ScanKeys {
    /// What the store said, told apart where a caller would act on it
    /// differently.
    pub fn from_store(under: &StorePath, why: Report<StorageError>) -> Self {
        match *why.current_context() {
            StorageError::Path => Self::KeyWillNotRead {
                under: under.clone(),
                why,
            },
            StorageError::Closed => Self::Closed {
                under: under.clone(),
            },
            _ => Self::Store(why),
        }
    }
}

impl fmt::Display for ScanKeys {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotAPath(why) => write!(f, "the scan was given no prefix to list: {why}"),
            Self::KeyWillNotRead { under, why } => {
                write!(
                    f,
                    "a key stored under {under} will not read back as a path: {}",
                    one_line(why)
                )
            }
            Self::Closed { under } => {
                write!(f, "the store was closed, so {under} was not listed")
            }
            Self::Store(why) => write!(f, "{}", why.current_context()),
        }
    }
}

impl std::error::Error for ScanKeys {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::NotAPath(why) => Some(why),
            Self::Store(why) | Self::KeyWillNotRead { why, .. } => Some(why.current_context()),
            Self::Closed { .. } => None,
        }
    }
}

impl From<StorePathError> for ScanKeys {
    fn from(why: StorePathError) -> Self {
        Self::NotAPath(why)
    }
}

impl From<Report<StorageError>> for ScanKeys {
    fn from(why: Report<StorageError>) -> Self {
        Self::Store(why)
    }
}

impl From<ScanKeys> for Report<StorageError> {
    fn from(why: ScanKeys) -> Self {
        match why {
            ScanKeys::Store(report) | ScanKeys::KeyWillNotRead { why: report, .. } => report,
            ScanKeys::NotAPath(why) => Report::new(why).change_context(StorageError::Path),
            ScanKeys::Closed { under } => {
                Report::new(StorageError::Closed).attach(amethystate_core::facts::Prefix(under))
            }
        }
    }
}

pub type ReadResult<T> = Result<T, ReadValue>;
pub type ScanResult<T> = Result<T, ScanKeys>;
