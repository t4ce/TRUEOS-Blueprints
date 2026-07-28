//! Safe Blueprint facade for the experimental Solara text-row UI4 ABI.

use alloc::vec::Vec;

pub const MAX_SCENE_TEXT_ROWS_PER_CALL: usize = 64;
const FONT_ID_STAMP_ONCE: u32 = 1 << 31;
const FONT_ID_TEXT_BACKBUFFER: u32 = 1 << 30;
const TEXT_BACKBUFFER_SPRITE_ID: u32 = u32::MAX;

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct FontSize {
    pub native_scale: u32,
    pub target_pixels: u32,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum Font {
    Default = 1,
    NotoSansSc = 2,
    Inconsolata = 3,
}

#[derive(Copy, Clone, Debug, PartialEq)]
pub struct TextRow<'a> {
    pub text: &'a str,
    pub x: f32,
    pub y: f32,
}

/// One Solara paint record in fixed viewport coordinates.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct SceneTextRow<'a> {
    pub text: &'a str,
    pub x: f32,
    pub y: f32,
    pub font_pixels: f32,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct Damage {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct CursorSource {
    pub controller_id: u32,
    pub slot_id: u32,
    pub ep_target: u32,
    pub hid_kind: u32,
}

/// Return the physical UI4/cursor output extent in pixels.
pub fn output_dimensions() -> Result<(u32, u32), Error> {
    let packed = unsafe { v::bp_abi::trueos_cabi_ui4_scene_output_dimensions() };
    let width = (packed >> 32) as u32;
    let height = packed as u32;
    if width == 0 || height == 0 {
        Err(Error::Ui4)
    } else {
        Ok((width, height))
    }
}

/// Kernel-provided slot-4 cursor sprites for a UI4 frame.
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
#[repr(u32)]
pub enum CursorIcon {
    #[default]
    Default = 0,
    Loading = 1,
    ResizeHorizontal = 2,
    ResizeVertical = 3,
    ResizeDiagonal = 4,
    /// The selected frame paints its own cursor pixels.
    AppOwned = 5,
}

/// One selected-frame pointer event after UI4 hit testing and capture.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct PointerEvent {
    pub source: CursorSource,
    pub x: u32,
    pub y: u32,
    pub local_x: i32,
    pub local_y: i32,
    pub dx: i32,
    pub dy: i32,
    pub wheel: i16,
    pub buttons_down: u32,
    pub buttons_pressed: u32,
    pub buttons_released: u32,
    pub combo_id: u32,
    pub vcursor: bool,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum PanPhase {
    Begin,
    Update,
    End,
}

/// One UI4 middle-button gesture already hit-tested and captured to this frame.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct PanEvent {
    pub source: CursorSource,
    pub phase: PanPhase,
    pub x: u32,
    pub y: u32,
    pub local_x: i32,
    pub local_y: i32,
    pub dx: i32,
    pub dy: i32,
    pub combo_id: u32,
    pub vcursor: bool,
}

/// A UI4 maximize/restore request for this frame's backing extent.
///
/// The frame remains valid at its current size until the Blueprint chooses to
/// call [`Frame::resize`]. Ignoring this event keeps the old pixels centered
/// 1:1 inside the broker-owned maximize geometry.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct ResizeEvent {
    pub old_width: u32,
    pub old_height: u32,
    pub width: u32,
    pub height: u32,
}

/// Held HID usages for the keyboard routed to this selected UI4 frame.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct KeyboardState {
    pub controller_id: u32,
    pub slot_id: u32,
    pub ep_target: u32,
    pub combo_id: u32,
    pub modifiers: u8,
    pub source_kind: u8,
    pub virtual_keyboard: bool,
    pub keys: [u8; 6],
    pub ascii: [u8; 6],
    pub key_down_bits: [u32; 8],
}

impl KeyboardState {
    pub fn is_down(&self, hid_usage: u8) -> bool {
        let key = hid_usage as usize;
        self.key_down_bits[key / 32] & (1u32 << (key % 32)) != 0
    }
}

