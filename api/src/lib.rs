#![no_std]

extern crate alloc;
#[cfg(feature = "tokio-runtime")]
extern crate std;
pub extern crate alloc as alloc_crate;
use core::alloc::{GlobalAlloc, Layout};
use core::ptr::null_mut;
#[cfg(feature = "tokio-runtime")]
pub use tokio;
pub use v::env;
pub use v::vclock as clock;
pub use v::vinput as hid;
pub use v::vshell;
pub use v::vsys;

pub mod platform {
    pub use alloc::borrow::{Cow, ToOwned};
    pub use alloc::boxed::Box;
    pub use alloc::format;
    pub use alloc::string::{String, ToString};
    pub use alloc::sync::Arc;
    pub use alloc::vec;
    pub use alloc::vec::Vec;
    pub use core::future;

    pub use v::vsys::{poll_once, sleep_ms, write_stream};

    #[cfg(feature = "tokio-runtime")]
    pub mod io {
        pub use tokio::io::{Error, ErrorKind, Result, SeekFrom};
    }

    #[cfg(feature = "tokio-runtime")]
    pub mod path {
        pub use std::path::{Component, Components, Path, PathBuf};
    }

    #[cfg(feature = "tokio-runtime")]
    pub mod thread {
        pub use std::thread::{Thread, ThreadId, current};

        #[inline]
        pub fn yield_now() {
            v::vsys::poll_once();
        }
    }
}

pub mod logl {
    use alloc::string::{String, ToString};
    use core::fmt::Write as _;

    pub mod level {
        pub const ERROR: u8 = 1;
        pub const WARN: u8 = 2;
        pub const INFO: u8 = 3;
        pub const DEBUG: u8 = 4;
        pub const TRACE: u8 = 5;
    }

    pub fn log(level: u8, message: impl IntoLogMessage) {
        let stream = if level <= level::ERROR { 2 } else { 1 };
        let line = message.into_log_message();
        v::vsys::write_stream(stream, line.as_bytes());
        if !line.ends_with('\n') {
            v::vsys::write_stream(stream, b"\n");
        }
    }

    pub trait IntoLogMessage {
        fn into_log_message(self) -> String;
    }

    impl IntoLogMessage for &str {
        fn into_log_message(self) -> String {
            self.to_string()
        }
    }

    impl IntoLogMessage for core::fmt::Arguments<'_> {
        fn into_log_message(self) -> String {
            let mut out = String::new();
            let _ = out.write_fmt(self);
            out
        }
    }
}

#[cfg(feature = "tokio-runtime")]
pub mod runtime {
    pub use tokio::runtime::{Builder, Handle, Runtime};

    pub fn current_thread() -> Builder {
        let mut builder = tokio::runtime::Builder::new_current_thread();
        builder.enable_time();
        builder
    }

    #[cfg(feature = "tokio-net-probe")]
    pub fn current_thread_net() -> Builder {
        let mut builder = current_thread();
        builder.enable_io();
        builder
    }
}

#[cfg(feature = "tokio-runtime")]
pub mod task {
    pub use tokio::spawn;
    pub use tokio::task::{JoinError, JoinHandle, JoinSet, LocalSet, yield_now};
}

#[cfg(feature = "tokio-runtime")]
pub mod sync {
    pub use tokio::sync::{
        Barrier, Mutex, Notify, RwLock, Semaphore, broadcast, mpsc, oneshot, watch,
    };
}

#[cfg(feature = "tokio-runtime")]
pub mod time {
    pub use tokio::time::{Duration, Instant, Interval, Sleep, interval, sleep, timeout};
}

pub mod rng {
    #[inline]
    pub fn fill(bytes: &mut [u8]) {
        if bytes.is_empty() {
            return;
        }

        let mut offset = 0usize;
        while offset < bytes.len() {
            let mut word = u32();
            let chunk = core::cmp::min(core::mem::size_of::<u32>(), bytes.len() - offset);
            bytes[offset..offset + chunk].copy_from_slice(&word.to_ne_bytes()[..chunk]);
            offset += chunk;
        }
    }

    #[inline]
    pub fn u8() -> u8 {
        u32() as u8
    }

    #[inline]
    pub fn i8() -> i8 {
        u8() as i8
    }

    #[inline]
    pub fn u16() -> u16 {
        u32() as u16
    }

    #[inline]
    pub fn i16() -> i16 {
        u16() as i16
    }

    #[inline]
    pub fn u32() -> u32 {
        let mut word = 0u32;
        unsafe { v::vcabi::sys_rand(&mut word, 1) };
        word
    }

    #[inline]
    pub fn i32() -> i32 {
        u32() as i32
    }

    #[inline]
    pub fn u64() -> u64 {
        let lo = u32() as u64;
        let hi = u32() as u64;
        lo | (hi << 32)
    }

    #[inline]
    pub fn i64() -> i64 {
        u64() as i64
    }

    #[inline]
    pub fn u128() -> u128 {
        let lo = u64() as u128;
        let hi = u64() as u128;
        lo | (hi << 64)
    }

