//! A bounded, validated wgpu custom backend over TRUEOS tenant GPU handles.
//!
//! The current slice supports the admitted position/color WGSL package, one
//! camera uniform, triangle lists, an opaque RGBA8 UNORM leased frame, and a
//! Depth32Float attachment. Device/queue bootstrap uses wgpu's public custom
//! dispatch; adapter enumeration and general surface creation are not provided.
//! Unsupported operations fail explicitly. Native shader admission remains in
//! the kernel, behind opaque handles. More shader and resource contracts can be
//! added here without exposing MMIO or native command packets to applications.

mod backend;

pub use backend::{Context, Error, ShaderPackage};
pub use wgpu;
