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
