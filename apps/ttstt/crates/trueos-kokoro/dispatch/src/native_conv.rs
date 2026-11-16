//! Guest-owned native AP carriers; immutable shared activations, async joins.
//! Only finite per-operation jobs hold AP leases. Idle pools retain weights,
//! while VM pause/teardown can drain accepted work without an idle worker loop.
use alloc::{collections::BTreeMap, format, string::String, sync::Arc, vec, vec::Vec};
use core::{
    future::Future,
    sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
};
use std::{
    sync::{Mutex, OnceLock},
    time::{Duration, Instant},
};
use trueos::worker::{self, CorePreference};
use trueos_ttstt_cpu::{Dispatcher, PreparedQConv1d, QConv1dParams, RhsZeroPoints};

type Failure = &'static str;
static POOL: OnceLock<Arc<Pool>> = OnceLock::new();
pub fn installed() -> bool {
    POOL.get().is_some()
}
pub fn pool() -> Option<&'static Arc<Pool>> {
    POOL.get()
}
pub fn install(pool: Arc<Pool>) -> Result<(), Failure> {
    POOL.set(pool).map_err(|_| "native-pool-already-installed")
}
pub async fn yield_now() {
    trueos::task::yield_now().await;
}
pub fn block_on<F: Future>(future: F) -> Result<F::Output, Failure> {
    Ok(trueos::runtime::current_thread()
        .build()
        .map_err(|_| "native-runtime-start")?
        .block_on(future))
}
pub async fn with_timeout<F: Future>(duration: Duration, f: F) -> Result<F::Output, Failure> {
    trueos::time::timeout(duration, f)
        .await
        .map_err(|_| "speech-inference-timeout")
}
pub fn install_default() -> Result<&'static Arc<Pool>, String> {
    if let Some(pool) = POOL.get() {
        return Ok(pool);
    }
    let pool = Pool::new(3).map_err(String::from)?;
    install(pool).map_err(String::from)?;
    Ok(POOL.get().unwrap())
}

