extern crate alloc;

use alloc::vec::Vec;

pub use crate::vcabi::{TrueosHidCursorEvent, TrueosHidKeyboardSample, TrueosMouseState};

#[inline]
pub fn mouse_poll() -> Option<TrueosMouseState> {
    None
}

#[inline]
pub fn qjs_mouse_pop() -> Option<TrueosMouseState> {
    None
}

#[inline]
pub fn pop_mouse_delta() -> Option<(u8, i8, i8, i8)> {
    None
}

#[inline]
pub fn cursor_pos(_cursor_id: u32) -> Result<(i32, i32), i32> {
    Err(-1)
}

#[inline]
pub fn cursor_buttons(_cursor_id: u32) -> Result<u32, i32> {
    Err(-1)
}

#[inline]
pub fn read_cursor_events_since(
    read_seq: u64,
    _out_cap: u32,
) -> Result<(Vec<TrueosHidCursorEvent>, u64, u32), i32> {
    Ok((Vec::new(), read_seq, 0))
}

#[inline]
pub fn write_cursor(
    _slot_id: u32,
    _x: i32,
    _y: i32,
    _buttons_down: u32,
    _wheel: i32,
    _flags: u32,
) -> Result<(), i32> {
    Err(-1)
}

#[inline]
pub fn hid_keyboard_read(
    _controller_id: u32,
    _slot_id: u32,
    _ep_target: u32,
    _out_cap: u32,
) -> (Vec<TrueosHidKeyboardSample>, u32) {
    (Vec::new(), 0)
}

#[inline]
pub fn write_keyboard_text(_slot_id: u32, _bytes: &[u8], _flags: u32) -> Result<(), i32> {
    Err(-1)
}

#[inline]
pub fn write_keyboard_key(
    _slot_id: u32,
    _codepoint: u32,
    _key_code: u32,
    _modifiers: u32,
    _flags: u32,
) -> Result<(), i32> {
    Err(-1)
}