    #[inline]
    pub fn i128() -> i128 {
        u128() as i128
    }

    #[inline]
    pub fn usize() -> usize {
        if usize::BITS <= 32 {
            u32() as usize
        } else {
            u64() as usize
        }
    }

    #[inline]
    pub fn isize() -> isize {
        usize() as isize
    }

    #[inline]
    pub fn boolean() -> bool {
        (u32() & 1) != 0
    }

    #[inline]
    pub fn f32() -> f32 {
        const SCALE: f32 = 1.0 / ((1u64 << 24) as f32);
        ((u32() >> 8) as f32) * SCALE
    }

    #[inline]
    pub fn f64() -> f64 {
        const SCALE: f64 = 1.0 / ((1u64 << 53) as f64);
        ((u64() >> 11) as f64) * SCALE
    }
}

#[cfg(feature = "tokio-runtime")]
pub mod io {
    pub use tokio::io::{
        AsyncBufReadExt, AsyncReadExt, AsyncSeekExt, AsyncWriteExt, Stderr, Stdin, Stdout, duplex,
        stderr, stdin, stdout,
    };
}

#[cfg(feature = "tokio-runtime")]
pub mod fs {
    pub use tokio::fs::{
        File, OpenOptions, canonicalize, copy, create_dir, create_dir_all, metadata, read,
        read_to_string, remove_dir_all, remove_file, rename, try_exists, write,
    };
}

#[cfg(feature = "tokio-net-probe")]
pub mod net {
    pub use tokio::net::{TcpListener, TcpStream, ToSocketAddrs, UdpSocket, lookup_host};

    pub mod mio {
        pub use mio::{Events, Interest, Poll, Registry, Token, Waker};
        pub use mio::{event, net};
    }

    pub mod socket2 {
        pub use socket2::{Domain, Protocol, SockAddr, Socket, Type};
    }
}

#[cfg(feature = "tokio-runtime")]
pub mod t {
    pub use crate::fs;
    pub use crate::io;
    #[cfg(feature = "tokio-net-probe")]
    pub use crate::net;
    pub use crate::runtime;
    pub use crate::sync;
    pub use crate::task;
    pub use crate::time;
    pub use crate::tokio;
}

pub struct TrueosAllocator;

#[cfg(feature = "default-global-allocator")]
#[global_allocator]
static DEFAULT_GLOBAL_ALLOCATOR: TrueosAllocator = TrueosAllocator;

unsafe impl GlobalAlloc for TrueosAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if layout.align() > core::mem::align_of::<usize>() {
            return unsafe { alloc_aligned(layout) };
        }
        unsafe { v::vcabi::trueos_cabi_alloc(layout.size().max(1)) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        if ptr.is_null() {
            return;
        }
        unsafe { v::vcabi::trueos_cabi_free(ptr) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        if layout.align() > core::mem::align_of::<usize>() {
            let ptr = unsafe { alloc_aligned(layout) };
            if !ptr.is_null() {
                unsafe { core::ptr::write_bytes(ptr, 0, layout.size().max(1)) };
            }
            return ptr;
        }
        unsafe { v::vcabi::trueos_cabi_calloc(1, layout.size().max(1)) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        if ptr.is_null() {
            return unsafe { self.alloc(layout) };
        }
        if layout.align() > core::mem::align_of::<usize>() {
            let Some(new_layout) = Layout::from_size_align(new_size.max(1), layout.align()).ok()
            else {
                return null_mut();
            };
            let new_ptr = unsafe { alloc_aligned(new_layout) };
            if !new_ptr.is_null() {
                unsafe {
                    core::ptr::copy_nonoverlapping(
                        ptr,
                        new_ptr,
                        core::cmp::min(layout.size(), new_size),
                    );
                    v::vcabi::trueos_cabi_free(ptr);
                }
            }
            return new_ptr;
        }
        unsafe { v::vcabi::trueos_cabi_realloc(ptr, new_size.max(1)) }
    }
}

unsafe fn alloc_aligned(layout: Layout) -> *mut u8 {
    unsafe { v::vcabi::sys_alloc_aligned(layout.size().max(1), layout.align()) }
}

pub fn panic_abort(message: &str) -> ! {
    v::vsys::write_err(message.as_bytes());
    loop {
        core::hint::spin_loop();
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn abort() -> ! {
    panic_abort("blueprint abort\n")
}

#[cfg(feature = "default-panic-handler")]
#[panic_handler]
fn default_panic(_info: &core::panic::PanicInfo<'_>) -> ! {
    panic_abort("blueprint panic\n")
}

pub mod prelude {
    pub use crate::TrueosAllocator;
    pub use crate::logl;
    #[cfg(feature = "tokio-net-probe")]
    pub use crate::net;
    pub use crate::platform;
    #[cfg(feature = "tokio-runtime")]
    pub use crate::t;
    #[cfg(feature = "tokio-runtime")]
    pub use crate::{fs, io, runtime, sync, task, time, tokio};
}
