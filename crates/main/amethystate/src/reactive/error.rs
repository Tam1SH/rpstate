use crate::store::StorageError;
use thiserror::Error;

/// Anything that can go wrong writing through a reactive primitive.
///
/// Fields and maps fail identically, so they share one error; `FieldError` and
/// `ReactiveMapError` are aliases that keep call sites readable.
#[derive(Error, Debug)]
pub enum WriteError {
    #[error(transparent)]
    StorageError(#[from] StorageError),

    #[error("Change intercepted")]
    Intercepted,

    /// The addressed value is absent: a map key for maps, a store path for fields.
    #[error("Key not found: {0}")]
    KeyNotFound(String),

    /// A `Kv` write aimed at a path a declared struct owns.
    #[error("path `{path}` belongs to the schema at `{prefix}`")]
    SchemaOwned { path: String, prefix: String },

    /// The same path asked for as two different types in one run.
    #[error("path `{path}` is already `{known}`, asked for `{asked}`")]
    TypeMismatch {
        path: String,
        known: String,
        asked: String,
    },
}

pub type WriteResult<T> = std::result::Result<T, WriteError>;

pub type FieldError = WriteError;
pub type ReactiveMapError = WriteError;

pub type ReactiveFieldResult<T> = WriteResult<T>;
pub type ReactiveMapResult<T> = WriteResult<T>;

impl<E> From<amethystate_core::error::WriteError<E>> for WriteError
where
    StorageError: From<E>,
{
    fn from(value: amethystate_core::error::WriteError<E>) -> Self {
        use amethystate_core::error::WriteError as Core;

        match value {
            Core::StorageError(e) => WriteError::StorageError(StorageError::from(e)),
            Core::Intercepted => WriteError::Intercepted,
            Core::KeyNotFound(k) => WriteError::KeyNotFound(k),
        }
    }
}
