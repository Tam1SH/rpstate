use error_stack::Report;
use std::fmt;

/// What a store operation failed at.
///
/// A context, not a chain. Which engine said what is the frame below this one
/// in the report, and the particulars - which path, which file, which prefix -
/// are attachments put there by whoever knew them. So the variants name the
/// operation rather than the source: two engines failing to write are the same
/// kind of failure, told apart by the frames underneath.
#[derive(Debug, PartialEq, Eq)]
pub enum StorageError {
    /// Opening or creating the store.
    Open,

    /// Reading a value.
    Read,

    /// Writing a value.
    Write,

    /// Removing a key or a subtree.
    Delete,

    /// Listing what is under a prefix.
    Scan,

    /// Getting what is buffered onto disk.
    Flush,

    /// Turning a value into the store's format, or reading it back.
    Codec,

    /// Reading or writing the schema bookkeeping - versions, snapshots, the
    /// migration log, the initialization markers.
    Meta,

    /// Bringing stored data up to the schema the code declares.
    Migrate,

    /// A name that cannot be a level, so nothing can be stored under it.
    Path,

    /// The flush this commit was waiting on did not complete.
    CommitFailed,
}

impl fmt::Display for StorageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            StorageError::Open => "the store could not be opened",
            StorageError::Read => "the store could not read",
            StorageError::Write => "the store could not write",
            StorageError::Delete => "the store could not delete",
            StorageError::Scan => "the store could not list a prefix",
            StorageError::Flush => "the store could not commit what it had buffered",
            StorageError::Codec => "the value could not be encoded or decoded",
            StorageError::Meta => "the schema bookkeeping could not be read or written",
            StorageError::Migrate => "the data could not be brought up to the declared schema",
            StorageError::Path => "a name that cannot be a level",
            StorageError::CommitFailed => "the flush this commit was waiting on did not complete",
        })
    }
}

impl std::error::Error for StorageError {}

pub type StorageResult<T> = Result<T, Report<StorageError>>;

/// What a migration step returns when it decides to fail.
///
/// A step is written by whoever uses the library, and the frames around it -
/// which prefix, which version, which store - are put there by the engine that
/// called it. So a step has nothing to add and only needs to say that this is
/// its refusal rather than the store's.
pub trait IntoStorageReport {
    fn into_report(self) -> Report<StorageError>;
}

impl IntoStorageReport for crate::MigrationError {
    fn into_report(self) -> Report<StorageError> {
        Report::new(self).change_context(StorageError::Migrate)
    }
}

impl IntoStorageReport for amethystate_core::path::StorePathError {
    fn into_report(self) -> Report<StorageError> {
        Report::new(self).change_context(StorageError::Path)
    }
}
