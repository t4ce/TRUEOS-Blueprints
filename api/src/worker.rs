//! Explicit native work, independent of std and Tokio thread-pool lifecycle.
//!
//! Build and drop a current-thread Runtime inside the submitted closure. Each
//! simultaneous job owns a native lane and a distinct worker-local slot; slots
//! may be reused by later jobs, so TLS is worker-local, not fresh thread-local
//! storage per submission. Work must terminate cooperatively. Dropping a join
//! handle detaches it; it cannot cancel a running closure. The kernel retains
//! the Blueprint's resources until all accepted native jobs finish.

use alloc::boxed::Box;
use core::{
    fmt,
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};
use tokio::sync::oneshot;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SpawnError {
    /// No lane is free, or Blueprint admission is closed during teardown.
    Unavailable,
    InvalidJob,
    Transport,
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
        Pin::new(&mut self.receiver)
            .poll(cx)
            .map(|result| result.map_err(|_| JoinError))
    }
}

impl<R> JoinHandle<R> {
    /// Returns a completed native result without polling the caller's async
    /// runtime.  CPU raster providers use this to join finite row bands while
    /// keeping their existing synchronous dispatch contract.
    pub fn try_take(&mut self) -> Option<Result<R, JoinError>> {
        match self.receiver.try_recv() {
            Ok(result) => Some(Ok(result)),
            Err(tokio::sync::oneshot::error::TryRecvError::Empty) => None,
            Err(tokio::sync::oneshot::error::TryRecvError::Closed) => Some(Err(JoinError)),
        }
    }

    /// Wait for a finite native job without spinning.  Each incomplete poll
    /// yields the current TRUEOS executor; host tests use the normal scheduler
    /// yield.  Submit only bounded bands and always keep a serial fallback.
    pub fn join_blocking(mut self) -> Result<R, JoinError> {
        loop {
            if let Some(result) = self.try_take() {
                return result;
            }
            #[cfg(any(target_os = "trueos", target_os = "zkvm"))]
            v::vsys::poll_once();
            #[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
            std::thread::yield_now();
        }
    }
}

/// Advisory count of currently available native service lanes. It may be zero;
/// concurrent submissions can consume this capacity before `spawn` is called.
pub fn capacity() -> usize {
    #[cfg(any(target_os = "trueos", target_os = "zkvm"))]
    {
        unsafe { v::worker_abi::trueos_service_lane_available_capacity() }
    }
    #[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
    {
        std::thread::available_parallelism().map_or(0, |count| count.get())
    }
}

/// True when this Blueprint's native-job admission has closed for teardown.
/// Check even while idle: a forced VM stop does not run application destructors.
/// Finish any accepted device work before returning; cancellation never frees
/// storage still referenced by hardware. Host jobs return false here.
pub fn cancellation_requested() -> bool {
    #[cfg(any(target_os = "trueos", target_os = "zkvm"))]
    {
        unsafe { v::worker_abi::trueos_service_lane_cancellation_requested() }
    }
    #[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
    {
        false
    }
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
    spawn_with(f, submit)
}

/// Submit finite CPU work to XPAPP's sticky two-worker strict-P-core pool.
/// This is unavailable outside TRUEOS so host raster parity tests stay scalar
/// and deterministic.
pub fn spawn_compute<F, R>(f: F) -> Result<JoinHandle<R>, SpawnError>
where
    F: FnOnce() -> R + Send + 'static,
    R: Send + 'static,
{
    #[cfg(any(target_os = "trueos", target_os = "zkvm"))]
    {
        let (sender, receiver) = oneshot::channel();
        let mut f = Some(f);
        let mut sender = Some(sender);
        let job: Box<dyn FnMut() -> bool + Send + 'static> = Box::new(move || {
            let result = f.take().expect("one-shot compute job invoked twice")();
            let _ = sender
                .take()
                .expect("one-shot compute completion")
                .send(result);
            false
        });
        submit_compute(job)?;
        Ok(JoinHandle { receiver })
    }
    #[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
    {
        let _ = f;
        Err(SpawnError::Unavailable)
    }
}

/// Submit a resumable finite compute job. Each call to `step` must finish a
/// bounded unit of work and return `true` while more work remains. The kernel
/// retains the same job on its selected P worker through its 4 ms work burst
/// and the following duty cooldown; it sends completion after `false`.
pub fn spawn_compute_resumable<F>(step: F) -> Result<JoinHandle<()>, SpawnError>
where
    F: FnMut() -> bool + Send + 'static,
{
    #[cfg(any(target_os = "trueos", target_os = "zkvm"))]
    {
        let (sender, receiver) = oneshot::channel();
        let mut step = step;
        let mut sender = Some(sender);
        let job: Box<dyn FnMut() -> bool + Send + 'static> = Box::new(move || {
            let more = step();
            if !more {
                let _ = sender
                    .take()
                    .expect("resumable compute completion")
                    .send(());
            }
            more
        });
        submit_compute(job)?;
        Ok(JoinHandle { receiver })
    }
    #[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
    {
        let _ = step;
        Err(SpawnError::Unavailable)
    }
}

