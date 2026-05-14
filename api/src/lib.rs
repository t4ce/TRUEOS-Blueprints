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
pub mod globalog;
pub mod logl;
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
    use super::{AtomicU8, Ordering, fmt};

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
        File, OpenOptions, canonicalize, create_dir, create_dir_all, read, read_to_string,
        try_exists, write,
    };

    pub use crate::vfs::{FsNodeKind, FsStat, stat};
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
    logl::log(logl::level::ERROR, message);
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
    pub use crate::TrueosAllocator;
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
    #[cfg(feature = "tokio-runtime")]
    pub use crate::task;
    #[cfg(feature = "tokio-runtime")]
    pub use crate::time;
    #[cfg(feature = "tokio-runtime")]
    pub use crate::tokio;
    pub use crate::ui2;
    pub use crate::vgfx;
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
