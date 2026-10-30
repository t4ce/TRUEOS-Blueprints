use std::sync::atomic::{AtomicI32, AtomicUsize, Ordering};
use trueos::ui4_scene::{
    Error, HULL_IMAGE_FAILED, HULL_IMAGE_PENDING, HULL_IMAGE_READY, HullImageInfo,
    hull_image_release, hull_image_request, hull_image_reset, hull_image_status,
};

static RESULT: AtomicI32 = AtomicI32::new(27);
static CALLS: AtomicUsize = AtomicUsize::new(0);
const GENERATION: u64 = 0x1234_5678_9ABC_DEF0;

#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_write(_: u32, _: *const u8, _: usize) {}
#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_blueprint_shutdown(_: *const u8, _: usize) -> i32 { 0 }

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_ui4_hull_image_request_v1(
    window: u32, url: *const u8, url_len: usize, cache_root: *const u8,
    cache_root_len: usize, page_host: *const u8, page_host_len: usize,
    width: u32, height: u32, generation: u64,
) -> i32 {
    CALLS.fetch_add(1, Ordering::Relaxed);
    assert_eq!((window, width, height, generation), (42, 320, 200, GENERATION));
    assert_eq!(unsafe { std::slice::from_raw_parts(url, url_len) }, b"https://example.org/a.png");
    assert_eq!(unsafe { std::slice::from_raw_parts(cache_root, cache_root_len) }, b"/home/web/cache");
    assert_eq!(unsafe { std::slice::from_raw_parts(page_host, page_host_len) }, b"example.org");
    RESULT.load(Ordering::Relaxed)
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_ui4_hull_image_status_v1(
    window: u32, ticket: u32, out: *mut HullImageInfo,
) -> i32 {
    assert_eq!((window, ticket), (42, 27));
    CALLS.fetch_add(1, Ordering::Relaxed);
    let result = RESULT.load(Ordering::Relaxed);
    if result >= 0 {
        unsafe { out.write(HullImageInfo {
            state: result as u32, sprite_id: 0xC000_0123, width: 320, height: 200,
            generation: GENERATION, error: if result == 2 { -6 } else { 0 }, reserved: 0,
        }); }
        0
    } else { result }
}
#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_ui4_hull_image_release_v1(window: u32, ticket: u32) -> i32 {
    assert_eq!((window, ticket), (42, 27));
    CALLS.fetch_add(1, Ordering::Relaxed);
    RESULT.load(Ordering::Relaxed)
}
#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_ui4_hull_image_reset_v1(window: u32, generation: u64) -> i32 {
    assert_eq!((window, generation), (42, GENERATION));
    CALLS.fetch_add(1, Ordering::Relaxed);
    RESULT.load(Ordering::Relaxed)
}

#[test]
fn async_image_tickets_states_generation_and_retry_errors_round_trip() {
    let request = || hull_image_request(42, "https://example.org/a.png", "/home/web/cache", "example.org", 320, 200, GENERATION);
    assert_eq!(request(), Ok(27));
    for state in [HULL_IMAGE_PENDING, HULL_IMAGE_READY, HULL_IMAGE_FAILED] {
        RESULT.store(state as i32, Ordering::Relaxed);
        let info = hull_image_status(42, 27).unwrap();
        assert_eq!(info.state, state);
        assert_eq!(info.generation, GENERATION);
        assert_eq!(info.sprite_id, 0xC000_0123);
        assert_eq!((info.width, info.height), (320, 200));
        assert_eq!(info.error, if state == HULL_IMAGE_FAILED { -6 } else { 0 });
    }
    RESULT.store(-7, Ordering::Relaxed);
    assert_eq!(request(), Err(Error::Busy));
    assert_eq!(hull_image_status(42, 27), Err(Error::Busy));
    assert_eq!(hull_image_release(42, 27), Err(Error::Busy));
    assert_eq!(hull_image_reset(42, GENERATION), Err(Error::Busy));
    RESULT.store(-3, Ordering::Relaxed);
    assert_eq!(hull_image_status(42, 27), Err(Error::NotFound));
    RESULT.store(0, Ordering::Relaxed);
    assert_eq!(request(), Err(Error::InvalidState));
    assert_eq!(hull_image_release(42, 27), Ok(()));
    assert_eq!(hull_image_reset(42, GENERATION), Ok(()));
    let before = CALLS.load(Ordering::Relaxed);
    assert_eq!(hull_image_request(0, "https://example.org/a.png", "/home/web/cache", "example.org", 320, 200, GENERATION), Err(Error::Invalid));
    assert_eq!(hull_image_request(42, "", "/home/web/cache", "example.org", 320, 200, GENERATION), Err(Error::Invalid));
    assert_eq!(hull_image_request(42, "https://example.org/a.png", "/home/web/cache", "example.org", 4097, 200, GENERATION), Err(Error::Invalid));
    assert_eq!(hull_image_status(42, 0), Err(Error::Invalid));
    assert_eq!(hull_image_release(42, 0), Err(Error::Invalid));
    assert_eq!(hull_image_reset(0, GENERATION), Err(Error::Invalid));
    assert_eq!(CALLS.load(Ordering::Relaxed), before);
}
