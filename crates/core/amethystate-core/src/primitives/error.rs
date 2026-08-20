use thiserror::Error;

/// Anything that can go wrong writing through a reactive primitive.
///
/// Fields and maps fail in exactly the same ways, so they share one error. The
/// per-primitive names below are aliases kept for readability at call sites.
#[derive(Error, Debug)]
pub enum WriteError<TStorageError> {
    #[error(transparent)]
    StorageError(#[from] TStorageError),

    #[error("Change intercepted")]
    Intercepted,

    /// The addressed value is absent: a map key for maps, a store path for fields.
    #[error("Key not found: {0}")]
    KeyNotFound(String),

    /// A key that cannot name a level, so nothing can be stored under it.
    #[error(transparent)]
    Path(crate::path::StorePathError),
}

pub type WriteResult<T, E> = std::result::Result<T, WriteError<E>>;

pub type FieldError<TStorageError> = WriteError<TStorageError>;
pub type ReactiveMapError<TStorageError> = WriteError<TStorageError>;

pub type ReactiveFieldResult<T, E> = WriteResult<T, E>;
pub type ReactiveMapResult<T, E> = WriteResult<T, E>;
