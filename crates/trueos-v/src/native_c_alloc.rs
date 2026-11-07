//! Blueprint-owned C allocations backed by the current Rust global allocator.
//! Exact layouts stay in each allocation's header; no process-wide C pointer
//! registry or retired kernel allocation import is involved. Pinned native C
//! build providers opt into these symbols as one allocator family.
use alloc::alloc::{alloc, alloc_zeroed, dealloc, realloc};
use core::{alloc::Layout, ffi::c_void, mem::size_of, ptr};

// max_align_t for the supported 64-bit C ABI is 16-byte aligned. C callers
// requiring greater alignment need a separate explicit aligned API.
#[repr(C, align(16))]
struct Header {
    requested: usize,
}
const PREFIX: usize = size_of::<Header>();
const _: () = assert!(PREFIX == 16);

fn layout(size: usize) -> Option<Layout> {
    Layout::from_size_align(size.max(1).checked_add(PREFIX)?, 16).ok()
}

unsafe fn allocate(size: usize, zeroed: bool) -> *mut c_void {
    let Some(layout) = layout(size) else {
        return ptr::null_mut();
    };
    let base = unsafe {
        if zeroed {
            alloc_zeroed(layout)
        } else {
            alloc(layout)
        }
    };
    if base.is_null() {
        return ptr::null_mut();
    }
    unsafe {
        base.cast::<Header>().write(Header { requested: size });
        base.add(PREFIX).cast()
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_blueprint_c_malloc(size: usize) -> *mut c_void {
    unsafe { allocate(size, false) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_blueprint_c_calloc(count: usize, size: usize) -> *mut c_void {
    let Some(size) = count.checked_mul(size) else {
        return ptr::null_mut();
    };
    unsafe { allocate(size, true) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_blueprint_c_free(pointer: *mut c_void) {
    if pointer.is_null() {
        return;
    }
    let base = unsafe { pointer.cast::<u8>().sub(PREFIX) };
    let size = unsafe { (*base.cast::<Header>()).requested };
    // SAFETY: only live allocations from this module cross this private C API.
    unsafe { dealloc(base, layout(size).unwrap_unchecked()) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_blueprint_c_realloc(
    pointer: *mut c_void,
    size: usize,
) -> *mut c_void {
    if pointer.is_null() {
        return unsafe { allocate(size, false) };
    }
    if size == 0 {
        unsafe { trueos_blueprint_c_free(pointer) };
        return ptr::null_mut();
    }
    let Some(new_layout) = layout(size) else {
        return ptr::null_mut();
    };
    let base = unsafe { pointer.cast::<u8>().sub(PREFIX) };
    let old_size = unsafe { (*base.cast::<Header>()).requested };
    let new_base = unsafe { realloc(base, layout(old_size).unwrap_unchecked(), new_layout.size()) };
    if new_base.is_null() {
        // realloc failure leaves both the old bytes and their header owned by C.
        return ptr::null_mut();
    }
    unsafe {
        new_base.cast::<Header>().write(Header { requested: size });
        new_base.add(PREFIX).cast()
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_blueprint_c_alloc_size(pointer: *mut c_void) -> usize {
    if pointer.is_null() {
        return 0;
    }
    unsafe { (*pointer.cast::<u8>().sub(PREFIX).cast::<Header>()).requested }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layouts_align_and_keep_the_requested_size() {
        for size in [0, 1, 3, 15, 16, 17, 8193, 1024 * 1024] {
            unsafe {
                let pointer = trueos_blueprint_c_malloc(size);
                assert!(!pointer.is_null());
                assert_eq!((pointer as usize) % 16, 0);
                assert_eq!(trueos_blueprint_c_alloc_size(pointer), size);
                if size != 0 {
                    pointer.cast::<u8>().write_bytes(0x5a, size);
                }
                trueos_blueprint_c_free(pointer);
            }
        }
    }

    #[test]
    fn calloc_is_zero_and_rejects_overflow() {
        unsafe {
            let pointer = trueos_blueprint_c_calloc(7, 33);
            assert!(!pointer.is_null());
            assert_eq!(trueos_blueprint_c_alloc_size(pointer), 231);
            assert!(
                core::slice::from_raw_parts(pointer.cast::<u8>(), 231)
                    .iter()
                    .all(|b| *b == 0)
            );
            trueos_blueprint_c_free(pointer);
            assert!(trueos_blueprint_c_calloc(usize::MAX, 2).is_null());
            assert!(trueos_blueprint_c_malloc(usize::MAX).is_null());
            assert!(trueos_blueprint_c_malloc(isize::MAX as usize).is_null());
        }
    }

    #[test]
    fn realloc_preserves_payload_and_failure_ownership() {
        unsafe {
            let mut pointer = trueos_blueprint_c_realloc(ptr::null_mut(), 101);
            assert!(!pointer.is_null());
            for i in 0..101 {
                pointer.cast::<u8>().add(i).write(i as u8);
            }
            assert!(trueos_blueprint_c_realloc(pointer, usize::MAX).is_null());
            assert_eq!(trueos_blueprint_c_alloc_size(pointer), 101);
            for size in [4097, 41, 8193] {
                pointer = trueos_blueprint_c_realloc(pointer, size);
                assert!(!pointer.is_null());
                assert_eq!((pointer as usize) % 16, 0);
                assert_eq!(trueos_blueprint_c_alloc_size(pointer), size);
                for i in 0..41 {
                    assert_eq!(pointer.cast::<u8>().add(i).read(), i as u8);
                }
            }
            assert!(trueos_blueprint_c_realloc(pointer, 0).is_null());
            trueos_blueprint_c_free(ptr::null_mut());
            assert_eq!(trueos_blueprint_c_alloc_size(ptr::null_mut()), 0);
        }
    }
}
