//! Tokio-first blueprint surface.
//!
//! This crate intentionally exports only the Tokio-side runtime ideas we are
//! aligning new blueprints around. Kernel-facing capability wrappers are not
//! reintroduced here.

#![no_std]

extern crate alloc;
#[cfg(not(target_os = "zkvm"))]
extern crate std;

use core::fmt;
use core::sync::atomic::{AtomicU8, Ordering};

#[cfg(feature = "tokio-runtime")]
pub use tokio;

pub mod diag {
    use super::{AtomicU8, Ordering, fmt};
    #[cfg(target_os = "zkvm")]
    use alloc::string::String;
    #[cfg(target_os = "zkvm")]
    use core::fmt::Write as _;

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

    #[cfg(target_os = "zkvm")]
    fn emit_impl(level: Level, args: fmt::Arguments<'_>) {
        let mut line = String::new();
        let _ = write!(&mut line, "[trueos-blueprint:{}] {}", level.as_str(), args);
        if !line.ends_with('\n') {
            line.push('\n');
        }

        let stream = match level {
            Level::Error => 2,
            Level::Warn | Level::Info | Level::Debug | Level::Trace => 1,
        };
        trueos::vsys::write_log_stream(stream, line.as_str());
    }

    #[cfg(not(target_os = "zkvm"))]
    fn emit_impl(level: Level, args: fmt::Arguments<'_>) {
        std::eprintln!("[trueos-blueprint:{}] {}", level.as_str(), args);
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
        File, OpenOptions, create_dir, create_dir_all, read, read_to_string, try_exists, write,
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

pub mod prelude {
    pub use crate::diag;
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
macro_rules! bp_error {
    ($($arg:tt)*) => {
        $crate::diag::emit($crate::diag::Level::Error, format_args!($($arg)*))
    };
}

#[macro_export]
macro_rules! bp_warn {
    ($($arg:tt)*) => {
        $crate::diag::emit($crate::diag::Level::Warn, format_args!($($arg)*))
    };
}

#[macro_export]
macro_rules! bp_info {
    ($($arg:tt)*) => {
        $crate::diag::emit($crate::diag::Level::Info, format_args!($($arg)*))
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
