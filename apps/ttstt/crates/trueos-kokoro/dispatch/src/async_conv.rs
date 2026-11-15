//! Host experiment: bounded async work sharing of unchanged dense convolutions.
//!
//! Jobs own snapshots, never arena pointers. The graph parks on `AsyncPending`,
//! awaits completion, then retries and commits that same sealed operation.
use alloc::{collections::BTreeMap, sync::Arc, vec, vec::Vec};
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use smol::channel::{Receiver, Sender, bounded};
use std::{
    sync::{Mutex, OnceLock},
    thread::JoinHandle,
    time::Instant,
};
use trueos_ttstt_cpu::{Dispatcher, PreparedQConv1d, QConv1dParams, RhsZeroPoints};

type Failure = &'static str;
static POOL: OnceLock<Arc<Pool>> = OnceLock::new();
std::thread_local! {
    // Each executor OS thread is pinned before jobs are admitted. Futures may
    // migrate, so cache dispatch on the thread, never inside the future.
    static DISPATCH: Dispatcher = Dispatcher::detect();
}

pub fn installed() -> bool {
    POOL.get().is_some()
}

pub fn install(pool: Arc<Pool>) -> Result<(), Failure> {
    POOL.set(pool).map_err(|_| "async-pool-already-installed")
}
pub fn pin_current(cpu: usize) -> bool {
    core_affinity::set_for_current(core_affinity::CoreId { id: cpu })
}

#[derive(Default)]
pub struct Counters {
    pub operations: AtomicU64,
    pub tiles: AtomicU64,
    pub worker_us: AtomicU64,
    pub max_tile_us: AtomicU64,
    pub wait_us: AtomicU64,
    pub snapshot_bytes: AtomicU64,
    pub shared_prepare_us: AtomicU64,
}

pub(crate) struct SharedActivation {
    cache: Vec<u8>,
    sums: Vec<i32>,
}

pub(crate) struct Weights {
    pub packed: Arc<[i8]>,
    pub sums: Arc<[i32]>,
}

#[derive(Clone, Copy)]
pub(crate) struct Geometry {
    pub batch: usize,
    pub input_channels: usize,
    pub input_width: usize,
    pub output_channels: usize,
    pub kernel_width: usize,
    pub stride: usize,
    pub dilation: usize,
    pub pad_left: usize,
    pub pad_right: usize,
    pub weight_zero: i8,
}
impl Geometry {
    pub fn params<'a>(&self, weights: &'a Weights) -> QConv1dParams<'a> {
        QConv1dParams {
            batch: self.batch,
            input_channels: self.input_channels,
            input_width: self.input_width,
            output_channels: self.output_channels,
            kernel_width: self.kernel_width,
            stride: self.stride,
            dilation: self.dilation,
            pad_left: self.pad_left,
            pad_right: self.pad_right,
            input_zero_point: 0,
            weight_zero_points: RhsZeroPoints::Scalar(self.weight_zero),
            weight_row_sums: Some(&weights.sums),
        }
    }
}
pub(crate) struct Operation {
    #[cfg_attr(not(feature = "occupancy-trace"), allow(dead_code))]
    pub op_index: u32,
    pub input: Arc<[f32]>,
    pub shared: Option<Arc<SharedActivation>>,
    pub weights: Arc<Weights>,
    pub geometry: Geometry,
    pub prepared: PreparedQConv1d,
    pub bias: Option<Arc<[i32]>>,
}
struct Job {
    operation: Arc<Operation>,
    start: usize,
    count: usize,
    reply: Sender<Result<Shard, Failure>>,
    cancel: Arc<AtomicBool>,
    #[cfg(feature = "occupancy-trace")]
    submitted_ns: u64,
}
struct Shard {
    start: usize,
    count: usize,
    output: Vec<f32>,
}
struct CancelOnDrop(Arc<AtomicBool>);
impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Release);
    }
}

