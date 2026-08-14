use std::future::Future;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Wake, Waker};

use trueos::vmedia::{DecodeBackend, ImageFormat};
use v::bp_abi::TrueosVmediaImageInfo;

const OPERATION_ID: u32 = 7;
const RGBA: [u8; 8] = [1, 2, 3, 255, 5, 6, 7, 128];

#[derive(Default)]
struct MockOperation {
    format: u32,
    total_len: usize,
    encoded: Vec<u8>,
    committed: bool,
    discarded: bool,
}

static OPERATION: Mutex<MockOperation> = Mutex::new(MockOperation {
    format: 0,
    total_len: 0,
    encoded: Vec::new(),
    committed: false,
    discarded: false,
});

#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_write(_stream: u32, _bytes: *const u8, _len: usize) {}

#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_blueprint_shutdown(_data_ptr: *const u8, _data_len: usize) -> i32 {
    0
}

#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_vmedia_image_decode_begin(format: u32, total_len: usize) -> i32 {
    let mut operation = OPERATION.lock().unwrap();
    *operation = MockOperation {
        format,
        total_len,
        encoded: Vec::new(),
        committed: false,
        discarded: false,
    };
    OPERATION_ID as i32
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_vmedia_image_decode_write(
    id: u32,
    offset: usize,
    bytes: *const u8,
    len: usize,
) -> i32 {
    assert_eq!(id, OPERATION_ID);
    let source = unsafe { std::slice::from_raw_parts(bytes, len) };
    let mut operation = OPERATION.lock().unwrap();
    assert_eq!(offset, operation.encoded.len());
    operation.encoded.extend_from_slice(source);
    0
}

#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_vmedia_image_decode_commit(id: u32) -> i32 {
    assert_eq!(id, OPERATION_ID);
    let mut operation = OPERATION.lock().unwrap();
    assert_eq!(operation.encoded.len(), operation.total_len);
    operation.committed = true;
    0
}

#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_vmedia_image_decode_status(id: u32) -> i32 {
    assert_eq!(id, OPERATION_ID);
    i32::from(OPERATION.lock().unwrap().committed)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_vmedia_image_decode_info(
    id: u32,
    out: *mut TrueosVmediaImageInfo,
) -> i32 {
    assert_eq!(id, OPERATION_ID);
    let operation = OPERATION.lock().unwrap();
    assert!(operation.committed);
    unsafe {
        out.write(TrueosVmediaImageInfo {
            width: 2,
            height: 1,
            stride_bytes: 8,
            byte_len: 8,
            source_format: operation.format,
            pixel_format: 1,
            backend: DecodeBackend::Png as u32,
            revision: 11,
        });
    }
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_vmedia_image_decode_read(
    id: u32,
    offset: usize,
    out: *mut u8,
    len: usize,
) -> isize {
    assert_eq!(id, OPERATION_ID);
    assert_eq!(offset, 0);
    assert_eq!(len, RGBA.len());
    unsafe { std::ptr::copy_nonoverlapping(RGBA.as_ptr(), out, RGBA.len()) };
    RGBA.len() as isize
}

#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_vmedia_image_decode_discard(id: u32) -> i32 {
    assert_eq!(id, OPERATION_ID);
    OPERATION.lock().unwrap().discarded = true;
    0
}

struct NoopWake;

impl Wake for NoopWake {
    fn wake(self: Arc<Self>) {}
}

fn ready<T>(future: impl Future<Output = T>) -> T {
    let waker = Waker::from(Arc::new(NoopWake));
    let mut context = Context::from_waker(&waker);
    let mut future = Box::pin(future);
    match future.as_mut().poll(&mut context) {
        Poll::Ready(output) => output,
        Poll::Pending => panic!("mock vmedia operation must complete immediately"),
    }
}

#[test]
fn public_vmedia_bridge_copies_rgba_and_discards_owner_operation() {
    let encoded = b"\x89PNG\r\n\x1a\nmock";
    let decoded = ready(trueos::vmedia::decode(ImageFormat::Png, encoded)).unwrap();

    assert_eq!(decoded.rgba, RGBA);
    assert_eq!(decoded.info.width, 2);
    assert_eq!(decoded.info.height, 1);
    assert_eq!(decoded.info.stride_bytes, 8);
    assert_eq!(decoded.info.source_format, ImageFormat::Png);
    assert_eq!(decoded.info.backend, DecodeBackend::Png);
    assert_eq!(decoded.info.revision, 11);

    let operation = OPERATION.lock().unwrap();
    assert_eq!(operation.encoded, encoded);
    assert!(operation.discarded);
}
