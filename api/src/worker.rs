//! Explicit native work, independent of std and Tokio thread-pool lifecycle.
//!
//! Build and drop a current-thread Runtime inside the submitted closure. Each
//! simultaneous job owns a native lane and a distinct worker-local slot; slots
//! may be reused by later jobs, so TLS is worker-local, not fresh thread-local
//! storage per submission. Work must terminate cooperatively. Dropping a join
//! handle detaches it; it cannot cancel a running closure. The kernel retains
//! the Blueprint's resources until all accepted native jobs finish.

use alloc::boxed::Box;
use core::{fmt, future::Future, pin::Pin, task::{Context, Poll}};
use tokio::sync::oneshot;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SpawnError {
    /// No lane is free, or Blueprint admission is closed during teardown.
    Unavailable,
    InvalidJob,
    Transport,
    Unsupported,
    Unknown(i32),
}

impl fmt::Display for SpawnError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "native worker submission failed: {self:?}")
    }
}
impl core::error::Error for SpawnError {}

/// Completion was dropped without returning a result. This is not a Tokio
/// panic payload or an assertion that native code was successfully cancelled.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct JoinError;

impl fmt::Display for JoinError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("native worker completion dropped")
    }
}
impl core::error::Error for JoinError {}

#[must_use = "await native work to observe completion; dropping the handle detaches it"]
pub struct JoinHandle<R> {
    receiver: oneshot::Receiver<R>,
}

impl<R> Future for JoinHandle<R> {
    type Output = Result<R, JoinError>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        Pin::new(&mut self.receiver).poll(cx).map(|result| result.map_err(|_| JoinError))
    }
}

/// Advisory count of currently available native service lanes. It may be zero;
/// concurrent submissions can consume this capacity before `spawn` is called.
pub fn capacity() -> usize {
    #[cfg(any(target_os = "trueos", target_os = "zkvm"))]
    { unsafe { v::worker_abi::trueos_service_lane_available_capacity() } }
    #[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
    { std::thread::available_parallelism().map_or(0, |count| count.get()) }
}

/// Stable worker-local identity while a native closure runs. The coordinating
/// Blueprint runtime has its own slot; this is not an OS thread identifier.
#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
pub fn local_slot() -> u32 {
    unsafe { v::bp_abi::trueos_cabi_wls_current_slot() }
}

/// Submission consumes the closure even on rejection. No closure is executed
/// inline as a fallback when native capacity is unavailable.
pub fn spawn<F, R>(f: F) -> Result<JoinHandle<R>, SpawnError>
where
    F: FnOnce() -> R + Send + 'static,
    R: Send + 'static,
{
    let (sender, receiver) = oneshot::channel();
    let job: Box<dyn FnOnce() + Send + 'static> = Box::new(move || {
        let result = f(); // closure-owned runtime/enter guards drop before send
        let _ = sender.send(result);
    });
    #[cfg(any(target_os = "trueos", target_os = "zkvm"))]
    {
        match unsafe { v::worker_abi::trueos_service_lane_submit_job(job) } {
            0 => {}
            -2 => return Err(SpawnError::Unavailable),
            -5 => return Err(SpawnError::InvalidJob),
            -6 => return Err(SpawnError::Transport),
            code => return Err(SpawnError::Unknown(code)),
        }
    }
    #[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
    {
        // Host-side use keeps native system threads; the TRUEOS target never
        // takes this branch or manufactures pthread imports from it.
        std::thread::Builder::new().spawn(job).map_err(|_| SpawnError::Unavailable)?;
    }
    Ok(JoinHandle { receiver })
}