pub struct Pool {
    jobs: Sender<Job>,
    stop: Sender<()>,
    threads: Mutex<Vec<JoinHandle<()>>>,
    workers: usize,
    shared_preparation: bool,
    weights: Mutex<BTreeMap<([u8; 32], u32), Arc<Weights>>>,
    pub counters: Arc<Counters>,
}
impl Pool {
    /// Fixed Linux CPU IDs; one smol executor thread per requested compute CPU.
    pub fn new(cpus: &[usize]) -> Result<Arc<Self>, Failure> {
        Self::with_preparation(cpus, false)
    }
    pub fn new_shared(cpus: &[usize]) -> Result<Arc<Self>, Failure> {
        Self::with_preparation(cpus, true)
    }
    fn with_preparation(cpus: &[usize], shared_preparation: bool) -> Result<Arc<Self>, Failure> {
        if cpus.is_empty() || cpus.len() > 3 {
            return Err("expected-one-to-three-compute-cpus");
        }
        let mut unique = cpus.to_vec();
        unique.sort_unstable();
        unique.dedup();
        if unique.len() != cpus.len() {
            return Err("duplicate-compute-cpu");
        }
        let executor = Arc::new(smol::Executor::new());
        let (jobs, incoming) = bounded::<Job>(cpus.len());
        let (stop, stopped) = bounded::<()>(1);
        let (ready, readiness) = bounded::<bool>(cpus.len());
        let counters = Arc::new(Counters::default());
        // Long-lived async consumers, bounded both in count and queued jobs.
        for _ in cpus {
            executor
                .spawn(worker(incoming.clone(), counters.clone()))
                .detach();
        }
        let mut threads = Vec::new();
        for &cpu in cpus {
            let ex = executor.clone();
            let stopped = stopped.clone();
            let ready = ready.clone();
            threads.push(
                std::thread::Builder::new()
                    .name(std::format!("tts-smol-{cpu}"))
                    .spawn(move || {
                        // Thread entry is the runtime boundary, never an operator wait.
                        smol::block_on(ex.run(async move {
                            let pinned = pin_current(cpu);
                            #[cfg(feature = "occupancy-trace")]
                            if pinned {
                                crate::occupancy::register(cpu);
                            }
                            ready.send(pinned).await.ok();
                            if pinned {
                                stopped.recv().await.ok();
                            }
                        }));
                    })
                    .map_err(|_| "compute-thread-start-failed")?,
            );
        }
        drop(ready);
        // Startup only, before inference or any async caller is running.
        for _ in cpus {
            if !readiness
                .recv_blocking()
                .map_err(|_| "compute-thread-start-failed")?
            {
                stop.close();
                for thread in threads {
                    thread.join().ok();
                }
                return Err("compute-cpu-affinity-failed");
            }
        }
        Ok(Arc::new(Self {
            jobs,
            stop,
            threads: Mutex::new(threads),
            workers: cpus.len(),
            shared_preparation,
            weights: Mutex::new(BTreeMap::new()),
            counters,
        }))
    }
    pub(crate) fn activation_snapshot(
        &self,
        input: &[f32],
        params: QConv1dParams<'_>,
        prepared: PreparedQConv1d,
    ) -> Result<(Arc<[f32]>, Option<Arc<SharedActivation>>), trueos_ttstt_cpu::Error> {
        if !self.shared_preparation {
            self.counters
                .snapshot_bytes
                .fetch_add((input.len() * 4) as u64, Ordering::Relaxed);
            return Ok((Arc::from(input), None));
        }
        let started = Instant::now();
        let needs = params.shared_cache_requirements()?;
        let mut cache = vec![0; needs.cache_bytes];
        let mut sums = vec![0; needs.patch_sums];
        let dispatcher = DISPATCH.with(|dispatcher| *dispatcher);
        dispatcher.prepare_qconv1d_shared_cache_with_lane(
            input,
            &mut cache,
            &mut vec![0; needs.column_sums],
            &mut sums,
            params,
            prepared,
            dispatcher.best_lane(),
        )?;
        self.counters
            .snapshot_bytes
            .fetch_add((cache.len() + sums.len() * 4) as u64, Ordering::Relaxed);
        self.counters
            .shared_prepare_us
            .fetch_add(started.elapsed().as_micros() as u64, Ordering::Relaxed);
        Ok((
            Arc::from([]),
            Some(Arc::new(SharedActivation { cache, sums })),
        ))
    }
    pub(crate) fn cached_weights(
        &self,
        artifact: [u8; 32],
        op: u32,
        packed: &[i8],
        sums: &[i32],
    ) -> Arc<Weights> {
        // Coordinator-only immutable packing cache; never locked across await.
        self.weights
            .lock()
            .unwrap()
            .entry((artifact, op))
            .or_insert_with(|| {
                Arc::new(Weights {
                    packed: Arc::from(packed),
                    sums: Arc::from(sums),
                })
            })
            .clone()
    }
    async fn evaluate(&self, operation: Operation) -> Result<Vec<f32>, Failure> {
        let started = Instant::now();
        #[cfg(feature = "occupancy-trace")]
        let _await_span =
            crate::occupancy::start(crate::occupancy::AWAIT_OPERATION, operation.op_index);
        let operation = Arc::new(operation);
        let g = operation.geometry;
        let width = operation.prepared.output_width();
        let workers = self.workers.min(g.output_channels);
        let (reply, replies) = bounded(workers);
        let cancellation = CancelOnDrop(Arc::new(AtomicBool::new(false)));
        let mut submitted = 0;
        for index in 0..workers {
            let start = index * g.output_channels / workers;
            let end = (index + 1) * g.output_channels / workers;
            self.jobs
                .send(Job {
                    operation: operation.clone(),
                    start,
                    count: end - start,
                    reply: reply.clone(),
                    cancel: cancellation.0.clone(),
                    #[cfg(feature = "occupancy-trace")]
                    submitted_ns: crate::occupancy::stamp(),
                })
                .await
                .map_err(|_| "compute-pool-closed")?;
            submitted += 1;
        }
        drop(reply);
        #[cfg(feature = "occupancy-trace")]
        let allocation_span =
            crate::occupancy::start(crate::occupancy::RESULT_ALLOC, operation.op_index);
        let mut output = vec![0.0; g.batch * g.output_channels * width];
        #[cfg(feature = "occupancy-trace")]
        drop(allocation_span);
        for _ in 0..submitted {
            let shard = replies
                .recv()
                .await
                .map_err(|_| "compute-result-channel-closed")??;
            #[cfg(feature = "occupancy-trace")]
            let _assembly_span =
                crate::occupancy::start(crate::occupancy::ASSEMBLE, operation.op_index);
            for batch in 0..g.batch {
                let src = batch * shard.count * width;
                let dst = (batch * g.output_channels + shard.start) * width;
                output[dst..dst + shard.count * width]
                    .copy_from_slice(&shard.output[src..src + shard.count * width]);
            }
        }
        self.counters.operations.fetch_add(1, Ordering::Relaxed);
        self.counters
            .wait_us
            .fetch_add(started.elapsed().as_micros() as u64, Ordering::Relaxed);
        Ok(output)
    }
}
impl Drop for Pool {
    fn drop(&mut self) {
        self.jobs.close();
        self.stop.close();
        // Process/pool shutdown only. No thread joins occur while an op awaits.
        for thread in self.threads.get_mut().unwrap().drain(..) {
            thread.join().ok();
        }
    }
}
async fn worker(incoming: Receiver<Job>, counters: Arc<Counters>) {
    while let Ok(job) = incoming.recv().await {
        #[cfg(feature = "occupancy-trace")]
        crate::occupancy::queued(job.operation.op_index, job.submitted_ns);
        let result = compute_shard(&job, &counters).await;
        let _ = job.reply.send(result).await;
    }
}
async fn compute_shard(job: &Job, counters: &Counters) -> Result<Shard, Failure> {
    let op = &job.operation;
    let g = op.geometry;
    let width = op.prepared.output_width();
    #[cfg(feature = "occupancy-trace")]
    let allocation_span = crate::occupancy::start(crate::occupancy::SHARD_ALLOC, op.op_index);
    let mut output = vec![0.0; g.batch * job.count * width];
    let mut scratch = vec![
        0;
        if op.shared.is_some() {
            0
        } else {
            4 * g.input_channels * g.kernel_width
        }
    ];
    const TIME_QUANTUM: usize = 64;
    let mut tile = vec![0.0; g.batch * job.count * TIME_QUANTUM.min(width)];
    #[cfg(feature = "occupancy-trace")]
    drop(allocation_span);
    for time in (0..width).step_by(TIME_QUANTUM) {
        if job.cancel.load(Ordering::Acquire) {
            return Err("compute-operation-cancelled");
        }
        let times = TIME_QUANTUM.min(width - time);
        let started = Instant::now();
        // Re-select the current executor thread's cached lane after a yield.
        let dispatcher = DISPATCH.with(|dispatcher| *dispatcher);
        let lane = dispatcher.best_lane();
        #[cfg(feature = "occupancy-trace")]
        let kernel_span = crate::occupancy::start(crate::occupancy::WORKER_KERNEL, op.op_index);
        if let Some(shared) = &op.shared {
            dispatcher
                .qconv1d_shared_cache_window_with_lane(
                    &shared.cache,
                    &shared.sums,
                    &op.weights.packed,
                    &mut tile[..g.batch * job.count * times],
                    g.params(&op.weights),
                    op.prepared,
                    op.bias.as_deref(),
                    job.start,
                    job.count,
                    time,
                    times,
                    lane,
                )
                .map_err(|_| "compute-shared-convolution-kernel-failed")?;
        } else {
            dispatcher
                .qconv1d_prepared_window_with_lane(
                    &op.input,
                    &op.weights.packed,
                    &mut tile[..g.batch * job.count * times],
                    &mut scratch,
                    g.params(&op.weights),
                    op.prepared,
                    op.bias.as_deref(),
                    job.start,
                    job.count,
                    time,
                    times,
                    lane,
                )
                .map_err(|_| "compute-convolution-kernel-failed")?;
        }
        #[cfg(feature = "occupancy-trace")]
        drop(kernel_span);
        #[cfg(feature = "occupancy-trace")]
        let copy_span = crate::occupancy::start(crate::occupancy::TILE_COPY, op.op_index);
        for batch in 0..g.batch {
            for channel in 0..job.count {
                let src = (batch * job.count + channel) * times;
                let dst = (batch * job.count + channel) * width + time;
                output[dst..dst + times].copy_from_slice(&tile[src..src + times]);
            }
        }
        #[cfg(feature = "occupancy-trace")]
        drop(copy_span);
        let elapsed = started.elapsed().as_micros() as u64;
        counters.tiles.fetch_add(1, Ordering::Relaxed);
        counters.worker_us.fetch_add(elapsed, Ordering::Relaxed);
        counters.max_tile_us.fetch_max(elapsed, Ordering::Relaxed);
        // Bounded CPU polls: other tasks/cancellation can run between windows.
        smol::future::yield_now().await;
    }
    Ok(Shard {
        start: job.start,
        count: job.count,
        output,
    })
}

