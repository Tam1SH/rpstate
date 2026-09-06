use crate::codec::CodecError;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum MigrationError {
    #[error(transparent)]
    Codec(#[from] CodecError),

    #[error(
        "Migration chain gap for [{prefix}]: reached v{reached_version}, expected v{expected_version}"
    )]
    Gap {
        prefix: String,
        reached_version: u32,
        expected_version: u32,
    },

    /// A step reached into a prefix whose own migration is already running, so
    /// neither can go first. The whole chain is named, outermost first, ending
    /// on the prefix that closed it.
    #[error("a migration reached round to where it started: {}", .0.join(" -> "))]
    Cycle(Vec<String>),

    #[error("Migration error: {0}")]
    Custom(String),

    #[error("Downgrade detected for [{prefix}]: DB v{db_version}, Code v{code_version}")]
    Downgrade {
        prefix: String,
        db_version: u32,
        code_version: u32,
    },
}
