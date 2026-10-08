//! Non-owning UI4 rendering targets for Winit and native GPU frontends.
//!
//! The frontend owns window lifetime and resize negotiation. These capabilities
//! write and publish the paired producers without creating input routes or
//! closing the window when dropped. Shared frame transport remains in UI4's
//! original Blueprint facade.

use crate::ui4_solara_text::Frame;
pub use crate::ui4_solara_text::{Damage, Error, SpriteCorner, SpriteQuad};

/// How a retained sprite command writes the producer's frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpriteBackend {
    /// GPU sprite sampling and source-over composition.
    Compositor,
    /// Source-over for assets already premultiplied by their producer.
    PremultipliedCompositor,
    /// Exact integer copies/fills on BCS0. Sprite sources must already be
    /// premultiplied; alpha-zero holes preserve existing foreground pixels.
    /// Partial alpha is preserved for the display engine's layer blend.
    Bcs0,
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SpriteCommand {
    pub quad: SpriteQuad,
    pub backend: SpriteBackend,
}

/// Non-owning rendering access to a window created by another frontend (for
/// example Winit). Dropping this target never closes the window. The kernel
/// validates ownership and window existence on each operation; after the
/// frontend closes the window, operations fail with `NotFound`.
///
/// Use the frontend's current physical dimensions. Complete a sprite draw
/// before replacing its retained source; draw calls wait for copy retirement.
/// Destination-buffer admission and publication remain protected by kernel
/// backpressure. Presentation receipts observe physical display progress;
/// they must not gate frontend input processing.
pub struct SceneTarget {
    surface: core::mem::ManuallyDrop<Frame>,
}

impl SceneTarget {
    pub fn for_window(window_id: u32, width: u32, height: u32) -> Result<Self, Error> {
        if window_id == 0 || width == 0 || height == 0 {
            return Err(Error::Invalid);
        }
        Ok(Self {
            surface: core::mem::ManuallyDrop::new(Frame {
                window_id,
                width,
                height,
            }),
        })
    }

    /// Render-target capability for a GPU scene producer. Background targets
    /// are not input window IDs and must not be passed to input APIs.
    pub fn render_target(&self) -> u32 {
        self.surface.window_id
    }

    /// Update the local extent after the owning window delivers a resize.
    /// This does not resize or acquire the kernel surface.
    pub fn set_extent(&mut self, width: u32, height: u32) -> Result<(), Error> {
        if width == 0 || height == 0 {
            return Err(Error::Invalid);
        }
        self.surface.width = width;
        self.surface.height = height;
        Ok(())
    }
    /// Borrow the paired scene producer. This never creates an input route.
    pub fn background(&self) -> Result<Self, Error> {
        let target =
            unsafe { v::bp_abi::trueos_cabi_ui4_scene_frame_layer_v1(self.surface.window_id, 1) };
        Self::for_window(target, self.surface.width, self.surface.height)
    }
    pub fn draw_sprite_commands(&mut self, commands: &[SpriteCommand]) -> Result<(), Error> {
        self.surface.draw_sprite_commands(commands)
    }
    pub fn upload_sprite_rgba8(
        &mut self,
        id: u32,
        width: u32,
        height: u32,
        pixels: &[u8],
    ) -> Result<(), Error> {
        self.surface.upload_sprite_rgba8(id, width, height, pixels)
    }
    pub fn begin_gpu_frame(&mut self) -> Result<(), Error> {
        self.surface.begin_gpu_frame()
    }
    /// Acquire and clear a foreground repaint region or background content viewport.
    /// See [`Frame::begin_gpu_frame_region`] for buffer coherence requirements.
    pub fn begin_gpu_frame_region(&mut self, damage: Damage) -> Result<(), Error> {
        self.surface.begin_gpu_frame_region(damage)
    }
    pub fn draw_sprite_quads(&mut self, quads: &[SpriteQuad]) -> Result<(), Error> {
        self.surface.draw_sprite_quads(quads)
    }
    /// Publish this producer's frame, including background updates and staged
    /// paired resize handoffs. Retry Busy before acquiring another write lease.
    pub fn publish(&mut self, damage: Damage) -> Result<(), Error> {
        self.surface.publish(damage)
    }
    /// Request a foreground-only presentation receipt. Background updates and
    /// staged paired resize handoffs must use [`Self::publish`] instead.
    pub fn publish_tracked(&mut self, damage: Damage) -> Result<u64, Error> {
        self.surface.publish_tracked(damage)
    }
    pub fn was_presented(&self, serial: u64) -> Result<bool, Error> {
        self.surface.was_presented(serial)
    }
}
