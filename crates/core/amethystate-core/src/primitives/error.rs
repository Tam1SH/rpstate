use thiserror::Error;

#[derive(Error, Debug)]
pub enum FieldError<TStorageError> {
    #[error(transparent)]
    StorageError(#[from] TStorageError),

    #[error("Change intercepted")]
    Intercepted,

    #[error("Key not found in Field: {0}")]
    KeyNotFound(String),
}

pub type ReactiveFieldResult<T, E> = std::result::Result<T, FieldError<E>>;

#[derive(Error, Debug)]
pub enum ReactiveMapError<TStorageError> {
    #[error(transparent)]
    StorageError(#[from] TStorageError),

    #[error("Change intercepted")]
    Intercepted,

    #[error("Key not found in ReactiveMap: {0}")]
    KeyNotFound(String),
}

pub type ReactiveMapResult<T, E> = std::result::Result<T, ReactiveMapError<E>>;
