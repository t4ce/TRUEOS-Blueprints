//! Safe Blueprint facade for the experimental Solara text-row UI4 ABI.

use alloc::vec::Vec;

pub const MAX_SCENE_TEXT_ROWS_PER_CALL: usize = 64;

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

    pub fn set_position(&mut self, x: i32, y: i32) -> Result<(), Error> {
        status(unsafe { v::bp_abi::trueos_cabi_ui4_scene_frame_set_position(self.window_id, x, y) })
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

    /// Draw paint records without fitting their collective bounds to a stamp.
    ///
    /// Calls are composited into the current dirty back buffer, so consumers
    /// may issue one call per color and split large scenes into chunks.
    pub fn draw_text_scene(
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
        other => Error::Unknown(other),
    }
}
