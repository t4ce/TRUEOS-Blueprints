//! Bounded two-worker dispatch for the software GL rasterizer.
//!
//! The raster core supplies owned, row-exclusive jobs.  This module keeps the
//! TRUEOS worker contract out of the pixel code: a job either runs on the
//! persistent strict-P-core pool or the caller retains its scalar path.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct PoolTiming {
    pub submit: Duration,
    pub join: Duration,
    pub retries: u64,
    pub polls: u64,
    pub active: [Duration; 2],
    pub queue: [Duration; 2],
    pub gaps: [Duration; 2],
    pub steps: [u64; 2],
}

struct Measured<T> {
    value: T,
    submitted: Instant,
    first: Option<Instant>,
    finished: Instant,
    active: Duration,
    steps: u64,
}
impl<T> Measured<T> {
    fn new(value: T) -> Self {
        let now = Instant::now();
        Self {
            value,
            submitted: now,
            first: None,
            finished: now,
            active: Duration::ZERO,
            steps: 0,
        }
    }
}

use trueos::worker::{self, JoinHandle, SpawnError};

/// The two persistent performance workers are available for this draw.
/// Admission remains fallible because another bounded pair can occupy them
/// between this check and submission.
#[inline]
pub(crate) fn has_two_workers() -> bool {
    cfg!(feature = "raster-pool") && worker::compute_capacity() >= 2
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
) -> Result<(T, T, PoolTiming), SpawnError>
where
    T: Send + 'static,
{
    let mut timing = PoolTiming::default();
    let first_slot = Arc::new(Mutex::new(Some(Measured::new(first))));
    let second_slot = Arc::new(Mutex::new(Some(Measured::new(second))));
    let started = Instant::now();
    let first_handle = submit_retry(first_slot.clone(), step, &mut timing.retries)?;
    let second_handle = match submit_retry(second_slot.clone(), step, &mut timing.retries) {
        Ok(handle) => handle,
        Err(error) => {
            // The first closure may still be in its duty cooldown.  Drain it
            // before dropping the last Arc that owns its output buffers.
            let _ = join(first_handle, &mut timing.polls);
            return Err(error);
        }
    };

    // Always wait for both handles.  A completion-channel error must not let
    // its sibling retain an Arc to buffers that a caller is about to discard.
    timing.submit = started.elapsed();
    let started = Instant::now();
    let first_result = join(first_handle, &mut timing.polls);
    let second_result = join(second_handle, &mut timing.polls);
    timing.join = started.elapsed();
    first_result?;
    second_result?;
    let first = take_slot(&first_slot);
    let second = take_slot(&second_slot);
    for (index, job) in [&first, &second].into_iter().enumerate() {
        let accepted = job.first.unwrap_or(job.finished);
        timing.queue[index] = accepted.saturating_duration_since(job.submitted);
        timing.active[index] = job.active;
        timing.gaps[index] = job
            .finished
            .saturating_duration_since(accepted)
            .saturating_sub(job.active);
        timing.steps[index] = job.steps;
    }
    Ok((first.value, second.value, timing))
}

fn submit_retry<T>(
    slot: Arc<Mutex<Option<Measured<T>>>>,
    step: fn(&mut T) -> bool,
    retries: &mut u64,
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
            let task_ref = task.as_mut().expect("raster band task missing");
            let started = Instant::now();
            task_ref.first.get_or_insert(started);
            let more = step(&mut task_ref.value);
            task_ref.finished = Instant::now();
            task_ref.active += task_ref.finished.saturating_duration_since(started);
            task_ref.steps += 1;
            if !more {
                *slot_for_worker.lock().expect("raster band mutex poisoned") = task.take();
            }
            more
        }) {
            Ok(handle) => return Ok(handle),
            Err(SpawnError::Unavailable) => {
                *retries += 1;
                yield_executor();
            }
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
fn join(mut handle: JoinHandle<()>, polls: &mut u64) -> Result<(), SpawnError> {
    loop {
        *polls += 1;
        if let Some(result) = handle.try_take() {
            return result.map_err(|_| SpawnError::Transport);
        }
        yield_executor();
    }
}

#[inline]
fn yield_executor() {
    #[cfg(any(target_os = "trueos", target_os = "zkvm"))]
    trueos::platform::poll_once();
    #[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
    std::thread::yield_now();
}
