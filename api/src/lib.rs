#![no_std]
#[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
extern crate std;

pub extern crate alloc;
#[cfg(feature = "tokio-runtime")]
pub use tokio;

use core::alloc::{GlobalAlloc, Layout};
use core::fmt;
#[cfg(feature = "default-panic-handler")]
use core::panic::PanicInfo;
use core::ptr::null_mut;
use core::sync::atomic::{AtomicU8, Ordering};

mod vcabi {
    pub use v::vcabi::*;
}

pub mod hid;
pub use hid as input;
pub mod rand {
    pub use crate::tyche::*;
}
pub mod platform;
pub mod tyche;
pub mod ui2;
pub mod vfs;
pub mod vgfx;
pub mod vgfx_hosted;
pub mod vnet;
pub mod vshell;

pub mod diag {
    #[cfg(any(target_os = "trueos", target_os = "zkvm"))]
    use alloc::string::String;
    #[cfg(any(target_os = "trueos", target_os = "zkvm"))]
    use core::fmt::Write as _;

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
        emit_impl(level, args);
    }

    #[cfg(any(target_os = "trueos", target_os = "zkvm"))]
    fn emit_impl(level: Level, args: fmt::Arguments<'_>) {
        let mut line = String::new();
        let _ = write!(&mut line, "[trueos:{}] {}", level.as_str(), args);
        if !line.ends_with('\n') {
            line.push('\n');
        }

        let stream = match level {
            Level::Error => 2,
            Level::Warn | Level::Info | Level::Debug | Level::Trace => 1,
        };
        crate::platform::write_log_stream(stream, line.as_str());
    }

    #[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
    fn emit_impl(level: Level, args: fmt::Arguments<'_>) {
        std::eprintln!("[trueos:{}] {}", level.as_str(), args);
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

pub struct TrueosAllocator;

#[cfg(feature = "default-global-allocator")]
#[global_allocator]
static DEFAULT_GLOBAL_ALLOCATOR: TrueosAllocator = TrueosAllocator;

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
    platform::log_error(message);
    loop {
        core::hint::spin_loop();
    }
}

#[cfg(feature = "default-panic-handler")]
#[panic_handler]
fn default_panic(_info: &PanicInfo<'_>) -> ! {
    panic_abort("blueprint panic\n")
}

pub mod prelude {
    pub use crate::diag;
    #[cfg(feature = "tokio-runtime")]
    pub use crate::fs;
    #[cfg(feature = "tokio-runtime")]
    pub use crate::io;
    #[cfg(feature = "tokio-net-probe")]
    pub use crate::net;
    pub use crate::panic_abort;
    pub use crate::platform;
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
