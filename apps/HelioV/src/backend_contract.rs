//! Compile-time witness for the real WGPU custom-backend seam.
//!
//! Keeping this in the application makes an accidental return to a hosted
//! Vulkan-only WGPU build fail at compile time. The implementation will live
//! in a reusable TRUEOS adapter crate, not in game or voxel code.

pub const REVISION: &str = "wgpu-30-custom/vmx-vgpu-v5-aot-indexed-resize-surflive";

pub fn custom_device_interface() -> &'static str {
    core::any::type_name::<dyn wgpu::custom::DeviceInterface>()
}