/// Camera basis and destination rectangle for the kernel RGB565 skybox
/// sampler. The source image is retained by the frame after upload.
#[derive(Copy, Clone, Debug, Default, PartialEq)]
pub struct SkyboxRenderParams {
    pub right_x: f32,
    pub right_y: f32,
    pub right_z: f32,
    pub up_x: f32,
    pub up_y: f32,
    pub up_z: f32,
    pub forward_x: f32,
    pub forward_y: f32,
    pub forward_z: f32,
    pub aspect_tan_half_fov_y: f32,
    pub tan_half_fov_y: f32,
    pub rect_x: u32,
    pub rect_y: u32,
    pub rect_width: u32,
    pub rect_height: u32,
}

pub const PARTICLE_CRAFT_WIDTH: u32 = 640;
pub const PARTICLE_CRAFT_HEIGHT: u32 = 400;
pub const PARTICLE_CRAFT_PARAMS_VERSION: u32 = 1;
pub const PARTICLE_CRAFT_FLAG_RESET: u32 = 1 << 0;
pub const PARTICLE_CRAFT_FLAG_ATTRACTOR: u32 = 1 << 1;
pub const PARTICLE_CRAFT_FLAG_ORBIT: u32 = 1 << 2;
pub const PARTICLE_CRAFT_MAX_PARTICLES: u32 = 256;

/// Pointer-free ParticleCraft v1 controls. Persistent particle state and GPU
/// addresses are retained by the kernel for this frame and never cross ABI.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct ParticleCraftParamsV1 {
    pub flags: u32,
    pub seed: u32,
    pub active_count: u32,
    pub dt_seconds: f32,
    pub time_seconds: f32,
    pub emitter_x: f32,
    pub emitter_y: f32,
    pub attractor_x: f32,
    pub attractor_y: f32,
    pub attraction: f32,
    pub swirl: f32,
    pub gravity_x: f32,
    pub gravity_y: f32,
    pub drag: f32,
    pub intensity: f32,
}

impl ParticleCraftParamsV1 {
    /// The Arc Forge preset used by both the Blueprint app and `cpp particle`.
    pub const fn arc_forge(time_seconds: f32, dt_seconds: f32, seed: u32) -> Self {
        Self {
            flags: PARTICLE_CRAFT_FLAG_ORBIT,
            seed,
            active_count: 128,
            dt_seconds,
            time_seconds,
            emitter_x: 320.0,
            emitter_y: 300.0,
            attractor_x: 320.0,
            attractor_y: 180.0,
            attraction: 94.0,
            swirl: 72.0,
            gravity_x: 0.0,
            gravity_y: 58.0,
            drag: 0.42,
            intensity: 1.0,
        }
    }
}

#[derive(Copy, Clone, Debug, Default, PartialEq)]
pub struct SpriteCorner {
    pub x: f32,
    pub y: f32,
    pub u: f32,
    pub v: f32,
}

/// One ordered draw from a frame-retained straight-alpha RGBA sprite. Sprite
/// id zero is the frame-owned white pixel and is useful for solid rectangles.
#[derive(Copy, Clone, Debug, Default, PartialEq)]
pub struct SpriteQuad {
    pub sprite_id: u32,
    pub c0: SpriteCorner,
    pub c1: SpriteCorner,
    pub c2: SpriteCorner,
    pub c3: SpriteCorner,
    pub color_rgba: u32,
    pub source_over: bool,
}

