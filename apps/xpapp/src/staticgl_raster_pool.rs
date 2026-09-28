//! Bounded two-worker dispatch for the software GL rasterizer.
//!
//! The raster core supplies owned, row-exclusive jobs.  This module keeps the
//! TRUEOS worker contract out of the pixel code: a job either runs on the
//! persistent strict-P-core pool or the caller retains its scalar path.

use std::sync::{Arc, Mutex};
use trueos::worker::{self, JoinHandle, SpawnError};

/// The two persistent performance workers are available for this draw.
/// Admission remains fallible because another bounded pair can occupy them
/// between this check and submission.
#[inline]
pub(crate) fn has_two_workers() -> bool {
    worker::compute_capacity() >= 2
}

/// Submit two owned resumable jobs.  `step` must process a finite bounded
/// amount of work and return whether it has another step.  A temporary full
/// pool is never turned into inline scalar work: admission retries using a
/// fresh closure that owns another Arc to the same job.
///
/// If the second admission fails permanently, this method still joins the
/// accepted first job before returning.  Callers only copy completed outputs
/// back after this function succeeds.
pub(crate) fn run_two<T>(
    first: T,
    second: T,
    step: fn(&mut T) -> bool,
) -> Result<(T, T), SpawnError>
where
    T: Send + 'static,
{
    debug_assert!(has_two_workers());
    let first_slot = Arc::new(Mutex::new(Some(first)));
    let second_slot = Arc::new(Mutex::new(Some(second)));
    let first_handle = submit_retry(first_slot.clone(), step)?;
    let second_handle = match submit_retry(second_slot.clone(), step) {
        Ok(handle) => handle,
        Err(error) => {
            // The first closure may still be in its duty cooldown.  Drain it
            // before dropping the last Arc that owns its output buffers.
            let _ = join(first_handle);
            return Err(error);
        }
    };

    // Always wait for both handles.  A completion-channel error must not let
    // its sibling retain an Arc to buffers that a caller is about to discard.
    let first_result = join(first_handle);
    let second_result = join(second_handle);
    first_result?;
    second_result?;
    Ok((take_slot(&first_slot), take_slot(&second_slot)))
}

fn submit_retry<T>(
    slot: Arc<Mutex<Option<T>>>,
    step: fn(&mut T) -> bool,
) -> Result<JoinHandle<()>, SpawnError>
where
    T: Send + 'static,
{
    loop {
        if worker::cancellation_requested() {
            return Err(SpawnError::Unavailable);
        }
        let slot_for_worker = slot.clone();
        // The input remains in `slot` until the accepted closure first runs.
        // Rejected submissions consume only this closure, retaining the owned
        // task for retry. Every later 8-row step uses worker-local state.
        let mut task = None;
        match worker::spawn_compute_resumable(move || {
            if task.is_none() {
                task = slot_for_worker
                    .lock()
                    .expect("raster band mutex poisoned")
                    .take();
            }
            let more = step(task.as_mut().expect("raster band task missing"));
            if !more {
                *slot_for_worker.lock().expect("raster band mutex poisoned") = task.take();
            }
            more
        }) {
            Ok(handle) => return Ok(handle),
            Err(SpawnError::Unavailable) => yield_executor(),
            Err(error) => return Err(error),
        }
    }
}

fn take_slot<T>(slot: &Arc<Mutex<Option<T>>>) -> T {
    slot.lock()
        .expect("raster band mutex poisoned")
        .take()
        .expect("raster worker completed without output")
}

#[inline]
fn join(handle: JoinHandle<()>) -> Result<(), SpawnError> {
    handle.join_blocking().map_err(|_| SpawnError::Transport)
}

#[inline]
fn yield_executor() {
    #[cfg(any(target_os = "trueos", target_os = "zkvm"))]
    trueos::platform::poll_once();
    #[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
    std::thread::yield_now();
}
