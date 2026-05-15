//! Small `std` compatibility ABI for hosted Blueprint apps.
//!
//! These symbols are not the TRUEOS API. They are the POSIX-shaped entry points
//! Rust `std` expects while the implementation stays mapped to the Blueprint
//! platform/CABI surface.

use core::ffi::{c_char, c_int, c_void};
use core::ptr;
use core::sync::atomic::AtomicI32;

const EINVAL: c_int = 22;
const ERANGE: c_int = 34;

static ERRNO: AtomicI32 = AtomicI32::new(0);

#[unsafe(no_mangle)]
pub unsafe extern "C" fn __errno_location() -> *mut c_int {
    (&ERRNO as *const AtomicI32).cast_mut().cast::<c_int>()
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn errno_location() -> *mut c_int {
    unsafe { __errno_location() }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn pthread_self() -> usize {
    unsafe { crate::vcabi::trueos_cabi_thread_current_id() }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn pthread_setname_np(_thread: usize, _name: *const c_char) -> c_int {
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn pthread_getname_np(
    _thread: usize,
    name: *mut c_char,
    len: usize,
) -> c_int {
    if name.is_null() || len == 0 {
        return ERANGE;
    }
    unsafe { *name = 0 };
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn pthread_attr_init(_attr: *mut c_void) -> c_int {
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn pthread_attr_destroy(_attr: *mut c_void) -> c_int {
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn pthread_attr_setstacksize(
    _attr: *mut c_void,
    _stack_size: usize,
) -> c_int {
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn pthread_attr_setguardsize(
    _attr: *mut c_void,
    _guard_size: usize,
) -> c_int {
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn pthread_create(
    thread: *mut usize,
    _attr: *const c_void,
    _start: *mut c_void,
    _arg: *mut c_void,
) -> c_int {
    if thread.is_null() {
        return EINVAL;
    }
    unsafe { *thread = crate::vcabi::trueos_cabi_thread_current_id() };
    EINVAL
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn pthread_join(_thread: usize, _retval: *mut *mut c_void) -> c_int {
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn pthread_detach(_thread: usize) -> c_int {
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sched_yield() -> c_int {
    crate::platform::poll_once();
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn getenv(_name: *const c_char) -> *mut c_char {
    ptr::null_mut()
}
