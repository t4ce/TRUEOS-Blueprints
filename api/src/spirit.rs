//! Generic Spirit presentation capabilities for Blueprints.
//!
//! These calls do not own an inference session. In particular,
//! [`present_text_silent`] requests visual text without local text-to-voice
//! generation. Kernels without a general text presenter return an error;
//! Spirit's boot greeting is a separate use of the embedded Paper app.
//!
//! UI4 interaction uses Lilly's tagged software cursor and paired keyboard.
//! It is independent of the permanent hardware-cursor avatar's [`move_to`].
//! Calls operate on kernel-authorized application instances and return
//! [`UI4_BUSY`] while the shared input serializer finishes an earlier program.

use alloc::string::String;
use alloc::vec::Vec;

const MAX_UI4_WINDOWS_BYTES: usize = 32 * 1024;
const MAX_UI4_METADATA_BYTES: usize = 4 * 1024;
const MAX_UI4_PNG_BYTES: usize = 64 * 1024;
const UI4_READ_CHUNK: usize = 16 * 1024;

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct Error(pub i32);

/// The calling application is not authorized to use Spirit's UI4 controls.
pub const UI4_DENIED: Error = Error(v::bp_abi::DOBBY_UI4_ERROR_DENIED);
/// The selected window, observation, or input state cannot serve this request.
pub const UI4_BAD_STATE: Error = Error(v::bp_abi::DOBBY_UI4_ERROR_BAD_STATE);
/// An argument is outside the supported input contract.
pub const UI4_BAD_INPUT: Error = Error(v::bp_abi::DOBBY_UI4_ERROR_BAD_INPUT);
/// The shared Lilly cursor/keyboard serializer is still draining an earlier program.
pub const UI4_BUSY: Error = Error(v::bp_abi::DOBBY_UI4_ERROR_BUSY);
/// The required native service or response buffer is unavailable.
pub const UI4_UNAVAILABLE: Error = Error(v::bp_abi::DOBBY_UI4_ERROR_UNAVAILABLE);
/// The requested live window was not found.
pub const UI4_NOT_FOUND: Error = Error(v::bp_abi::DOBBY_UI4_ERROR_NOT_FOUND);
/// The request could not cross the Blueprint transport.
pub const UI4_TRANSPORT: Error = Error(v::bp_abi::DOBBY_UI4_ERROR_TRANSPORT);
/// A native response changed size or returned an invalid partial read.
pub const UI4_INCONSISTENT_RESPONSE: Error = Error(-9);

#[doc(hidden)]
pub const DOBBY_UI4_BUSY: Error = UI4_BUSY;

fn result(code: i32) -> Result<(), Error> {
    if code == 0 { Ok(()) } else { Err(Error(code)) }
}

/// Queue a model-facing emotion: anger, disgust, fear, joy, sadness, or surprise.
pub fn play_emotion(idea: &str) -> Result<(), Error> {
    result(unsafe { v::bp_abi::trueos_cabi_spirit_emotion_play(idea.as_ptr(), idea.len()) })
}

/// Request display-safe UTF-8 without invoking a local inference or voice path.
/// Returns an error when the kernel's general text presenter is unavailable.
pub fn present_text_silent(turn: u64, text: &str) -> Result<(), Error> {
    result(unsafe {
        v::bp_abi::trueos_cabi_spirit_text_present_silent(turn, text.as_ptr(), text.len())
    })
}

