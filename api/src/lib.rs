#![no_std]
#[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
extern crate std;

pub extern crate alloc;
#[cfg(feature = "tokio-runtime")]
pub use tokio;

use core::alloc::{GlobalAlloc, Layout};
use core::ffi::c_void;
use core::fmt;
#[cfg(feature = "default-panic-handler")]
use core::panic::PanicInfo;
use core::ptr::null_mut;
use core::sync::atomic::{AtomicU8, Ordering};

mod vcabi {
    pub use v::vcabi::*;
}

unsafe extern "C" {
    fn posix_memalign(memptr: *mut *mut c_void, align: usize, size: usize) -> i32;
    fn free(ptr: *mut c_void);
}

pub mod hid;
pub use hid as input;
pub mod globalog;
pub mod clock;
pub mod logl;
pub mod rand {
    pub use crate::tyche::*;
}
pub mod platform;
#[cfg(feature = "tokio-runtime")]
pub mod std_abi;
pub mod tyche;
pub mod ui2;
pub mod vfs;
pub mod vgfx;
pub mod vgfx_hosted;
pub mod vnet;
pub mod vshell;

pub mod diag {
    use super::{fmt, AtomicU8, Ordering};

    #[derive(Copy, Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
    pub enum Level {
        Error = 1,
        Warn = 2,
        Info = 3,
        Debug = 4,
        Trace = 5,
    }

    impl Level {
        pub const fn as_str(self) -> &'static str {
            match self {
                Self::Error => "ERROR",
                Self::Warn => "WARN",
                Self::Info => "INFO",
                Self::Debug => "DEBUG",
                Self::Trace => "TRACE",
            }
        }
    }

    static MAX_LEVEL: AtomicU8 = AtomicU8::new(Level::Info as u8);

    pub fn set_max_level(level: Level) {
        MAX_LEVEL.store(level as u8, Ordering::Release);
    }

    pub fn enabled(level: Level) -> bool {
        (level as u8) <= MAX_LEVEL.load(Ordering::Acquire)
    }

    pub fn emit(level: Level, args: fmt::Arguments<'_>) {
        if !enabled(level) {
            return;
        }
        crate::globalog::log_with_concept_level("trueos", level.into(), args);
    }

    pub fn error(message: &str) {
        emit(Level::Error, format_args!("{message}"));
    }

    pub fn warn(message: &str) {
        emit(Level::Warn, format_args!("{message}"));
    }

    pub fn info(message: &str) {
        emit(Level::Info, format_args!("{message}"));
    }

    pub fn debug(message: &str) {
        emit(Level::Debug, format_args!("{message}"));
    }

    pub fn trace(message: &str) {
        emit(Level::Trace, format_args!("{message}"));
    }

    impl From<Level> for crate::globalog::Level {
        fn from(level: Level) -> Self {
            match level {
                Level::Error => Self::Error,
                Level::Warn => Self::Warn,
                Level::Info => Self::Info,
                Level::Debug => Self::Debug,
                Level::Trace => Self::Trace,
            }
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
    pub use tokio::task::{yield_now, JoinError, JoinHandle, JoinSet, LocalSet};
}

#[cfg(feature = "tokio-runtime")]
pub mod sync {
    pub use tokio::sync::{
        broadcast, mpsc, oneshot, watch, Barrier, Mutex, Notify, RwLock, Semaphore,
    };
}

#[cfg(feature = "tokio-runtime")]
pub mod time {
    pub use tokio::time::{interval, sleep, timeout, Duration, Instant, Interval, Sleep};
}

#[cfg(feature = "tokio-runtime")]
pub mod io {
    pub use tokio::io::{
        duplex, stderr, stdin, stdout, AsyncBufReadExt, AsyncReadExt, AsyncSeekExt, AsyncWriteExt,
        Stderr, Stdin, Stdout,
    };
}

#[cfg(feature = "tokio-runtime")]
pub mod fs {
    pub use tokio::fs::{
        canonicalize, create_dir, create_dir_all, read, read_to_string, try_exists, write, File,
        OpenOptions,
    };

    pub use crate::vfs::{stat, FsNodeKind, FsStat};
}

#[cfg(feature = "tokio-net-probe")]
pub mod net {
    pub use tokio::net::{lookup_host, TcpListener, TcpStream, ToSocketAddrs, UdpSocket};

    pub mod mio {
        pub use mio::{event, net};
        pub use mio::{Events, Interest, Poll, Registry, Token, Waker};
    }

    pub mod socket2 {
        pub use socket2::{Domain, Protocol, SockAddr, Socket, Type};
    }
}

pub mod t {
    #[cfg(feature = "tokio-runtime")]
    pub use crate::fs;
    #[cfg(feature = "tokio-runtime")]
    pub use crate::io;
    #[cfg(feature = "tokio-net-probe")]
    pub use crate::net;
    #[cfg(feature = "tokio-runtime")]
    pub use crate::runtime;
    #[cfg(feature = "tokio-runtime")]
    pub use crate::sync;
    #[cfg(feature = "tokio-runtime")]
    pub use crate::task;
    #[cfg(feature = "tokio-runtime")]
    pub use crate::time;
    #[cfg(feature = "tokio-runtime")]
    pub use crate::tokio;
}

pub struct TrueosAllocator;

#[cfg(feature = "default-global-allocator")]
#[global_allocator]
static DEFAULT_GLOBAL_ALLOCATOR: TrueosAllocator = TrueosAllocator;

struct AllocDiagLine {
    buf: [u8; 192],
    len: usize,
}

impl AllocDiagLine {
    const fn new() -> Self {
        Self {
            buf: [0; 192],
            len: 0,
        }
    }

    fn write_to_stream(&self) {
        if self.len != 0 {
            unsafe { vcabi::trueos_cabi_write(2, self.buf.as_ptr(), self.len) }
        }
    }
}

impl fmt::Write for AllocDiagLine {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        let remaining = self.buf.len().saturating_sub(self.len);
        let bytes = s.as_bytes();
        let copy_len = core::cmp::min(remaining, bytes.len());
        if copy_len != 0 {
            self.buf[self.len..self.len + copy_len].copy_from_slice(&bytes[..copy_len]);
            self.len += copy_len;
        }
        Ok(())
    }
}

