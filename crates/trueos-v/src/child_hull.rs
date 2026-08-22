//! Generic parent/child Blueprint Hull transport.
//!
//! A parent receives an opaque handle from `spawn`. Inside a child, handle
//! zero addresses its parent. Payloads are byte frames; a zero-capacity
//! receive probes the retained frame size without consuming it.

use crate::bp_abi;

pub type Handle = u64;
pub const HANDLE_PARENT: Handle = 0;

pub const STATUS_STARTING: i32 = 1;
pub const STATUS_RUNNING: i32 = 2;
pub const STATUS_STOPPING: i32 = 3;
pub const STATUS_EXITED: i32 = 4;

pub const ERR_INVALID: i32 = -1;
pub const ERR_NOT_FOUND: i32 = -2;
pub const ERR_QUEUE_FULL: i32 = -3;
pub const ERR_UNAVAILABLE: i32 = -4;

pub fn spawn(initial: &[u8]) -> Result<Handle, i32> {
    let mut handle = 0;
    let rc = unsafe {
        bp_abi::trueos_cabi_blueprint_child_spawn_v1(initial.as_ptr(), initial.len(), &mut handle)
    };
    if rc == 0 && handle != HANDLE_PARENT {
        Ok(handle)
    } else {
        Err(if rc == 0 { ERR_UNAVAILABLE } else { rc })
    }
}

pub fn send(handle: Handle, bytes: &[u8]) -> Result<usize, i32> {
    let result =
        unsafe { bp_abi::trueos_cabi_blueprint_child_send_v1(handle, bytes.as_ptr(), bytes.len()) };
    if result < 0 {
        Err(result as i32)
    } else {
        Ok(result as usize)
    }
}

/// Return `Ok(None)` when no frame is ready, or the size of the retained next
/// frame. Calling this does not consume a frame.
pub fn receive_len(handle: Handle) -> Result<Option<usize>, i32> {
    let result =
        unsafe { bp_abi::trueos_cabi_blueprint_child_receive_v1(handle, core::ptr::null_mut(), 0) };
    if result < 0 {
        Err(result as i32)
    } else if result == 0 {
        Ok(None)
    } else {
        Ok(Some(result as usize))
    }
}

/// Copy and consume one frame. `out` must be at least `receive_len` bytes.
pub fn receive_into(handle: Handle, out: &mut [u8]) -> Result<Option<usize>, i32> {
    let result = unsafe {
        bp_abi::trueos_cabi_blueprint_child_receive_v1(handle, out.as_mut_ptr(), out.len())
    };
    if result < 0 {
        Err(result as i32)
    } else if result == 0 {
        Ok(None)
    } else {
        Ok(Some(result as usize))
    }
}

pub fn status(handle: Handle) -> i32 {
    unsafe { bp_abi::trueos_cabi_blueprint_child_status_v1(handle) }
}

pub fn terminate(handle: Handle) -> Result<(), i32> {
    let rc = unsafe { bp_abi::trueos_cabi_blueprint_child_terminate_v1(handle) };
    if rc == 0 { Ok(()) } else { Err(rc) }
}