/// Move Spirit to one normalized scanout point. Both axes are inclusive 0..=1.
pub fn move_to(x_normalized: f32, y_normalized: f32) -> Result<(), Error> {
    if !x_normalized.is_finite()
        || !y_normalized.is_finite()
        || !(0.0..=1.0).contains(&x_normalized)
        || !(0.0..=1.0).contains(&y_normalized)
    {
        return Err(Error(-3));
    }
    result(unsafe { v::bp_abi::trueos_cabi_spirit_move(x_normalized, y_normalized) })
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Ui4Observation {
    pub metadata: String,
    pub png: Vec<u8>,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum Ui4PointerAction {
    Move,
    Click,
}

impl Ui4PointerAction {
    const fn abi(self) -> u32 {
        match self {
            Self::Move => v::bp_abi::DOBBY_UI4_POINTER_MOVE,
            Self::Click => v::bp_abi::DOBBY_UI4_POINTER_PRIMARY_CLICK,
        }
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum Ui4Key {
    Enter,
    Escape,
    Tab,
    Space,
    Backspace,
    Up,
    Down,
    Left,
    Right,
}

impl Ui4Key {
    const fn abi(self) -> u32 {
        match self {
            Self::Enter => v::bp_abi::DOBBY_UI4_KEY_ENTER,
            Self::Escape => v::bp_abi::DOBBY_UI4_KEY_ESCAPE,
            Self::Tab => v::bp_abi::DOBBY_UI4_KEY_TAB,
            Self::Space => v::bp_abi::DOBBY_UI4_KEY_SPACE,
            Self::Backspace => v::bp_abi::DOBBY_UI4_KEY_BACKSPACE,
            Self::Up => v::bp_abi::DOBBY_UI4_KEY_ARROW_UP,
            Self::Down => v::bp_abi::DOBBY_UI4_KEY_ARROW_DOWN,
            Self::Left => v::bp_abi::DOBBY_UI4_KEY_ARROW_LEFT,
            Self::Right => v::bp_abi::DOBBY_UI4_KEY_ARROW_RIGHT,
        }
    }
}

fn bounded_len(code: isize, maximum: usize) -> Result<usize, Error> {
    if code < 0 {
        return Err(Error(code as i32));
    }
    let len = usize::try_from(code).map_err(|_| Error(-3))?;
    if len > maximum {
        Err(Error(-5))
    } else {
        Ok(len)
    }
}

/// Return a compact live inventory of brokered UI4 windows, bounded to 32 KiB.
/// Window identifiers belong to current native window lifetimes.
pub fn ui4_windows() -> Result<String, Error> {
    let needed = bounded_len(
        unsafe { v::bp_abi::trueos_cabi_dobby_ui4_windows(core::ptr::null_mut(), 0) },
        MAX_UI4_WINDOWS_BYTES,
    )?;
    let mut bytes = Vec::new();
    bytes.try_reserve_exact(needed).map_err(|_| Error(-5))?;
    bytes.resize(needed, 0);
    let read = bounded_len(
        unsafe { v::bp_abi::trueos_cabi_dobby_ui4_windows(bytes.as_mut_ptr(), bytes.len()) },
        MAX_UI4_WINDOWS_BYTES,
    )?;
    if read != needed {
        return Err(UI4_INCONSISTENT_RESPONSE);
    }
    String::from_utf8(bytes).map_err(|_| Error(-3))
}

/// Select a current live window for Spirit's own UI4 cursor/keyboard source.
pub fn ui4_focus(window_id: u64) -> Result<(), Error> {
    result(unsafe { v::bp_abi::trueos_cabi_dobby_ui4_focus(window_id) })
}

/// Capture the selected UI4 window with its normalized interaction grid.
/// Metadata is bounded to 4 KiB and the PNG to 64 KiB. Capture before pointer
/// interaction so coordinates refer to the current selected-window observation.
pub fn ui4_observe() -> Result<Ui4Observation, Error> {
    let png_len = bounded_len(
        unsafe { v::bp_abi::trueos_cabi_dobby_ui4_observe_prepare() },
        MAX_UI4_PNG_BYTES,
    )?;
    let metadata_len = bounded_len(
        unsafe { v::bp_abi::trueos_cabi_dobby_ui4_observe_metadata(core::ptr::null_mut(), 0) },
        MAX_UI4_METADATA_BYTES,
    )?;
    let mut metadata = Vec::new();
    metadata
        .try_reserve_exact(metadata_len)
        .map_err(|_| Error(-5))?;
    metadata.resize(metadata_len, 0);
    let read = bounded_len(
        unsafe {
            v::bp_abi::trueos_cabi_dobby_ui4_observe_metadata(metadata.as_mut_ptr(), metadata.len())
        },
        MAX_UI4_METADATA_BYTES,
    )?;
    if read != metadata_len {
        return Err(UI4_INCONSISTENT_RESPONSE);
    }

    let mut png = Vec::new();
    png.try_reserve_exact(png_len).map_err(|_| Error(-5))?;
    png.resize(png_len, 0);
    let mut offset = 0usize;
    while offset < png_len {
        let cap = core::cmp::min(UI4_READ_CHUNK, png_len - offset);
        let read = unsafe {
            v::bp_abi::trueos_cabi_dobby_ui4_observe_read(
                offset,
                png[offset..offset + cap].as_mut_ptr(),
                cap,
            )
        };
        if read < 0 {
            return Err(Error(read as i32));
        }
        if read == 0 || read as usize > cap {
            return Err(UI4_INCONSISTENT_RESPONSE);
        }
        offset += read as usize;
    }
    Ok(Ui4Observation {
        metadata: String::from_utf8(metadata).map_err(|_| Error(-3))?,
        png,
    })
}

/// Operate Spirit's software cursor in 0..=1000 selected-window grid units.
pub fn ui4_pointer(x: u16, y: u16, action: Ui4PointerAction) -> Result<(), Error> {
    if x > 1_000 || y > 1_000 {
        return Err(Error(-3));
    }
    result(unsafe { v::bp_abi::trueos_cabi_dobby_ui4_pointer(x, y, action.abi()) })
}

/// Type through the keyboard paired with Spirit's UI4 software cursor.
/// Each request accepts at most 64 Unicode scalars and 1,024 UTF-8 bytes.
/// A selected window must receive input; successful submission queues typing.
pub fn ui4_type(text: &str) -> Result<(), Error> {
    result(unsafe { v::bp_abi::trueos_cabi_dobby_ui4_type(text.as_ptr(), text.len()) })
}

/// Press one named interaction key through Spirit's UI4 keyboard.
pub fn ui4_key(key: Ui4Key) -> Result<(), Error> {
    result(unsafe { v::bp_abi::trueos_cabi_dobby_ui4_key(key.abi()) })
}

// Rust source compatibility; the native wire symbols retain their established ABI.
#[doc(hidden)]
pub use self::{
    ui4_focus as dobby_ui4_focus, ui4_key as dobby_ui4_key, ui4_observe as dobby_ui4_observe,
    ui4_pointer as dobby_ui4_pointer, ui4_type as dobby_ui4_type, ui4_windows as dobby_ui4_windows,
};

#[cfg(test)]
mod tests {
    use super::{Error, move_to};

    #[test]
    fn move_to_rejects_non_normalized_coordinates_before_crossing_the_abi() {
        assert_eq!(move_to(-0.1, 0.5), Err(Error(-3)));
        assert_eq!(move_to(0.5, 1.1), Err(Error(-3)));
        assert_eq!(move_to(f32::NAN, 0.5), Err(Error(-3)));
    }
}
