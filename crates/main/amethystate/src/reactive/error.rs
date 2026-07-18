use crate::store::StorageError;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum FieldError {
    #[error(transparent)]
    StorageError(#[from] StorageError),

    #[error("Change intercepted")]
    Intercepted,

    #[error("Key not found in Field: {0}")]
    KeyNotFound(String),
}

pub type ReactiveFieldResult<T> = std::result::Result<T, FieldError>;

impl<E> From<amethystate_core::error::FieldError<E>> for FieldError
where
    StorageError: From<E>,
{
    fn from(value: amethystate_core::error::FieldError<E>) -> Self {
        match value {
            amethystate_core::error::FieldError::StorageError(e) => {
                FieldError::StorageError(StorageError::from(e))
            }
            amethystate_core::error::FieldError::Intercepted => FieldError::Intercepted,
            amethystate_core::error::FieldError::KeyNotFound(k) => FieldError::KeyNotFound(k),
        }
    }
}

#[derive(Error, Debug)]
pub enum ReactiveMapError {
    #[error(transparent)]
    StorageError(#[from] StorageError),

    #[error("Change intercepted")]
    Intercepted,

    #[error("Key not found in ReactiveMap: {0}")]
    KeyNotFound(String),
}

pub type ReactiveMapResult<T> = std::result::Result<T, ReactiveMapError>;

impl<E> From<amethystate_core::error::ReactiveMapError<E>> for ReactiveMapError
where
    StorageError: From<E>,
{
    fn from(value: amethystate_core::error::ReactiveMapError<E>) -> Self {
        match value {
            amethystate_core::error::ReactiveMapError::StorageError(e) => {
                ReactiveMapError::StorageError(StorageError::from(e))
            }
            amethystate_core::error::ReactiveMapError::Intercepted => ReactiveMapError::Intercepted,
            amethystate_core::error::ReactiveMapError::KeyNotFound(k) => {
                ReactiveMapError::KeyNotFound(k)
            }
        }
    }
}
