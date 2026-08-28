#[cfg(feature = "redb")]
pub mod redb;
#[cfg(feature = "sqlite")]
pub mod sqlite;
#[cfg(feature = "text")]
pub mod text;

#[cfg(not(feature = "bench-internals"))]
mod utils;

/// Reachable from a bench, which is an external crate and cannot see a private
/// module. Off by default: nothing here is API, and the feature says so.
#[cfg(feature = "bench-internals")]
pub mod utils;
