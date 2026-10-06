//! A bounded, validated wgpu custom backend over TRUEOS tenant GPU handles.
//!
//! The current slice supports admitted world-camera color and sampled-atlas
//! WGSL packages, triangle lists, an opaque RGBA8 UNORM leased frame, and a
//! Depth32Float attachment. Atlases use RGBA8 UNORM pixels with nearest clamp
//! sampling and revision uploads to retained GPU storage. Native vertex and
//! identity index buffers also persist; source revisions and draw layout changes
//! upload only affected geometry segments, independently of the camera uniform.
//! Device/queue bootstrap exposes a wgpu adapter over public custom dispatch.
//! Adapter enumeration and general surface creation are not provided.
//! Unsupported operations fail explicitly. Native shader admission remains in
//! the kernel, behind opaque handles. More shader and resource contracts can be
//! added here without exposing MMIO or native command packets to applications.

mod backend;

pub use backend::{Context, Error, ShaderPackage, adapter_with_package};
pub use wgpu;