fn submit_compute(job: Box<dyn FnMut() -> bool + Send + 'static>) -> Result<(), SpawnError> {
    #[cfg(any(target_os = "trueos", target_os = "zkvm"))]
    let code = unsafe { v::worker_abi::trueos_guest_compute_submit_job(job) };
    #[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
    let code = {
        drop(job);
        -2
    };
    match code {
        0 => Ok(()),
        -2 => Err(SpawnError::Unavailable),
        -5 => Err(SpawnError::InvalidJob),
        -6 => Err(SpawnError::Transport),
        other => Err(SpawnError::Unknown(other)),
    }
}

/// Runtime number of strict performance workers the two-worker pool can use.
/// It is advisory; an active Hull/native lane may still make a submission fail.
pub fn compute_capacity() -> usize {
    #[cfg(any(target_os = "trueos", target_os = "zkvm"))]
    {
        unsafe { v::worker_abi::trueos_guest_compute_capacity() }
    }
    #[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
    {
        0
    }
}

fn spawn_with<F, R>(
    f: F,
    submit: impl FnOnce(Box<dyn FnOnce() + Send + 'static>) -> i32,
) -> Result<JoinHandle<R>, SpawnError>
where
    F: FnOnce() -> R + Send + 'static,
    R: Send + 'static,
{
    let (sender, receiver) = oneshot::channel();
    let job: Box<dyn FnOnce() + Send + 'static> = Box::new(move || {
        let result = f(); // closure-owned runtime/enter guards drop before send
        let _ = sender.send(result);
    });
    match submit(job) {
        0 => {}
        -2 => return Err(SpawnError::Unavailable),
        -5 => return Err(SpawnError::InvalidJob),
        -6 => return Err(SpawnError::Transport),
        code => return Err(SpawnError::Unknown(code)),
    }
    Ok(JoinHandle { receiver })
}

fn submit(job: Box<dyn FnOnce() + Send + 'static>) -> i32 {
    #[cfg(any(target_os = "trueos", target_os = "zkvm"))]
    {
        unsafe { v::worker_abi::trueos_service_lane_submit_job(job) }
    }
    #[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
    {
        // Host-side use keeps native system threads; the TRUEOS target never
        // takes this branch or manufactures pthread imports from it.
        match std::thread::Builder::new().spawn(job) {
            Ok(_) => 0,
            Err(_) => -2,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::sync::Arc;
    use core::sync::atomic::{AtomicUsize, Ordering};

    struct CountDrop(Arc<AtomicUsize>);
    impl Drop for CountDrop {
        fn drop(&mut self) {
            self.0.fetch_add(1, Ordering::AcqRel);
        }
    }

    #[test]
    fn rejection_drops_capture_once_and_does_not_execute_it() {
        let drops = Arc::new(AtomicUsize::new(0));
        let capture = CountDrop(drops.clone());
        let result = spawn_with(
            move || -> () {
                drop(capture);
                panic!("rejected work ran");
            },
            |job| {
                drop(job);
                -2
            },
        );
        assert!(matches!(result, Err(SpawnError::Unavailable)));
        assert_eq!(drops.load(Ordering::Acquire), 1);
    }

    #[test]
    fn dropping_join_handle_keeps_accepted_work_owned_by_submitter() {
        let drops = Arc::new(AtomicUsize::new(0));
        let capture = CountDrop(drops.clone());
        let mut queued = None;
        let handle = spawn_with(
            move || {
                drop(capture);
                7
            },
            |job| {
                queued = Some(job);
                0
            },
        )
        .unwrap();
        drop(handle);
        assert_eq!(drops.load(Ordering::Acquire), 0);
        queued.unwrap()();
        assert_eq!(drops.load(Ordering::Acquire), 1);
    }

    #[test]
    fn completion_contains_the_returned_value() {
        let mut handle = spawn_with(
            || 13,
            |job| {
                job();
                0
            },
        )
        .unwrap();
        assert_eq!(handle.receiver.try_recv().unwrap(), 13);
    }

    #[test]
    fn resumable_completion_sender_is_moved_only_after_the_final_step() {
        let (sender, mut receiver) = oneshot::channel();
        let mut sender = Some(sender);
        let mut steps = 0usize;
        let mut job: Box<dyn FnMut() -> bool + Send> = Box::new(move || {
            steps += 1;
            let more = steps < 3;
            if !more {
                let _ = sender.take().expect("one completion").send(());
            }
            more
        });
        assert!(job());
        assert!(job());
        assert!(!job());
        assert_eq!(receiver.try_recv().unwrap(), ());
    }

    #[test]
    fn host_resumable_rejection_drops_capture_once_without_running_step() {
        let drops = Arc::new(AtomicUsize::new(0));
        let capture = CountDrop(drops.clone());
        let result = spawn_compute_resumable(move || {
            let _ = &capture;
            panic!("unavailable compute job ran")
        });
        assert!(matches!(result, Err(SpawnError::Unavailable)));
        assert_eq!(drops.load(Ordering::Acquire), 1);
    }
}