impl Damage {
    pub const fn full(width: u32, height: u32) -> Self {
        Self {
            x: 0,
            y: 0,
            width,
            height,
        }
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum Error {
    Invalid,
    NoBlueprintContext,
    NotFound,
    InvalidState,
    Font,
    Ui4,
    Busy,
    Unknown(i32),
}

/// Optional UI4 work requested at the coherent frame/session teardown point.
/// The default performs no capture or filesystem I/O.
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub struct CloseRequest {
    persist_final_frame: bool,
}

impl CloseRequest {
    /// Save the last published frame as `trueosfs:/finalframes/<app>.png`.
    /// A later close of the same app replaces the previous image.
    pub const fn persist_final_frame(mut self) -> Self {
        self.persist_final_frame = true;
        self
    }

    const fn flags(self) -> u32 {
        if self.persist_final_frame { 1 } else { 0 }
    }
}

pub struct Frame {
    window_id: u32,
    width: u32,
    height: u32,
}

impl Frame {
    pub fn open(x: i32, y: i32, width: u32, height: u32) -> Result<Self, Error> {
        let window_id =
            unsafe { v::bp_abi::trueos_cabi_ui4_solara_frame_open(x, y, width, height) };
        if window_id == 0 {
            Err(Error::Ui4)
        } else {
            Ok(Self {
                window_id,
                width,
                height,
            })
        }
    }

    /// Open a one-buffer snapshot frame.
    ///
    /// Each successful publish makes that allocation immutable. Calling
    /// `begin` again prepares a new one-buffer generation privately; the
    /// kernel swaps it into the same window only after the next publish.
    pub fn open_immutable(x: i32, y: i32, width: u32, height: u32) -> Result<Self, Error> {
        let window_id =
            unsafe { v::bp_abi::trueos_cabi_ui4_scene_frame_open_immutable(x, y, width, height) };
        if window_id == 0 {
            Err(Error::Ui4)
        } else {
            Ok(Self {
                window_id,
                width,
                height,
            })
        }
    }

    /// Open a triple-buffered scene frame for continuously shaded content.
    pub fn open_streaming(x: i32, y: i32, width: u32, height: u32) -> Result<Self, Error> {
        let window_id =
            unsafe { v::bp_abi::trueos_cabi_ui4_scene_frame_open_streaming(x, y, width, height) };
        if window_id == 0 {
            Err(Error::Ui4)
        } else {
            Ok(Self {
                window_id,
                width,
                height,
            })
        }
    }

    pub const fn window_id(&self) -> u32 {
        self.window_id
    }

    pub const fn width(&self) -> u32 {
        self.width
    }

    pub const fn height(&self) -> u32 {
        self.height
    }

    pub fn begin(&mut self, clear_rgba: u32) -> Result<(), Error> {
        status(unsafe { v::bp_abi::trueos_cabi_ui4_solara_frame_begin(self.window_id, clear_rgba) })
    }

    /// Take the next selected-frame pointer event. The click which selected
    /// this frame is absorbed by UI4 and is never returned here.
    pub fn take_pointer_event(&mut self) -> Result<Option<PointerEvent>, Error> {
        let mut raw = v::bp_abi::TrueosUi4PointerEvent::default();
        let result = unsafe {
            v::bp_abi::trueos_cabi_ui4_scene_pointer_event_take(self.window_id, &mut raw)
        };
        if result == 1 {
            return Ok(None);
        }
        if result != 0 {
            return Err(error_from_status(result));
        }
        let Ok(wheel) = i16::try_from(raw.wheel) else {
            return Err(Error::Invalid);
        };
        Ok(Some(PointerEvent {
            source: CursorSource {
                controller_id: raw.controller_id,
                slot_id: raw.slot_id,
                ep_target: raw.ep_target,
                hid_kind: raw.hid_kind,
            },
            x: raw.x,
            y: raw.y,
            local_x: raw.local_x,
            local_y: raw.local_y,
            dx: raw.dx,
            dy: raw.dy,
            wheel,
            buttons_down: raw.buttons_down,
            buttons_pressed: raw.buttons_pressed,
            buttons_released: raw.buttons_released,
            combo_id: raw.combo_id,
            vcursor: raw.vcursor != 0,
        }))
    }

    /// Take the next app-owned middle-button pan event for this frame.
    pub fn take_pan_event(&mut self) -> Result<Option<PanEvent>, Error> {
        let mut raw = v::bp_abi::TrueosUi4PanEvent::default();
        let result =
            unsafe { v::bp_abi::trueos_cabi_ui4_scene_pan_event_take(self.window_id, &mut raw) };
        if result == 1 {
            return Ok(None);
        }
        if result != 0 {
            return Err(error_from_status(result));
        }
        let phase = match raw.phase {
            1 => PanPhase::Begin,
            2 => PanPhase::Update,
            3 => PanPhase::End,
            _ => return Err(Error::Invalid),
        };
        Ok(Some(PanEvent {
            source: CursorSource {
                controller_id: raw.controller_id,
                slot_id: raw.slot_id,
                ep_target: raw.ep_target,
                hid_kind: raw.hid_kind,
            },
            phase,
            x: raw.x,
            y: raw.y,
            local_x: raw.local_x,
            local_y: raw.local_y,
            dx: raw.dx,
            dy: raw.dy,
            combo_id: raw.combo_id,
            vcursor: raw.vcursor != 0,
        }))
    }

    /// Take the next UI4 maximize/restore extent request for this frame.
    pub fn take_resize_event(&mut self) -> Result<Option<ResizeEvent>, Error> {
        let mut raw = v::bp_abi::TrueosUi4ResizeEvent::default();
        let result =
            unsafe { v::bp_abi::trueos_cabi_ui4_scene_resize_event_take(self.window_id, &mut raw) };
        if result == 1 {
            return Ok(None);
        }
        if result != 0 {
            return Err(error_from_status(result));
        }
        Ok(Some(ResizeEvent {
            old_width: raw.old_width,
            old_height: raw.old_height,
            width: raw.width,
            height: raw.height,
        }))
    }

    /// Take the one-shot event proving that this window's first published
    /// frame crossed the compositor's physical SURFLIVE handoff.
    pub fn take_first_presentation(&mut self) -> Result<bool, Error> {
        let result =
            unsafe { v::bp_abi::trueos_cabi_ui4_scene_first_presentation_take(self.window_id) };
        match result {
            0 => Ok(true),
            1 => Ok(false),
            _ => Err(error_from_status(result)),
        }
    }

    /// Sample held keys only from the keyboard routed to this selected frame.
    /// Click/tap the frame first to establish the global UI4 selection.
    pub fn keyboard_state(&self) -> Result<Option<KeyboardState>, Error> {
        let mut raw = v::bp_abi::TrueosUi4KeyboardState::default();
        let result =
            unsafe { v::bp_abi::trueos_cabi_ui4_scene_keyboard_state(self.window_id, &mut raw) };
        if result == 1 {
            return Ok(None);
        }
        if result != 0 {
            return Err(error_from_status(result));
        }
        Ok(Some(KeyboardState {
            controller_id: raw.controller_id,
            slot_id: raw.slot_id,
            ep_target: raw.ep_target,
            combo_id: raw.combo_id,
            modifiers: raw.modifiers,
            source_kind: raw.source_kind,
            virtual_keyboard: raw.virtual_keyboard != 0,
            keys: raw.keys,
            ascii: raw.ascii,
            key_down_bits: raw.key_down_bits,
        }))
    }

    pub fn set_position(&mut self, x: i32, y: i32) -> Result<(), Error> {
        status(unsafe { v::bp_abi::trueos_cabi_ui4_scene_frame_set_position(self.window_id, x, y) })
    }

    /// Compatibility shorthand for a frame-wide [`CursorIcon::AppOwned`].
    pub fn set_custom_cursor(&mut self, enabled: bool) -> Result<(), Error> {
        status(unsafe {
            v::bp_abi::trueos_cabi_ui4_scene_set_custom_cursor(self.window_id, u32::from(enabled))
        })
    }

    /// Set the selected frame's fallback cursor for every source which does
    /// not have its own override.
    pub fn set_cursor_icon(&mut self, icon: CursorIcon) -> Result<(), Error> {
        status(unsafe {
            v::bp_abi::trueos_cabi_ui4_scene_set_cursor_icon(
                self.window_id,
                core::ptr::null(),
                icon as u32,
            )
        })
    }

    /// Override the cursor sprite for one of UI4's independent cursor routes.
    pub fn set_cursor_icon_for(
        &mut self,
        source: CursorSource,
        icon: CursorIcon,
    ) -> Result<(), Error> {
        let source = v::bp_abi::TrueosUi4CursorSource {
            controller_id: source.controller_id,
            slot_id: source.slot_id,
            ep_target: source.ep_target,
            hid_kind: source.hid_kind,
        };
        status(unsafe {
            v::bp_abi::trueos_cabi_ui4_scene_set_cursor_icon(self.window_id, &source, icon as u32)
        })
    }

    pub fn resize(&mut self, width: u32, height: u32) -> Result<(), Error> {
        status(unsafe {
            v::bp_abi::trueos_cabi_ui4_scene_frame_resize(self.window_id, width, height)
        })?;
        self.width = width;
        self.height = height;
        Ok(())
    }

    /// Copy a tightly packed full-frame RGBA8 image. Every input pixel must be
    /// opaque so it already satisfies UI4's premultiplied-alpha contract.
    pub fn write_opaque_rgba8(&mut self, rgba: &[u8]) -> Result<(), Error> {
        let expected = self.width as usize * self.height as usize * 4;
        if rgba.len() != expected || rgba.chunks_exact(4).any(|pixel| pixel[3] != u8::MAX) {
            return Err(Error::Invalid);
        }
        status(unsafe {
            v::bp_abi::trueos_cabi_ui4_scene_frame_write_opaque_rgba8(
                self.window_id,
                rgba.as_ptr(),
                rgba.len(),
            )
        })
    }

    /// Retain one decoded RGBA sprite in the UI4 frame's warm source set.
    /// Upload is normally performed once; later frames submit only quad data.
    pub fn upload_sprite_rgba8(
        &mut self,
        sprite_id: u32,
        width: u32,
        height: u32,
        rgba: &[u8],
    ) -> Result<(), Error> {
        let Some(expected) = (width as usize)
            .checked_mul(height as usize)
            .and_then(|pixels| pixels.checked_mul(4))
        else {
            return Err(Error::Invalid);
        };
        if sprite_id == 0 || rgba.len() != expected {
            return Err(Error::Invalid);
        }
        status(unsafe {
            v::bp_abi::trueos_cabi_ui4_scene_sprite_upload_rgba8(
                self.window_id,
                sprite_id,
                width,
                height,
                rgba.as_ptr(),
                rgba.len(),
            )
        })
    }

    /// Acquire a back buffer whose opaque clear is performed by the first GPU
    /// sprite batch rather than by a CPU full-frame paint.
    pub fn begin_sprite_frame(&mut self, clear_rgba: u32) -> Result<(), Error> {
        status(unsafe {
            v::bp_abi::trueos_cabi_ui4_scene_sprite_frame_begin(self.window_id, clear_rgba)
        })
    }

    /// Acquire a back buffer for a full-frame GPU producer. No CPU clear is
    /// performed; the producer must overwrite the complete frame.
    pub fn begin_gpu_frame(&mut self) -> Result<(), Error> {
        status(unsafe { v::bp_abi::trueos_cabi_ui4_scene_sprite_frame_begin(self.window_id, 0) })
    }

    /// Render ordered retained sprites and solid rectangles into the active
    /// sprite-frame lease. Scenes larger than one hardware worklist are split
    /// by the kernel while preserving order.
    pub fn draw_sprite_quads(&mut self, quads: &[SpriteQuad]) -> Result<(), Error> {
        let raw = quads
            .iter()
            .map(|quad| v::bp_abi::TrueosUi4SpriteQuad {
                sprite_id: quad.sprite_id,
                c0_x: quad.c0.x,
                c0_y: quad.c0.y,
                c0_u: quad.c0.u,
                c0_v: quad.c0.v,
                c1_x: quad.c1.x,
                c1_y: quad.c1.y,
                c1_u: quad.c1.u,
                c1_v: quad.c1.v,
                c2_x: quad.c2.x,
                c2_y: quad.c2.y,
                c2_u: quad.c2.u,
                c2_v: quad.c2.v,
                c3_x: quad.c3.x,
                c3_y: quad.c3.y,
                c3_u: quad.c3.u,
                c3_v: quad.c3.v,
                color_rgba: quad.color_rgba,
                flags: u32::from(quad.source_over),
            })
            .collect::<Vec<_>>();
        status(unsafe {
            v::bp_abi::trueos_cabi_ui4_scene_sprite_quads(self.window_id, raw.as_ptr(), raw.len())
        })
    }

    /// Retain one tightly packed RGB565 equirectangular source for shaded
    /// rendering into this frame's back buffers.
    pub fn upload_skybox_rgb565(
        &mut self,
        width: u32,
        height: u32,
        rgb565: &[u8],
    ) -> Result<(), Error> {
        let Some(expected) = (width as usize)
            .checked_mul(height as usize)
            .and_then(|pixels| pixels.checked_mul(2))
        else {
            return Err(Error::Invalid);
        };
        if rgb565.len() != expected {
            return Err(Error::Invalid);
        }
        status(unsafe {
            v::bp_abi::trueos_cabi_ui4_scene_skybox_upload_rgb565(
                self.window_id,
                width,
                height,
                rgb565.as_ptr(),
                rgb565.len(),
            )
        })
    }

    /// Shade the retained skybox into the currently acquired UI4 back buffer.
    pub fn render_skybox_rgb565(&mut self, params: &SkyboxRenderParams) -> Result<(), Error> {
        let raw = v::bp_abi::TrueosUi4SkyboxRenderParams {
            right_x: params.right_x,
            right_y: params.right_y,
            right_z: params.right_z,
            up_x: params.up_x,
            up_y: params.up_y,
            up_z: params.up_z,
            forward_x: params.forward_x,
            forward_y: params.forward_y,
            forward_z: params.forward_z,
            aspect_tan_half_fov_y: params.aspect_tan_half_fov_y,
            tan_half_fov_y: params.tan_half_fov_y,
            rect_x: params.rect_x,
            rect_y: params.rect_y,
            rect_width: params.rect_width,
            rect_height: params.rect_height,
        };
        status(unsafe {
            v::bp_abi::trueos_cabi_ui4_scene_skybox_render_rgb565(self.window_id, &raw)
        })
    }

    /// Advance the retained particle state and shade the complete current frame.
    ///
    /// Particle simulation and gather cost remain fixed at the native 640x400
    /// logical extent; a resized frame is covered from that retained result.
    pub fn render_particle_craft(&mut self, params: &ParticleCraftParamsV1) -> Result<(), Error> {
        if params.active_count == 0 || params.active_count > PARTICLE_CRAFT_MAX_PARTICLES {
            return Err(Error::Invalid);
        }
        let raw = v::bp_abi::TrueosUi4ParticleCraftParamsV1 {
            version: PARTICLE_CRAFT_PARAMS_VERSION,
            flags: params.flags,
            seed: params.seed,
            active_count: params.active_count,
            dt_seconds: params.dt_seconds,
            time_seconds: params.time_seconds,
            emitter_x: params.emitter_x,
            emitter_y: params.emitter_y,
            attractor_x: params.attractor_x,
            attractor_y: params.attractor_y,
            attraction: params.attraction,
            swirl: params.swirl,
            gravity_x: params.gravity_x,
            gravity_y: params.gravity_y,
            drag: params.drag,
            intensity: params.intensity,
        };
        status(unsafe {
            v::bp_abi::trueos_cabi_ui4_scene_particle_craft_render(self.window_id, &raw)
        })
    }

    pub fn draw_text_rows(
        &mut self,
        font: Font,
        native_scale: u32,
        destination: (i32, i32),
        color_rgba: u32,
        rows: &[TextRow<'_>],
    ) -> Result<(), Error> {
        let raw: Vec<_> = rows
            .iter()
            .map(|row| v::bp_abi::TrueosUi4SolaraTextRow {
                text_ptr: row.text.as_ptr(),
                text_len: row.text.len(),
                x: row.x,
                y: row.y,
            })
            .collect();
        status(unsafe {
            v::bp_abi::trueos_cabi_ui4_solara_text_rows(
                self.window_id,
                font as u32,
                native_scale,
                destination.0,
                destination.1,
                color_rgba,
                raw.as_ptr(),
                raw.len(),
            )
        })
    }

    /// Retain paint records without fitting their collective bounds to a stamp.
    ///
    /// The kernel builds analytical coverage on the asynchronous FontKernel
    /// task and keeps it GPU-VM resident behind this frame. Later paint passes
    /// reuse the same masks when only color or a common integral translation
    /// changed. The destination must be opened with [`Frame::open_streaming`]
    /// so the final compute release can cross UI4's triple-buffered scene
    /// handoff. Consumers should split large scenes into bounded calls.
    pub fn retain_text_scene(
        &mut self,
        font: Font,
        viewport: (u32, u32),
        color_rgba: u32,
        rows: &[SceneTextRow<'_>],
    ) -> Result<(), Error> {
        if viewport != (self.width, self.height) {
            return Err(Error::Invalid);
        }
        let raw: Vec<_> = rows
            .iter()
            .map(|row| v::bp_abi::TrueosUi4SolaraSceneTextRow {
                text_ptr: row.text.as_ptr(),
                text_len: row.text.len(),
                x: row.x,
                y: row.y,
                font_pixels: row.font_pixels,
            })
            .collect();
        status(unsafe {
            v::bp_abi::trueos_cabi_ui4_solara_text_scene(
                self.window_id,
                font as u32,
                viewport.0,
                viewport.1,
                color_rgba,
                raw.as_ptr(),
                raw.len(),
            )
        })
    }

    /// Retain document text for one persistent, offscreen RGBA8 canvas.
    ///
    /// Call this one or more times between [`Frame::begin_sprite_frame`] and
    /// [`Frame::publish_text_backbuffer_view`]. The kernel retains analytical
    /// glyph coverage for every layer, materializes the complete canvas once,
    /// and then releases the masks. Subsequent viewport pans sample the warm
    /// RGBA8 canvas without rebuilding text or entering FontKernel again.
    pub fn retain_text_backbuffer(
        &mut self,
        font: Font,
        canvas: (u32, u32),
        color_rgba: u32,
        rows: &[SceneTextRow<'_>],
    ) -> Result<(), Error> {
        let raw: Vec<_> = rows
            .iter()
            .map(|row| v::bp_abi::TrueosUi4SolaraSceneTextRow {
                text_ptr: row.text.as_ptr(),
                text_len: row.text.len(),
                x: row.x,
                y: row.y,
                font_pixels: row.font_pixels,
            })
            .collect();
        status(unsafe {
            v::bp_abi::trueos_cabi_ui4_solara_text_scene(
                self.window_id,
                font as u32 | FONT_ID_TEXT_BACKBUFFER,
                canvas.0,
                canvas.1,
                color_rgba,
                raw.as_ptr(),
                raw.len(),
            )
        })
    }

    /// Crop the persistent text canvas into the active UI4 sprite-frame lease
    /// and publish it. No font work occurs after the first successful call.
    pub fn publish_text_backbuffer_view(
        &mut self,
        canvas: (u32, u32),
        origin: (u32, u32),
    ) -> Result<(), Error> {
        let Some(right) = origin.0.checked_add(self.width) else {
            return Err(Error::Invalid);
        };
        let Some(bottom) = origin.1.checked_add(self.height) else {
            return Err(Error::Invalid);
        };
        if canvas.0 == 0 || canvas.1 == 0 || right > canvas.0 || bottom > canvas.1 {
            return Err(Error::Invalid);
        }
        let u0 = origin.0 as f32 / canvas.0 as f32;
        let v0 = origin.1 as f32 / canvas.1 as f32;
        let u1 = right as f32 / canvas.0 as f32;
        let v1 = bottom as f32 / canvas.1 as f32;
        let quad = SpriteQuad {
            sprite_id: TEXT_BACKBUFFER_SPRITE_ID,
            c0: SpriteCorner {
                x: 0.0,
                y: 0.0,
                u: u0,
                v: v0,
            },
            c1: SpriteCorner {
                x: self.width as f32,
                y: 0.0,
                u: u1,
                v: v0,
            },
            c2: SpriteCorner {
                x: self.width as f32,
                y: self.height as f32,
                u: u1,
                v: v1,
            },
            c3: SpriteCorner {
                x: 0.0,
                y: self.height as f32,
                u: u0,
                v: v1,
            },
            color_rgba: rgba(255, 255, 255, 255),
            source_over: false,
        };
        self.draw_sprite_quads(core::slice::from_ref(&quad))?;
        self.publish(Damage::full(self.width, self.height))
    }

    /// Acquire a new UI4 lease, crop the already-materialized text canvas, and
    /// publish it. This is the complete steady-state pan operation.
    pub fn present_text_backbuffer_view(
        &mut self,
        canvas: (u32, u32),
        origin: (u32, u32),
        clear_rgba: u32,
    ) -> Result<(), Error> {
        self.begin_sprite_frame(clear_rgba)?;
        self.publish_text_backbuffer_view(canvas, origin)
    }

    /// Asynchronously rasterize fixed text once into the leased UI4 frame.
    ///
    /// This is the economical path for static labels or infrequently refreshed
    /// dashboards. Multiple calls before publication become ordered layers in
    /// one FontKernel request; no intermediate full-frame RGBA allocation or
    /// follow-up UI4 compositor pass is required.
    pub fn stamp_text_scene(
        &mut self,
        font: Font,
        viewport: (u32, u32),
        color_rgba: u32,
        rows: &[SceneTextRow<'_>],
    ) -> Result<(), Error> {
        if viewport != (self.width, self.height) {
            return Err(Error::Invalid);
        }
        let raw: Vec<_> = rows
            .iter()
            .map(|row| v::bp_abi::TrueosUi4SolaraSceneTextRow {
                text_ptr: row.text.as_ptr(),
                text_len: row.text.len(),
                x: row.x,
                y: row.y,
                font_pixels: row.font_pixels,
            })
            .collect();
        status(unsafe {
            v::bp_abi::trueos_cabi_ui4_solara_text_scene(
                self.window_id,
                font as u32 | FONT_ID_STAMP_ONCE,
                viewport.0,
                viewport.1,
                color_rgba,
                raw.as_ptr(),
                raw.len(),
            )
        })
    }

    pub fn publish(&mut self, damage: Damage) -> Result<(), Error> {
        status(unsafe {
            v::bp_abi::trueos_cabi_ui4_solara_frame_publish(
                self.window_id,
                damage.x,
                damage.y,
                damage.width,
                damage.height,
            )
        })
    }

    /// Close this UI4 frame with optional teardown work. If final-frame
    /// persistence is requested without a writable TRUEOSFS root, close still
    /// succeeds and the capture is skipped.
    pub fn close(mut self, request: CloseRequest) -> Result<(), Error> {
        let result = status(unsafe {
            v::bp_abi::trueos_cabi_ui4_solara_frame_close_requested(self.window_id, request.flags())
        });
        if result.is_ok() {
            self.window_id = 0;
        }
        result
    }
}

impl Drop for Frame {
    fn drop(&mut self) {
        if self.window_id != 0 {
            let _ = unsafe { v::bp_abi::trueos_cabi_ui4_solara_frame_close(self.window_id) };
            self.window_id = 0;
        }
    }
}

pub fn font_sizes() -> Result<Vec<FontSize>, Error> {
    let count = unsafe { v::bp_abi::trueos_cabi_ui4_solara_font_sizes(core::ptr::null_mut(), 0) };
    if count < 0 {
        return Err(error_from_status(count as i32));
    }
    let mut raw = Vec::with_capacity(count as usize);
    raw.resize(
        count as usize,
        v::bp_abi::TrueosUi4SolaraFontSize::default(),
    );
    let written =
        unsafe { v::bp_abi::trueos_cabi_ui4_solara_font_sizes(raw.as_mut_ptr(), raw.len()) };
    if written < 0 {
        return Err(error_from_status(written as i32));
    }
    raw.truncate((written as usize).min(raw.len()));
    Ok(raw
        .into_iter()
        .map(|size| FontSize {
            native_scale: size.native_scale,
            target_pixels: size.target_pixels,
        })
        .collect())
}

/// Pack conventional RGBA channels into the ABI's little-endian `u32` form.
pub const fn rgba(red: u8, green: u8, blue: u8, alpha: u8) -> u32 {
    u32::from_le_bytes([red, green, blue, alpha])
}

fn status(code: i32) -> Result<(), Error> {
    if code == 0 {
        Ok(())
    } else {
        Err(error_from_status(code))
    }
}

fn error_from_status(code: i32) -> Error {
    match code {
        -1 => Error::Invalid,
        -2 => Error::NoBlueprintContext,
        -3 => Error::NotFound,
        -4 => Error::InvalidState,
        -5 => Error::Font,
        -6 => Error::Ui4,
        -7 => Error::Busy,
        other => Error::Unknown(other),
    }
}
