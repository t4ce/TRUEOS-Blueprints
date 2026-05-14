extern crate alloc;

use alloc::string::String;
use core::fmt;
use core::fmt::Write as _;

use crate::vcabi;

pub use alloc::borrow::{Cow, ToOwned};
pub use alloc::boxed::Box;
pub use alloc::format;
pub use alloc::string::ToString;
pub use alloc::sync::Arc;
pub use alloc::vec;
pub use alloc::vec::Vec;

pub mod future {
    pub use core::future::{Future, IntoFuture, pending, poll_fn};
}

#[cfg(feature = "tokio-runtime")]
pub mod io {
    pub use tokio::io::{Error, ErrorKind, Result, SeekFrom};
}

#[cfg(feature = "tokio-runtime")]
pub mod path {
    #[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
    pub use std::path::{Component, Components, Path, PathBuf};
    #[cfg(any(target_os = "trueos", target_os = "zkvm"))]
    pub use tokio::path::{Component, Components, Path, PathBuf};
}

#[cfg(feature = "tokio-runtime")]
pub mod thread {
    #[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
    pub use std::thread::{Thread, ThreadId, current};
    #[cfg(any(target_os = "trueos", target_os = "zkvm"))]
    pub use tokio::thread::{Thread, ThreadId, current};

    #[inline]
    pub fn yield_now() {
        #[cfg(any(target_os = "trueos", target_os = "zkvm"))]
        {
            super::poll_once();
        }
        #[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
        {
            std::thread::yield_now();
        }
    }
}

#[inline]
pub fn poll_once() {
    unsafe { vcabi::trueos_cabi_poll_once() }
}

#[inline]
pub fn sleep_ms(ms: u64) {
    unsafe { vcabi::trueos_cabi_sleep_ms(ms) }
}

#[inline]
pub fn write_stream(stream: u32, bytes: &[u8]) {
    if bytes.is_empty() {
        return;
    }
    unsafe { vcabi::trueos_cabi_write(stream, bytes.as_ptr(), bytes.len()) }
}

#[inline]
pub fn write_log_stream(stream: u32, s: &str) {
    write_stream(stream, s.as_bytes());
}

#[inline]
pub fn log_info(s: &str) {
    write_log_stream(1, s);
}

#[inline]
pub fn log_error(s: &str) {
    write_log_stream(2, s);
}

#[inline]
pub fn log_infof(args: fmt::Arguments<'_>) {
    logf(1, args);
}

#[inline]
pub fn log_errorf(args: fmt::Arguments<'_>) {
    logf(2, args);
}

#[inline]
pub fn log_info_with_args(prefix: &str, args: &[&str]) {
    log_with_args(1, prefix, args);
}

#[inline]
pub fn log_error_with_args(prefix: &str, args: &[&str]) {
    log_with_args(2, prefix, args);
}

fn log_with_args(stream: u32, prefix: &str, args: &[&str]) {
    let mut line = String::from(prefix);
    if args.is_empty() {
        line.push_str(" args=(none)\n");
    } else {
        line.push_str(" args=");
        for (idx, arg) in args.iter().enumerate() {
            if idx != 0 {
                line.push(' ');
            }
            line.push_str(arg);
        }
        line.push('\n');
    }

    write_log_stream(stream, line.as_str());
}

fn logf(stream: u32, args: fmt::Arguments<'_>) {
    let mut line = String::new();
    let _ = line.write_fmt(args);
    if !line.ends_with('\n') {
        line.push('\n');
    }
    write_log_stream(stream, line.as_str());
}