fn log_alloc_null(op: &str, reason: &str, size: usize, align: usize, new_size: usize) {
    let mut line = AllocDiagLine::new();
    let _ = fmt::write(
        &mut line,
        format_args!(
            "[blueprint:ERROR] alloc-null op={} reason={} size={} align={} new_size={}\n",
            op, reason, size, align, new_size
        ),
    );
    line.write_to_stream();
}

fn log_abort_entry(kind: &str) {
    let mut sp = 0usize;
    let mut fp = 0usize;

    #[cfg(target_arch = "x86_64")]
    unsafe {
        core::arch::asm!("mov {}, rsp", out(reg) sp, options(nomem, nostack, preserves_flags));
        core::arch::asm!("mov {}, rbp", out(reg) fp, options(nomem, nostack, preserves_flags));
    }

    let stack = sp as *const usize;
    let s0 = unsafe { stack.add(0).read_volatile() };
    let s1 = unsafe { stack.add(1).read_volatile() };
    let s2 = unsafe { stack.add(2).read_volatile() };
    let s3 = unsafe { stack.add(3).read_volatile() };

    let mut line = AllocDiagLine::new();
    let _ = fmt::write(
        &mut line,
        format_args!(
            "[blueprint:ERROR] abort-entry kind={} sp=0x{:016X} fp=0x{:016X} stack=[0x{:016X},0x{:016X},0x{:016X},0x{:016X}]\n",
            kind, sp, fp, s0, s1, s2, s3
        ),
    );
    line.write_to_stream();
}

fn allocator_base_align() -> usize {
    core::mem::align_of::<usize>()
}

unsafe fn alloc_aligned(op: &str, layout: Layout, new_size: usize) -> *mut u8 {
    let mut ptr = null_mut::<c_void>();
    let rc = unsafe { posix_memalign(&mut ptr, layout.align(), layout.size().max(1)) };
    if rc != 0 || ptr.is_null() {
        log_alloc_null(op, "posix-memalign-failed", layout.size(), layout.align(), new_size);
        return null_mut();
    }
    ptr.cast::<u8>()
}

// The thin blueprint path uses the host-exported C allocator directly.
unsafe impl GlobalAlloc for TrueosAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if layout.align() > allocator_base_align() {
            return unsafe { alloc_aligned("alloc", layout, 0) };
        }
        let ptr = unsafe { vcabi::trueos_cabi_alloc(layout.size().max(1)) };
        if ptr.is_null() {
            log_alloc_null("alloc", "backend-null", layout.size(), layout.align(), 0);
        }
        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        if ptr.is_null() {
            return;
        }
        if layout.align() > allocator_base_align() {
            unsafe { free(ptr.cast::<c_void>()) }
        } else {
            unsafe { vcabi::trueos_cabi_free(ptr) }
        }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let ptr = if layout.align() > allocator_base_align() {
            unsafe { alloc_aligned("alloc_zeroed", layout, 0) }
        } else {
            unsafe { vcabi::trueos_cabi_calloc(1, layout.size().max(1)) }
        };
        if ptr.is_null() {
            log_alloc_null("alloc_zeroed", "backend-null", layout.size(), layout.align(), 0);
        } else if layout.align() > allocator_base_align() {
            unsafe { core::ptr::write_bytes(ptr, 0, layout.size().max(1)) };
        }
        ptr
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        if ptr.is_null() {
            return unsafe { self.alloc(layout) };
        }
        if layout.align() > allocator_base_align() {
            let Some(new_layout) = Layout::from_size_align(new_size.max(1), layout.align()).ok()
            else {
                log_alloc_null(
                    "realloc",
                    "layout-invalid",
                    layout.size(),
                    layout.align(),
                    new_size,
                );
                return null_mut();
            };
            let new_ptr = unsafe { alloc_aligned("realloc", new_layout, new_size) };
            if new_ptr.is_null() {
                log_alloc_null("realloc", "backend-null", layout.size(), layout.align(), new_size);
                return null_mut();
            }
            unsafe {
                core::ptr::copy_nonoverlapping(
                    ptr,
                    new_ptr,
                    core::cmp::min(layout.size(), new_size),
                );
                free(ptr.cast::<c_void>());
            }
            return new_ptr;
        }
        let new_ptr = unsafe { vcabi::trueos_cabi_realloc(ptr, new_size.max(1)) };
        if new_ptr.is_null() {
            log_alloc_null("realloc", "backend-null", layout.size(), layout.align(), new_size);
        }
        new_ptr
    }
}

