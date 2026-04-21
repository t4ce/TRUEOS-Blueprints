//! Tokio-first blueprint surface.
//!
//! This crate intentionally exports only the Tokio-side runtime ideas we are
//! aligning new blueprints around. Kernel-facing capability wrappers are not
//! reintroduced here.

use std::fmt;
use std::sync::atomic::{AtomicU8, Ordering};

pub use tokio;

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
        eprintln!("[trueos-blueprint:{}] {}", level.as_str(), args);
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

pub mod runtime {
    pub use tokio::runtime::{Builder, Handle, Runtime};
}

pub mod task {
    pub use tokio::spawn;
    pub use tokio::task::{JoinError, JoinHandle, JoinSet, LocalSet, yield_now};
}

pub mod sync {
    pub use tokio::sync::{
        Barrier, Mutex, Notify, RwLock, Semaphore, broadcast, mpsc, oneshot, watch,
    };
}

pub mod time {
    pub use tokio::time::{Duration, Instant, Interval, Sleep, interval, sleep, timeout};
}

pub mod io {
    pub use tokio::io::{
        AsyncBufReadExt, AsyncReadExt, AsyncSeekExt, AsyncWriteExt, Stderr, Stdin, Stdout,
        duplex, stderr, stdin, stdout,
    };
}

pub mod fs {
    pub use tokio::fs::{File, OpenOptions, create_dir, create_dir_all, read, read_to_string, write};
}

pub mod prelude {
    pub use crate::diag;
    pub use crate::fs;
    pub use crate::io;
    pub use crate::runtime;
    pub use crate::sync;
    pub use crate::task;
    pub use crate::time;
    pub use crate::tokio;
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