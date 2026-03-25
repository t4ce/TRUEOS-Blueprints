extern crate alloc;

use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;
use core::alloc::{GlobalAlloc, Layout};
use core::ffi::{c_char, CStr};
use core::panic::PanicInfo;

pub struct TrueosAllocator;

unsafe impl GlobalAlloc for TrueosAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        unsafe { std::alloc::System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { std::alloc::System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        unsafe { std::alloc::System.realloc(ptr, layout, new_size) }
    }
}

pub fn panic_handler(info: &PanicInfo<'_>) -> ! {
    eprintln!("panic: {}", info);
    std::process::abort();
}

pub fn host_args() -> Vec<String> {
    std::env::args().collect()
}

pub unsafe fn args_from_abi<'a>(argc: usize, argv: *const *const c_char) -> &'a [&'a str] {
    if argc == 0 || argv.is_null() {
        return &[];
    }

    let ptrs = unsafe { core::slice::from_raw_parts(argv, argc) };
    let mut strings = Vec::with_capacity(argc);

    for &ptr in ptrs {
        if ptr.is_null() {
            strings.push("");
            continue;
        }

        let arg = unsafe { CStr::from_ptr(ptr) }.to_str().unwrap_or("");
        strings.push(arg);
    }

    Box::leak(strings.into_boxed_slice())
}