pub(crate) struct Pending {
    pub op: u32,
    request: Option<Operation>,
    pub output: Option<Vec<f32>>,
}
pub(crate) struct State {
    pub pool: Option<Arc<Pool>>,
    pub pending: Option<Pending>,
}
impl Default for State {
    fn default() -> Self {
        Self {
            pool: POOL.get().cloned(),
            pending: None,
        }
    }
}
impl State {
    pub fn stage(&mut self, op: u32, request: Operation) {
        self.pending = Some(Pending {
            op,
            request: Some(request),
            output: None,
        });
    }
    pub async fn wait_ready(&mut self) -> Result<(), Failure> {
        if let Some(pending) = self.pending.as_mut() {
            if let Some(request) = pending.request.take() {
                pending.output = Some(
                    self.pool
                        .as_ref()
                        .ok_or("compute-pool-missing")?
                        .evaluate(request)
                        .await?,
                );
                return Ok(());
            }
            if pending.output.is_none() {
                // Dropping a waiter cancels the owned jobs. Never report that
                // an abandoned operation is ready or spin retrying its cursor.
                return Err("compute-operation-wait-cancelled");
            }
        }
        smol::future::yield_now().await;
        Ok(())
    }
}

#[cfg(test)]
#[path = "async_conv_tests.rs"]
mod tests;