#[derive(Clone, Debug)]
pub struct WorkerPlacement {
    pub cpu_slot: Option<u32>,
    pub core_kind: String,
    pub lane: &'static str,
}
pub fn current_placement()->WorkerPlacement {
    let placement=worker::placement();
    WorkerPlacement {cpu_slot:placement.map(|p|p.cpu_slot),core_kind:placement.map_or_else(||"host".into(),|p|format!("{:?}",p.core_kind)),lane:Dispatcher::detect().best_lane().as_str()}
}
#[derive(Default)]
pub struct Counters {
    pub operations: AtomicU64,
    pub tiles: AtomicU64,
    pub worker_us: AtomicU64,
    pub wait_us: AtomicU64,
    pub snapshot_bytes: AtomicU64,
    pub shared_prepare_us: AtomicU64,
    pub remote_jobs: AtomicU64,
    pub local_fallbacks: AtomicU64,
    pub active_remote: AtomicUsize,
    pub peak_remote: AtomicUsize,
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
    pub fn params<'a>(&self, w: &'a Weights) -> QConv1dParams<'a> {
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
            weight_row_sums: Some(&w.sums),
        }
    }
}
pub(crate) struct Operation {
    pub op_index: u32,
    pub input: Arc<[f32]>,
    pub shared: Option<Arc<SharedActivation>>,
    pub weights: Arc<Weights>,
    pub geometry: Geometry,
    pub prepared: PreparedQConv1d,
    pub bias: Option<Arc<[i32]>>,
}
struct Job {
    op: Arc<Operation>,
    start: usize,
    count: usize,
    cancel: Arc<AtomicBool>,
}
struct Shard {
    start: usize,
    count: usize,
    output: Vec<f32>,
}
struct Cancel(Arc<AtomicBool>);
impl Drop for Cancel {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Release);
    }
}
pub struct Pool {
    requested: usize,
    active: AtomicUsize,
    placements: Mutex<BTreeMap<u32, WorkerPlacement>>,
    weights: Mutex<BTreeMap<([u8; 32], u32), Arc<Weights>>>,
    pub counters: Arc<Counters>,
}
impl Pool {
    pub fn new(requested: usize) -> Result<Arc<Self>, Failure> {
        if !(1..=3).contains(&requested) {
            return Err("native-worker-count");
        }
        Ok(Arc::new(Self {
            requested,
            active: AtomicUsize::new(requested),
            placements: Mutex::new(BTreeMap::new()),
            weights: Mutex::new(BTreeMap::new()),
            counters: Arc::new(Counters::default()),
        }))
    }
    pub fn placements(&self) -> Vec<WorkerPlacement> {
        self.placements.lock().unwrap().values().cloned().collect()
    }
    pub fn reset_observations(&self) -> Result<(), Failure> {
        if self.counters.active_remote.load(Ordering::Acquire) != 0 {
            return Err("native-work-still-active");
        }
        self.placements.lock().unwrap().clear();
        for counter in [
            &self.counters.operations,
            &self.counters.tiles,
            &self.counters.worker_us,
            &self.counters.wait_us,
            &self.counters.snapshot_bytes,
            &self.counters.shared_prepare_us,
            &self.counters.remote_jobs,
            &self.counters.local_fallbacks,
        ] {
            counter.store(0, Ordering::Relaxed);
        }
        self.counters.peak_remote.store(0, Ordering::Relaxed);
        Ok(())
    }
    pub fn worker_count(&self) -> usize {
        self.requested
    }
    pub fn set_workers(&self, count: usize) -> Result<(), Failure> {
        if count > self.requested {
            return Err("native-worker-count");
        }
        self.active.store(count, Ordering::Release);
        Ok(())
    }
    pub fn active_workers(&self) -> usize {
        self.active.load(Ordering::Acquire)
    }
    pub(crate) fn cached_weights(
        &self,
        artifact: [u8; 32],
        op: u32,
        packed: &[i8],
        sums: &[i32],
    ) -> Arc<Weights> {
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
    pub(crate) fn activation_snapshot(
        &self,
        input: &[f32],
        params: QConv1dParams<'_>,
        prepared: PreparedQConv1d,
    ) -> Result<(Arc<[f32]>, Option<Arc<SharedActivation>>), trueos_ttstt_cpu::Error> {
        let started = Instant::now();
        let needs = params.shared_cache_requirements()?;
        let mut cache = vec![0; needs.cache_bytes];
        let mut sums = vec![0; needs.patch_sums];
        let dispatcher = Dispatcher::detect();
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
            .fetch_add((cache.len() + 4 * sums.len()) as u64, Ordering::Relaxed);
        self.counters
            .shared_prepare_us
            .fetch_add(started.elapsed().as_micros() as u64, Ordering::Relaxed);
        Ok((
            Arc::from([]),
            Some(Arc::new(SharedActivation { cache, sums })),
        ))
    }
    async fn evaluate(&self, op: Operation) -> Result<Vec<f32>, Failure> {
        let started = Instant::now();
        let op = Arc::new(op);
        let g = op.geometry;
        let workers = self.active_workers().min(g.output_channels);
        if workers == 0 {
            return Err("native-worker-mode-changed");
        }
        let cancel = Cancel(Arc::new(AtomicBool::new(false)));
        let mut results = Vec::new();
        let mut local = Vec::new();
        for index in 0..workers {
            let start = index * g.output_channels / workers;
            let end = (index + 1) * g.output_channels / workers;
            let job = Job {
                op: op.clone(),
                start,
                count: end - start,
                cancel: cancel.0.clone(),
            };
            let c = self.counters.clone();
            match worker::spawn_with_preference(CorePreference::PreferPerformance, move || {
                let active = c.active_remote.fetch_add(1, Ordering::AcqRel) + 1;
                c.peak_remote.fetch_max(active, Ordering::Relaxed);
                struct Running(Arc<Counters>);
                impl Drop for Running {
                    fn drop(&mut self) {
                        self.0.active_remote.fetch_sub(1, Ordering::AcqRel);
                    }
                }
                let _running = Running(c.clone());
                let dispatcher = Dispatcher::detect();
                let placement = worker::placement();
                let info = WorkerPlacement {
                    cpu_slot: placement.map(|p| p.cpu_slot),
                    core_kind: placement
                        .map_or_else(|| "host".into(), |p| format!("{:?}", p.core_kind)),
                    lane: dispatcher.best_lane().as_str(),
                };
                let result = trueos::runtime::current_thread()
                    .build()
                    .map_err(|_| "native-runtime-start")?
                    .block_on(compute(&job, &c, dispatcher));
                Ok::<_, Failure>((info, result))
            }) {
                Ok(handle) => {
                    self.counters.remote_jobs.fetch_add(1, Ordering::Relaxed);
                    results.push(handle);
                }
                Err(_) => {
                    self.counters
                        .local_fallbacks
                        .fetch_add(1, Ordering::Relaxed);
                    local.push(Job {
                        op: op.clone(),
                        start,
                        count: end - start,
                        cancel: cancel.0.clone(),
                    });
                }
            }
        }
        let width = op.prepared.output_width();
        let mut output = vec![0.; g.batch * g.output_channels * width];
        let mut commit = |shard: Shard| {
            for batch in 0..g.batch {
                let src = batch * shard.count * width;
                let dst = (batch * g.output_channels + shard.start) * width;
                output[dst..dst + shard.count * width]
                    .copy_from_slice(&shard.output[src..src + shard.count * width]);
            }
        };
        for job in local {
            commit(compute(&job, &self.counters, Dispatcher::detect()).await?);
        }
        for result in results {
            let (info, shard) = result.await.map_err(|_| "native-result-dropped")??;
            if let Some(slot) = info.cpu_slot {
                self.placements.lock().unwrap().insert(slot, info);
            }
            commit(shard?);
        }
        self.counters.operations.fetch_add(1, Ordering::Relaxed);
        self.counters
            .wait_us
            .fetch_add(started.elapsed().as_micros() as u64, Ordering::Relaxed);
        Ok(output)
    }
}
async fn compute(job: &Job, counters: &Counters, dispatcher: Dispatcher) -> Result<Shard, Failure> {
    let op = &job.op;
    let g = op.geometry;
    let width = op.prepared.output_width();
    let shared = op.shared.as_ref().ok_or("native-shared-cache-missing")?;
    let mut output = vec![0.; g.batch * job.count * width];
    let mut tile = vec![0.; g.batch * job.count * 64.min(width)];
    for time in (0..width).step_by(64) {
        if job.cancel.load(Ordering::Acquire) || worker::cancellation_requested() {
            return Err("native-operation-cancelled");
        }
        let times = 64.min(width - time);
        let started = Instant::now();
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
                dispatcher.best_lane(),
            )
            .map_err(|_| "native-convolution-kernel")?;
        for batch in 0..g.batch {
            for channel in 0..job.count {
                let src = (batch * job.count + channel) * times;
                let dst = (batch * job.count + channel) * width + time;
                output[dst..dst + times].copy_from_slice(&tile[src..src + times]);
            }
        }
        counters.tiles.fetch_add(1, Ordering::Relaxed);
        counters
            .worker_us
            .fetch_add(started.elapsed().as_micros() as u64, Ordering::Relaxed);
        #[cfg(target_os = "trueos")]
        trueos::platform::poll_once();
        yield_now().await;
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
            pool: POOL.get().filter(|p| p.active_workers() > 0).cloned(),
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
        if let Some(p) = self.pending.as_mut() {
            if let Some(request) = p.request.take() {
                p.output = Some(
                    self.pool
                        .as_ref()
                        .ok_or("native-pool-missing")?
                        .evaluate(request)
                        .await?,
                );
                return Ok(());
            }
            if p.output.is_none() {
                return Err("native-operation-wait-cancelled");
            }
        }
        yield_now().await;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture(pool:&Pool,width:usize)->(Operation,Vec<f32>){
        let g=Geometry{batch:2,input_channels:35,input_width:width,output_channels:7,kernel_width:7,stride:2,dilation:3,pad_left:9,pad_right:9,weight_zero:-73};
        let input:Vec<_>=(0..g.batch*g.input_channels*g.input_width).map(|i|((i*37%97)as f32-53.)*0.027).collect();
        let packed:Arc<[i8]>=(0..g.output_channels*g.input_channels*g.kernel_width).map(|i|(i as u8).wrapping_mul(71).wrapping_add(131) as i8).collect::<Vec<_>>().into();
        let sums:Arc<[i32]>=packed.chunks_exact(g.input_channels*g.kernel_width).map(|row|row.iter().fold(0i32,|sum,&x|sum.wrapping_add(i32::from(x)))).collect::<Vec<_>>().into();
        let weights=Arc::new(Weights{packed,sums});let bias:Vec<_>=(0..g.output_channels).map(|i|i as f32*0.013-0.12).collect();
        let mut qb=vec![0;g.output_channels];let params=g.params(&weights);
        let prepared=trueos_ttstt_cpu::prepare_qconv1d(&input,params,0.03125,Some(&bias),&mut qb).unwrap();
        let mut expected=vec![0.;g.batch*g.output_channels*prepared.output_width()];
        Dispatcher::detect().qconv1d_dequantized(&input,&weights.packed,&mut expected,&mut vec![0;4*g.input_channels*g.kernel_width],&mut vec![0;g.output_channels],params,0.03125,Some(&bias)).unwrap();
        let (input,shared)=pool.activation_snapshot(&input,params,prepared).unwrap();
        (Operation{op_index:0,input,shared,weights,geometry:g,prepared,bias:Some(qb.into())},expected)
    }
    #[test]
    fn finite_native_jobs_match_serial_with_two_and_three_workers(){
        for count in [2,3]{let pool=Pool::new(count).unwrap();let(op,expected)=fixture(&pool,137);
            let actual=block_on(pool.evaluate(op)).unwrap().unwrap();
            assert_eq!(actual.iter().map(|v|v.to_bits()).collect::<Vec<_>>(),expected.iter().map(|v|v.to_bits()).collect::<Vec<_>>());
            assert_eq!(pool.counters.active_remote.load(Ordering::Acquire),0);
            assert_eq!(pool.counters.remote_jobs.load(Ordering::Relaxed),count as u64);
        }
    }
    #[test]
    fn cancelled_wait_is_terminal_and_releases_owned_cache(){
        let pool=Pool::new(3).unwrap();let(op,_)=fixture(&pool,4097);let weak=Arc::downgrade(op.shared.as_ref().unwrap());
        let mut state=State{pool:Some(pool.clone()),pending:None};state.stage(0,op);
        block_on(async{
            let mut waiting=std::boxed::Box::pin(state.wait_ready());
            let polled=core::future::poll_fn(|cx|core::task::Poll::Ready(waiting.as_mut().poll(cx))).await;
            assert!(polled.is_pending());drop(waiting);
            assert_eq!(state.wait_ready().await,Err("native-operation-wait-cancelled"));
            with_timeout(Duration::from_secs(10),async{while weak.upgrade().is_some(){trueos::time::sleep(Duration::from_millis(1)).await;}}).await.unwrap();
            let(op,expected)=fixture(&pool,73);let actual=pool.evaluate(op).await.unwrap();
            assert_eq!(actual.iter().map(|v|v.to_bits()).collect::<Vec<_>>(),expected.iter().map(|v|v.to_bits()).collect::<Vec<_>>());
        }).unwrap();
    }
}