pub fn panic_abort(message: &str) -> ! {
    logl::log(logl::level::ERROR, message);
    loop {
        core::hint::spin_loop();
    }
}

type UnwindReasonCode = i32;
const UNWIND_END_OF_STACK: UnwindReasonCode = 5;

#[unsafe(no_mangle)]
pub extern "C" fn abort() -> ! {
    log_abort_entry("abort");
    panic_abort("blueprint abort\n")
}

#[unsafe(no_mangle)]
pub extern "C" fn _Unwind_Backtrace(
    _trace: extern "C" fn(*mut c_void, *mut c_void) -> UnwindReasonCode,
    _trace_argument: *mut c_void,
) -> UnwindReasonCode {
    UNWIND_END_OF_STACK
}

#[unsafe(no_mangle)]
pub extern "C" fn _Unwind_GetIP(_context: *mut c_void) -> usize {
    0
}

#[cfg(feature = "default-panic-handler")]
#[panic_handler]
fn default_panic(_info: &PanicInfo<'_>) -> ! {
    log_abort_entry("panic-handler");
    panic_abort("blueprint panic\n")
}

pub mod prelude {
    pub use crate::diag;
    #[cfg(feature = "tokio-runtime")]
    pub use crate::fs;
    pub use crate::globalog;
    #[cfg(feature = "tokio-runtime")]
    pub use crate::io;
    pub use crate::logl;
    #[cfg(feature = "tokio-net-probe")]
    pub use crate::net;
    pub use crate::panic_abort;
    pub use crate::platform;
    #[cfg(feature = "tokio-runtime")]
    pub use crate::runtime;
    #[cfg(feature = "tokio-runtime")]
    pub use crate::sync;
    pub use crate::t;
    #[cfg(feature = "tokio-runtime")]
    pub use crate::task;
    #[cfg(feature = "tokio-runtime")]
    pub use crate::time;
    #[cfg(feature = "tokio-runtime")]
    pub use crate::tokio;
    pub use crate::ui2;
    pub use crate::vgfx;
    pub use crate::TrueosAllocator;
}

#[macro_export]
macro_rules! bp_info {
    ($($arg:tt)*) => {
        $crate::diag::emit($crate::diag::Level::Info, format_args!($($arg)*))
    };
}

#[macro_export]
macro_rules! log {
    ($msg:expr, error) => {
        $crate::diag::emit($crate::diag::Level::Error, format_args!("{}", $msg))
    };
    ($msg:expr, warn) => {
        $crate::diag::emit($crate::diag::Level::Warn, format_args!("{}", $msg))
    };
    ($msg:expr, info) => {
        $crate::diag::emit($crate::diag::Level::Info, format_args!("{}", $msg))
    };
    ($msg:expr, debug) => {
        $crate::diag::emit($crate::diag::Level::Debug, format_args!("{}", $msg))
    };
    ($msg:expr, trace) => {
        $crate::diag::emit($crate::diag::Level::Trace, format_args!("{}", $msg))
    };
    ($msg:expr) => {
        $crate::diag::emit($crate::diag::Level::Trace, format_args!("{}", $msg))
    };
}

#[macro_export]
macro_rules! bp_warn {
    ($($arg:tt)*) => {
        $crate::diag::emit($crate::diag::Level::Warn, format_args!($($arg)*))
    };
}

#[macro_export]
macro_rules! bp_debug {
    ($($arg:tt)*) => {
        $crate::diag::emit($crate::diag::Level::Debug, format_args!($($arg)*))
    };
}

#[macro_export]
macro_rules! bp_trace {
    ($($arg:tt)*) => {
        $crate::diag::emit($crate::diag::Level::Trace, format_args!($($arg)*))
    };
}

#[macro_export]
macro_rules! bp_error {
    ($($arg:tt)*) => {
        $crate::diag::emit($crate::diag::Level::Error, format_args!($($arg)*))
    };
}
