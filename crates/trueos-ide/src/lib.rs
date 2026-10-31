//! Shared project source storage for TRUEOS development tools.
#![cfg_attr(not(feature = "std"), no_std)]

#[macro_use]
extern crate alloc;

pub mod protocol;
#[cfg(feature = "store")]
pub mod workspace;

#[cfg(feature = "store-core")]
pub mod source_db;
#[cfg(feature = "store-core")]
pub use source_db::*;
#[cfg(feature = "store-core")]
pub mod source_image;
#[cfg(feature = "store-core")]
pub mod store_protocol;

#[cfg(feature = "api-reference")]
pub mod api_reference;
