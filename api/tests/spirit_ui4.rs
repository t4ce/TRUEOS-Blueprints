//! Exercise the production Spirit wrappers against a deterministic native ABI.
//! No test reaches a live cursor, keyboard, or window service.

#![cfg(feature = "spirit")]

use std::sync::Mutex;
use trueos::spirit;
static FIXTURE: Mutex<Fixture> = Mutex::new(Fixture {
    read_error: 0,
    png_len: 40_000,
    windows_changed: false,
    pointer: None,
    key: 0,
    typed: String::new(),
    focused: 0,
    chunks: Vec::new(),
});
static SERIAL: Mutex<()> = Mutex::new(());
struct Fixture {
    read_error: isize,
    png_len: isize,
    windows_changed: bool,
    pointer: Option<(u16, u16, u32)>,
    key: u32,
    typed: String,
    focused: u64,
    chunks: Vec<(usize, usize)>,
}
fn reset() {
    *FIXTURE.lock().unwrap() = Fixture {
        read_error: 0,
        png_len: 40_000,
        windows_changed: false,
        pointer: None,
        key: 0,
        typed: String::new(),
        focused: 0,
        chunks: Vec::new(),
    };
}
mod native {
    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn trueos_cabi_spirit_agents_publish_v1(
        ptr: *const u8,
        len: usize,
    ) -> i32 {
        let bytes = unsafe { core::slice::from_raw_parts(ptr, len) };
        let snapshot = v::spirit_agents::decode(bytes).expect("validated snapshot wire bytes");
        *crate::SNAPSHOT.lock().unwrap() = Some(snapshot);
        crate::SNAPSHOT_ERROR.load(std::sync::atomic::Ordering::Relaxed)
    }
    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn trueos_cabi_spirit_emotion_play(_: *const u8, _: usize) -> i32 {
        0
    }
    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn trueos_cabi_spirit_text_present_silent(
        _: u64,
        _: *const u8,
        _: usize,
    ) -> i32 {
        -5
    }
    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn trueos_cabi_spirit_move(_: f32, _: f32) -> i32 {
        0
    }
    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn trueos_cabi_dobby_ui4_windows(out: *mut u8, cap: usize) -> isize {
        const BYTES: &[u8] = b"{\"windows\":[]}";
        let changed = crate::FIXTURE.lock().unwrap().windows_changed;
        if !out.is_null() && cap >= BYTES.len() {
            unsafe {
                core::ptr::copy_nonoverlapping(BYTES.as_ptr(), out, BYTES.len());
            }
        }
        (BYTES.len() + usize::from(changed && !out.is_null())) as isize
    }
    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn trueos_cabi_dobby_ui4_focus(id: u64) -> i32 {
        crate::FIXTURE.lock().unwrap().focused = id;
        0
    }
    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn trueos_cabi_dobby_ui4_observe_prepare() -> isize {
        crate::FIXTURE.lock().unwrap().png_len
    }
    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn trueos_cabi_dobby_ui4_observe_metadata(
        out: *mut u8,
        cap: usize,
    ) -> isize {
        const BYTES: &[u8] = b"{\"grid_extent\":1000}";
        if !out.is_null() && cap >= BYTES.len() {
            unsafe {
                core::ptr::copy_nonoverlapping(BYTES.as_ptr(), out, BYTES.len());
            }
        }
        BYTES.len() as isize
    }
    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn trueos_cabi_dobby_ui4_observe_read(
        offset: usize,
        out: *mut u8,
        cap: usize,
    ) -> isize {
        let mut f = crate::FIXTURE.lock().unwrap();
        f.chunks.push((offset, cap));
        match f.read_error {
            1 => return 0,
            2 => return cap as isize + 1,
            n if n < 0 => return n,
            _ => {}
        }
        unsafe {
            core::ptr::write_bytes(out, 0x42, cap);
        }
        cap as isize
    }
    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn trueos_cabi_dobby_ui4_pointer(x: u16, y: u16, action: u32) -> i32 {
        crate::FIXTURE.lock().unwrap().pointer = Some((x, y, action));
        0
    }
    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn trueos_cabi_dobby_ui4_type(ptr: *const u8, len: usize) -> i32 {
        crate::FIXTURE.lock().unwrap().typed =
            String::from_utf8(unsafe { core::slice::from_raw_parts(ptr, len) }.to_vec()).unwrap();
        0
    }
    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn trueos_cabi_dobby_ui4_key(key: u32) -> i32 {
        crate::FIXTURE.lock().unwrap().key = key;
        0
    }
}
static SNAPSHOT: Mutex<Option<spirit::spirit_agents::AgentSnapshot>> = Mutex::new(None);
static SNAPSHOT_ERROR: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(0);

#[test]
fn agent_snapshot_keeps_lifecycle_attention_and_revision() {
    use spirit::spirit_agents::*;
    let _serial = SERIAL.lock().unwrap();
    SNAPSHOT_ERROR.store(0, std::sync::atomic::Ordering::Relaxed);
    let snapshot = AgentSnapshot {
        version: SNAPSHOT_VERSION,
        epoch: 3,
        revision: 11,
        complete: true,
        connected: true,
        agents: vec![AgentEntry {
            id: "agent-1".into(),
            parent_id: Some("root".into()),
            name: Some("thought".into()),
            role: None,
            lifecycle: Some(AgentLifecycle::Running),
            runtime: Some(AgentRuntime::Active),
            waiting_approval: true,
            waiting_input: false,
            loaded: true,
        }],
    };
    spirit::publish_agent_snapshot(&snapshot).unwrap();
    assert_eq!(*SNAPSHOT.lock().unwrap(), Some(snapshot.clone()));
    SNAPSHOT_ERROR.store(-9, std::sync::atomic::Ordering::Relaxed);
    assert_eq!(
        spirit::publish_agent_snapshot(&snapshot),
        Err(spirit::Error(-9))
    );
    SNAPSHOT_ERROR.store(0, std::sync::atomic::Ordering::Relaxed);
}

