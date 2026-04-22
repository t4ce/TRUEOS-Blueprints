#![no_std]

use core::alloc::{GlobalAlloc, Layout};
use core::ptr::null_mut;

mod vcabi {
    pub use trueos_sys::vcabi::*;
}

pub mod ui2;
pub mod vgfx;
pub mod vgfx_hosted;
pub mod vsys;

pub struct TrueosAllocator;

// The thin blueprint path uses the host-exported C allocator directly.
unsafe impl GlobalAlloc for TrueosAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if layout.align() > core::mem::align_of::<usize>() {
            return null_mut();
        }
        unsafe { vcabi::trueos_cabi_alloc(layout.size().max(1)) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, _layout: Layout) {
        if ptr.is_null() {
            return;
        }
        unsafe { vcabi::trueos_cabi_free(ptr) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        if layout.align() > core::mem::align_of::<usize>() {
            return null_mut();
        }
        unsafe { vcabi::trueos_cabi_calloc(1, layout.size().max(1)) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        if layout.align() > core::mem::align_of::<usize>() {
            return null_mut();
        }
        if ptr.is_null() {
            return unsafe { self.alloc(layout) };
        }
        unsafe { vcabi::trueos_cabi_realloc(ptr, new_size.max(1)) }
    }
}

pub fn panic_abort(message: &str) -> ! {
    vsys::log_error(message);
    loop {
        core::hint::spin_loop();
    }
}

pub mod prelude {
    pub use crate::panic_abort;
    pub use crate::TrueosAllocator;
    pub use crate::ui2;
    pub use crate::vgfx;
    pub use crate::vsys;
}
