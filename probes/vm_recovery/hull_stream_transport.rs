//! Probe-only Hull stream requests. Layout and serialization match
//! TRUEOS/crates/trueos-vm/src/vmcall.rs; the SDK's staged async upload does
//! not retain a disk write lease and cannot exercise abandoned stream cleanup.
use core::sync::atomic::{AtomicU32, Ordering};

pub const OP_BP_FS_WRITE_BEGIN: u32 = 0x2E;
pub const OP_BP_FS_WRITE_CHUNK: u32 = 0x2F;
pub const OP_BP_FS_WRITE_ABORT: u32 = 0x31;
pub const STATUS_OK: u32 = 0;
const PAYLOAD_CAP: usize = 160 * 1024 - 56;

#[repr(C)]
struct CommPage {
    request_op: u32,
    request_seq: u32,
    request_arg0: u64,
    request_arg1: u64,
    request_len: u32,
    request_pad: u32,
    response_seq: u32,
    response_status: u32,
    response_data: u64,
    response_len: u32,
    response_pad: u32,
    payload: [u8; PAYLOAD_CAP],
}

const _: () = assert!(core::mem::offset_of!(CommPage, payload) == 56);
const _: () = assert!(core::mem::size_of::<CommPage>() == 160 * 1024);
static SEQUENCE: AtomicU32 = AtomicU32::new(1);

struct PageGuard(*const AtomicU32);
impl Drop for PageGuard {
    fn drop(&mut self) { unsafe { &*self.0 }.store(0, Ordering::Release); }
}

/// Called only by Hull setup/normal Hull cleanup, never by native worker code.
pub fn call_with_payload(op: u32, argument: u64, request: &[u8]) -> (u32, u64) {
    assert!(request.len() <= PAYLOAD_CAP);
    let page = 0x2040_0000usize as *mut CommPage;
    let lock = unsafe { AtomicU32::from_ptr(core::ptr::addr_of_mut!((*page).request_pad)) };
    while lock.compare_exchange_weak(0, 1, Ordering::Acquire, Ordering::Relaxed).is_err() {
        core::hint::spin_loop();
    }
    let _guard = PageGuard(lock);
    unsafe {
        (&mut (*page).payload)[..request.len()].copy_from_slice(request);
        core::ptr::write_volatile(&mut (*page).request_arg0, argument);
        core::ptr::write_volatile(&mut (*page).request_arg1, 0);
        core::ptr::write_volatile(&mut (*page).request_len, request.len() as u32);
        core::ptr::write_volatile(&mut (*page).request_seq, SEQUENCE.fetch_add(1, Ordering::Relaxed));
        core::ptr::write_volatile(&mut (*page).request_op, op);
        core::arch::asm!("vmcall", options(nostack, preserves_flags));
        (core::ptr::read_volatile(&(*page).response_status),
         core::ptr::read_volatile(&(*page).response_data))
    }
}
