//! Shared project source storage for TRUEOS development tools.
#![cfg_attr(not(feature = "std"), no_std)]

#[macro_use]
extern crate alloc;

pub mod models;
pub use models::*;
pub mod protocol;
pub mod html_preview;
pub mod workspace;

#[cfg(feature = "store-core")]
pub mod source_db;
#[cfg(feature = "store-core")]
pub use source_db::*;
#[cfg(feature = "store-core")]
pub mod source_image;
pub mod store_protocol;

#[cfg(feature = "api-reference")]
pub mod api_reference;
