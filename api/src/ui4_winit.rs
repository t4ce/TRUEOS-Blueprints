//! Non-owning UI4 rendering targets for Winit and native GPU frontends.
//!
//! The frontend owns window lifetime and resize negotiation. These capabilities
//! write and publish the paired producers without creating input routes or
//! closing the window when dropped. Shared frame transport remains in UI4's
//! original Blueprint facade.

use crate::ui4_solara_text::Frame;
pub use crate::ui4_solara_text::{Damage, Error, SpriteCorner, SpriteQuad};

/// A simple Hull microfont run in physical foreground pixels. All fonts use
/// microfont; missing glyphs become red question marks. The service snaps size
/// down to the display's 0.25 cm ladder while preserving the SVG baseline and
/// whole-run advance. `rgba` is `u32::from_le_bytes([r, g, b, a])`.
#[derive(Clone, Copy, Debug)]
pub struct HullTextRun<'a> {
    pub text: &'a str,
    pub x: f32,
    pub baseline_y: f32,
    pub size_px: f32,
    /// Whole SVG textLength in pixels; zero selects the natural font advance.
    pub advance_px: f32,
    pub rgba: u32,
}

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

    /// Set the shared display background beneath transparent scene pixels.
    /// Uses the same helper as the UI4 color picker; no frame lease is needed.
    pub fn set_display_bottom_color(&mut self, rgb: [u8; 3]) -> Result<(), Error> {
        self.surface.set_display_bottom_color(rgb)
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
    /// Acquire a writable frame and clear it to a premultiplied RGBA color.
    /// Publish before acquiring another frame; this target never owns the window.
    pub fn begin(&mut self, clear_rgba: u32) -> Result<(), Error> {
        self.surface.begin(clear_rgba)
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
    /// Paint cached microfont coverage over the foreground's retired sprite
    /// draw. Call after drawing quads and before publishing the same frame.
    /// The kernel copies text and checks ownership and exact producer release.
    /// Large pages are transported in bounded batches; long UTF-8 runs retain
    /// their original whole-run spacing across batch boundaries.
    pub fn draw_hull_text_runs(&mut self, runs: &[HullTextRun<'_>]) -> Result<(), Error> {
        // Reject invalid input before writing any part of this page.
        for run in runs {
            if !run.x.is_finite()
                || !run.baseline_y.is_finite()
                || !run.size_px.is_finite()
                || !run.advance_px.is_finite()
                || run.x.abs() > 1_000_000.0
                || run.baseline_y.abs() > 1_000_000.0
                || run.size_px <= 0.0
                || run.size_px > 1_440.0
                || run.advance_px < 0.0
                || run.advance_px > 1_000_000.0
                || run.text.len() > u32::MAX as usize
            {
                return Err(Error::Invalid);
            }
        }
        let submit = |raw: &[v::bp_abi::TrueosUi4HullTextRunV1]| {
            crate::ui4_solara_text::status(unsafe {
                v::bp_abi::trueos_cabi_ui4_hull_text_runs_v1(
                    self.render_target(),
                    raw.as_ptr(),
                    raw.len(),
                )
            })
        };
        let mut raw = alloc::vec::Vec::with_capacity(runs.len().min(2_048));
        let mut batch_bytes = 0usize;
        for run in runs {
            let columns = run.text.chars().count();
            let mut byte_offset = 0usize;
            let mut column_offset = 0usize;
            while byte_offset < run.text.len() {
                let mut end = byte_offset.saturating_add(4_096).min(run.text.len());
                while !run.text.is_char_boundary(end) {
                    end -= 1;
                }
                let text = &run.text[byte_offset..end];
                let chunk_columns = text.chars().count();
                if raw.len() == 2_048 || batch_bytes + text.len() > 64 * 1024 {
                    submit(&raw)?;
                    raw.clear();
                    batch_bytes = 0;
                }
                let explicit = run.advance_px > 0.0;
                raw.push(v::bp_abi::TrueosUi4HullTextRunV1 {
                    text: text.as_ptr(),
                    text_len: text.len(),
                    x: if explicit {
                        run.x + run.advance_px * column_offset as f32 / columns as f32
                    } else {
                        run.x
                    },
                    baseline_y: run.baseline_y,
                    size_px: run.size_px,
                    advance_px: if explicit {
                        run.advance_px * chunk_columns as f32 / columns as f32
                    } else {
                        0.0
                    },
                    rgba: run.rgba,
                    column_offset: if explicit { 0 } else { column_offset as u32 },
                });
                batch_bytes += text.len();
                byte_offset = end;
                column_offset += chunk_columns;
            }
        }
        if !raw.is_empty() {
            submit(&raw)?;
        }
        Ok(())
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
