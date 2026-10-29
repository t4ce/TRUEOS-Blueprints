//! Shared project source storage for TRUEOS development tools.

pub mod protocol;
#[cfg(feature = "store")]
pub mod workspace;

#[cfg(feature = "store")]
pub mod source_db;
#[cfg(feature = "store")]
pub use source_db::*;
