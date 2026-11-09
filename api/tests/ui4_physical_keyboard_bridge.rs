use std::sync::atomic::{AtomicI32, AtomicUsize, Ordering};
use trueos::{input, ui4_scene::{Error, Frame}};
use input::TrueosKeyboardOutputEvent;

static RESULT: AtomicI32 = AtomicI32::new(0);
static CALLS: AtomicUsize = AtomicUsize::new(0);
#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_ui4_solara_frame_open(_: i32, _: i32, _: u32, _: u32) -> u32 { 42 }
#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_ui4_solara_frame_close(_: u32) -> i32 { 0 }
#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_write(_: u32, _: *const u8, _: usize) {}
#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_blueprint_shutdown(_: *const u8, _: usize) -> i32 { 0 }
#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_ui4_scene_keyboard_event_take_v1(
    window: u32, out: *mut TrueosKeyboardOutputEvent,
) -> i32 {
    assert_eq!(window, 42);
    let index = CALLS.fetch_add(1, Ordering::Relaxed);
    unsafe { out.write(TrueosKeyboardOutputEvent {
        controller_id: 11, slot_id: 22, ep_target: 33,
        kind: input::KEYBOARD_OUTPUT_KIND_PHYSICAL, key_code: 0xe1,
        flags: if index == 0 { input::KEYBOARD_OUTPUT_FLAG_PRESS } else { 0 },
        ..Default::default()
    }); }
    RESULT.load(Ordering::Relaxed)
}

#[test]
fn physical_stream_preserves_source_usage_release_and_empty_or_busy_results() {
    let mut frame = Frame::open(0, 0, 800, 600).unwrap();
    let press = frame.take_physical_keyboard_event().unwrap().unwrap();
    assert_eq!((press.controller_id, press.slot_id, press.ep_target), (11, 22, 33));
    assert_eq!((press.kind, press.key_code, press.flags),
               (input::KEYBOARD_OUTPUT_KIND_PHYSICAL, 0xe1, input::KEYBOARD_OUTPUT_FLAG_PRESS));
    let release = frame.take_physical_keyboard_event().unwrap().unwrap();
    assert_eq!((release.key_code, release.flags), (0xe1, 0));
    RESULT.store(1, Ordering::Relaxed);
    assert!(frame.take_physical_keyboard_event().unwrap().is_none());
    RESULT.store(-7, Ordering::Relaxed);
    assert!(matches!(frame.take_physical_keyboard_event(), Err(Error::Busy)));
}