#[test]
fn invalid_agent_snapshot_never_enters_native_service() {
    use spirit::spirit_agents::*;
    let _serial = SERIAL.lock().unwrap();
    *SNAPSHOT.lock().unwrap() = None;
    let mut snapshot = AgentSnapshot {
        version: SNAPSHOT_VERSION,
        epoch: 0,
        revision: 1,
        complete: true,
        connected: true,
        agents: vec![],
    };
    assert_eq!(
        spirit::publish_agent_snapshot(&snapshot),
        Err(spirit::Error(-3))
    );
    snapshot.epoch = 1;
    snapshot.agents = (0..=MAX_AGENTS)
        .map(|index| AgentEntry {
            id: format!("agent-{index}"),
            parent_id: Some("root".into()),
            name: None,
            role: None,
            lifecycle: Some(AgentLifecycle::Running),
            runtime: None,
            waiting_approval: false,
            waiting_input: false,
            loaded: true,
        })
        .collect();
    assert_eq!(
        spirit::publish_agent_snapshot(&snapshot),
        Err(spirit::Error(-4))
    );
    assert!(SNAPSHOT.lock().unwrap().is_none());
}
#[test]
fn generic_calls_keep_the_existing_wire_contract() {
    let _serial = SERIAL.lock().unwrap();
    reset();
    assert_eq!(spirit::ui4_windows().unwrap(), "{\"windows\":[]}");
    spirit::ui4_focus(12345).unwrap();
    spirit::ui4_pointer(0, 1000, spirit::Ui4PointerAction::Click).unwrap();
    spirit::ui4_type("Lilly §").unwrap();
    spirit::ui4_key(spirit::Ui4Key::Escape).unwrap();
    let f = FIXTURE.lock().unwrap();
    assert_eq!(f.focused, 12345);
    assert_eq!(
        f.pointer,
        Some((0, 1000, v::bp_abi::DOBBY_UI4_POINTER_PRIMARY_CLICK))
    );
    assert_eq!(f.typed, "Lilly §");
    assert_eq!(f.key, v::bp_abi::DOBBY_UI4_KEY_ESCAPE);
}
#[test]
fn observations_are_read_in_bounded_native_chunks() {
    let _serial = SERIAL.lock().unwrap();
    reset();
    let observation = spirit::ui4_observe().unwrap();
    assert_eq!(observation.metadata, "{\"grid_extent\":1000}");
    assert_eq!(observation.png, vec![0x42; 40000]);
    assert_eq!(
        FIXTURE.lock().unwrap().chunks,
        vec![(0, 16384), (16384, 16384), (32768, 7232)]
    );
}

#[test]
fn oversized_capture_is_rejected_before_reading_native_pixels() {
    let _serial = SERIAL.lock().unwrap();
    reset();
    FIXTURE.lock().unwrap().png_len = 64 * 1024 + 1;
    assert_eq!(spirit::ui4_observe(), Err(spirit::UI4_UNAVAILABLE));
    assert!(FIXTURE.lock().unwrap().chunks.is_empty());
}
#[test]
fn invalid_partial_read_sizes_are_protocol_errors() {
    let _serial = SERIAL.lock().unwrap();
    for mode in [1, 2] {
        reset();
        FIXTURE.lock().unwrap().read_error = mode;
        assert_eq!(
            spirit::ui4_observe(),
            Err(spirit::UI4_INCONSISTENT_RESPONSE)
        );
    }
}
#[test]
fn busy_and_other_native_errors_are_preserved() {
    let _serial = SERIAL.lock().unwrap();
    for error in [-1, -2, -3, -4, -5, -6, -7] {
        reset();
        FIXTURE.lock().unwrap().read_error = error;
        assert_eq!(spirit::ui4_observe(), Err(spirit::Error(error as i32)));
    }
}
#[test]
fn changed_inventory_sizes_are_protocol_errors() {
    let _serial = SERIAL.lock().unwrap();
    reset();
    FIXTURE.lock().unwrap().windows_changed = true;
    assert_eq!(
        spirit::ui4_windows(),
        Err(spirit::UI4_INCONSISTENT_RESPONSE)
    );
}
#[test]
fn legacy_rust_names_are_aliases_of_generic_calls() {
    let _serial = SERIAL.lock().unwrap();
    reset();
    assert_eq!(spirit::DOBBY_UI4_BUSY, spirit::UI4_BUSY);
    spirit::dobby_ui4_focus(77).unwrap();
    assert_eq!(FIXTURE.lock().unwrap().focused, 77);
    assert_eq!(spirit::dobby_ui4_windows(), spirit::ui4_windows());
}
#[test]
fn invalid_pointer_coordinates_do_not_reach_native_input() {
    let _serial = SERIAL.lock().unwrap();
    reset();
    assert_eq!(
        spirit::ui4_pointer(1001, 1, spirit::Ui4PointerAction::Move),
        Err(spirit::UI4_BAD_INPUT)
    );
    assert_eq!(FIXTURE.lock().unwrap().pointer, None);
}
