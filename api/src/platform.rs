extern crate alloc;

use crate::vcabi;

pub use alloc::borrow::{Cow, ToOwned};
pub use alloc::boxed::Box;
pub use alloc::format;
pub use alloc::string::{String, ToString};
pub use alloc::sync::Arc;
pub use alloc::vec;
pub use alloc::vec::Vec;

type TrueosBlockingJob = Box<dyn FnOnce() + Send + 'static>;

unsafe extern "Rust" {
    fn trueos_tokio_spawn_blocking_job(job: TrueosBlockingJob) -> i32;
}

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
pub fn spawn_blocking(job: impl FnOnce() + Send + 'static) -> Result<(), i32> {
    let rc = unsafe { trueos_tokio_spawn_blocking_job(Box::new(job)) };
    if rc == 0 { Ok(()) } else { Err(rc) }
}

#[inline]
pub fn write_stream(stream: u32, bytes: &[u8]) {
    if bytes.is_empty() {
        return;
    }
    unsafe { vcabi::trueos_cabi_write(stream, bytes.as_ptr(), bytes.len()) }
}
