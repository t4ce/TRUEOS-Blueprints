extern crate alloc;

use alloc::vec;
use alloc::vec::Vec;

use crate::vcabi;

pub const KEYBOARD_OUTPUT_KIND_TEXT: u8 = 1;
pub const KEYBOARD_OUTPUT_KIND_KEY: u8 = 2;
pub const KEYBOARD_KEY_ENTER: u16 = 3;
pub const KEYBOARD_KEY_ESCAPE: u16 = 4;
pub const KEYBOARD_KEY_SPACE: u16 = 5;
pub const KEYBOARD_KEY_ARROW_UP: u16 = 12;
pub const KEYBOARD_KEY_ARROW_DOWN: u16 = 13;
pub const KEYBOARD_KEY_ARROW_LEFT: u16 = 14;
pub const KEYBOARD_KEY_ARROW_RIGHT: u16 = 15;

pub use crate::vcabi::{TrueosHidCursorEvent, TrueosKeyboardOutputEvent};

#[inline]
pub fn read_cursor_events_since(
    read_seq: u64,
    out_cap: u32,
) -> (Vec<TrueosHidCursorEvent>, u64, u32) {
    let mut events = vec![TrueosHidCursorEvent::default(); out_cap as usize];
    let mut next_seq = read_seq;
    let mut dropped = 0u32;
    let got = unsafe {
        vcabi::trueos_cabi_input_read_cursor_events_since(
            read_seq,
            events.as_mut_ptr(),
            out_cap,
            &mut next_seq,
            &mut dropped,
        )
    };
    events.truncate(got as usize);
    (events, next_seq, dropped)
}

#[inline]
pub fn pop_keyboard_output() -> Option<TrueosKeyboardOutputEvent> {
    let mut out = TrueosKeyboardOutputEvent::default();
    let rc = unsafe { vcabi::trueos_cabi_input_pop_keyboard_output(&mut out as *mut _) };
    if rc == 0 { Some(out) } else { None }
}

#[inline]
pub fn read_keyboard_output_since(
    read_seq: u64,
    out_cap: u32,
) -> (Vec<TrueosKeyboardOutputEvent>, u64, u32) {
    let mut events = vec![TrueosKeyboardOutputEvent::default(); out_cap as usize];
    let mut next_seq = read_seq;
    let mut dropped = 0u32;
    let got = unsafe {
        vcabi::trueos_cabi_input_read_keyboard_output_since(
            read_seq,
            events.as_mut_ptr(),
            out_cap,
            &mut next_seq,
            &mut dropped,
        )
    };
    events.truncate(got as usize);
    (events, next_seq, dropped)
}
