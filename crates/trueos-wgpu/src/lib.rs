//! A bounded, validated wgpu custom backend over TRUEOS tenant GPU handles.
//!
//! Unsupported operations fail explicitly. Native shader admission remains in
//! the kernel; applications receive no native addresses or command packets.

mod backend;

pub use backend::{Context, Error, ShaderPackage};
pub use wgpu;
