//! Ubuntu experiment: execute complete independent MRF branches on owned arenas.
//! The coordinator replays the original op order and copies only branch endpoints
//! at their original producers, retaining the sealed arena's lifetime contract.
use crate::{
    CpuDispatcher, CpuExecutionState, CpuWorkspace, KOKORO_CPU_WORKSPACE_REQUIREMENTS,
    PreparedWeights,
};
use alloc::{boxed::Box, collections::BTreeSet, sync::Arc, vec, vec::Vec};
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use smol::channel::{Receiver, Sender, bounded};
use std::{
    sync::{Mutex, OnceLock},
    thread::JoinHandle,
    time::Instant,
};
use trueos_kokoro_aot::{StorageKind, WorkSlice};
use trueos_kokoro_exec::{DispatchResult, Dispatcher, TensorShapeTable};
use trueos_kokoro_memory::{ExternalBindings, PhaseOneMemoryPlan, TensorMemory};

pub type Failure = &'static str;
pub const SHAPES: usize = 4744;
pub const SLOTS: usize = 2055;
const BINDINGS: usize = 16;
static POOL: OnceLock<Arc<Pool>> = OnceLock::new();
pub fn install(pool: Arc<Pool>) -> Result<(), Failure> {
    POOL.set(pool).map_err(|_| "graph-pool-already-installed")
}
pub fn installed() -> bool {
    POOL.get().is_some()
}
pub fn stage_start(stage: usize) -> u32 {
    [1646, 1918][stage]
}
pub fn stage_end(stage: usize) -> u32 {
    [1903, 2175][stage]
}
pub fn stage_at(op: u32) -> Option<usize> {
    match op {
        1646 => Some(0),
        1918 => Some(1),
        _ => None,
    }
}

#[derive(Default)]
pub struct Counters {
    pub operations: AtomicU64,
    pub branches: AtomicU64,
    pub worker_us: AtomicU64,
    pub wait_us: AtomicU64,
    pub snapshot_us: AtomicU64,
    pub snapshot_bytes: AtomicU64,
    pub private_arena_bytes: AtomicU64,
}
impl Counters {
    pub fn reset(&self) {
        for counter in [
            &self.operations,
            &self.branches,
            &self.worker_us,
            &self.wait_us,
            &self.snapshot_us,
            &self.snapshot_bytes,
            &self.private_arena_bytes,
        ] {
            counter.store(0, Ordering::Relaxed);
        }
    }
}

struct Arena {
    bytes: Vec<u8>,
    offset: usize,
    len: usize,
}
impl Arena {
    fn new(len: usize) -> Result<Self, Failure> {
        let count = len.checked_add(63).ok_or("graph-arena-overflow")?;
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(count)
            .map_err(|_| "graph-arena-allocation")?;
        bytes.resize(count, 0);
        let offset = bytes.as_ptr().align_offset(64);
        Ok(Self { bytes, offset, len })
    }
    fn bytes_mut(&mut self) -> &mut [u8] {
        &mut self.bytes[self.offset..self.offset + self.len]
    }
}

struct Workspace {
    q: Vec<u8>,
    w: Vec<i8>,
    a: Vec<i32>,
    sums: Vec<i32>,
    bias: Vec<i32>,
    gates: Vec<f32>,
}
impl Workspace {
    fn new() -> Self {
        let r = KOKORO_CPU_WORKSPACE_REQUIREMENTS;
        Self {
            q: vec![0; r.quant_u8],
            w: vec![0; r.packed_i8],
            a: vec![0; r.accum_i32],
            sums: vec![0; r.row_sums_i32],
            bias: vec![0; r.bias_i32],
            gates: vec![0.0; r.lstm_gates_f32],
        }
    }
    fn view(&mut self) -> Result<CpuWorkspace<'_>, Failure> {
        CpuWorkspace::new(
            &mut self.q,
            &mut self.w,
            &mut self.a,
            &mut self.sums,
            &mut self.bias,
            &mut self.gates,
        )
        .map_err(|_| "graph-workspace")
    }
}

trait BranchWork: Send {
    fn step(&mut self) -> Result<Option<Vec<f32>>, Failure>;
}
struct Runner {
    stage: usize,
    branch: usize,
    next: usize,
    plan: PhaseOneMemoryPlan<'static, SLOTS>,
    shapes: Box<TensorShapeTable<SHAPES>>,
    arena: Arena,
    workspace: Workspace,
    execution: CpuExecutionState<'static>,
    prepared: Option<Arc<PreparedWeights<'static>>>,
}
impl Runner {
    fn new(
        stage: usize,
        branch: usize,
        plan: &PhaseOneMemoryPlan<'static, SLOTS>,
        shapes: &TensorShapeTable<SHAPES>,
        source: &[u8],
        prepared: Option<Arc<PreparedWeights<'static>>>,
        counters: &Counters,
    ) -> Result<Self, Failure> {
        let started = Instant::now();
        let program = plan.program();
        if program.artifact_sha256() != &EXPECTED_ARTIFACT {
            return Err("graph-foreign-artifact");
        }
        let mut arena = Arena::new(source.len())?;
        let nodes = BRANCH_OPS[stage][branch];
        let produced: BTreeSet<u32> = nodes
            .iter()
            .flat_map(|&i| {
                let op = program.op(i).expect("pinned branch op");
                (0..op.output_count).map(move |j| program.op_output(op, j).expect("pinned output"))
            })
            .collect();
        let mut live_slots = BTreeSet::new();
        for &i in nodes {
            let op = program.op(i).ok_or("graph-op")?;
            for j in 0..op.input_count {
                let mut tensor = program.op_input(op, j).ok_or("graph-input")?;
                if produced.contains(&tensor) {
                    continue;
                }
                loop {
                    let desc = program.tensor(tensor).ok_or("graph-input-tensor")?;
                    match desc.storage {
                        StorageKind::Constant => break,
                        StorageKind::Slot => {
                            live_slots.insert(desc.slot_id);
                            break;
                        }
                        StorageKind::View => tensor = desc.view_of,
                        StorageKind::External => return Err("graph-external-live-input"),
                    }
                }
            }
        }
        let mut copied = 0;
        for slot_id in live_slots {
            let slot = program.slot(slot_id).ok_or("graph-slot")?;
            let base = plan.slot_bases()[slot_id as usize] as usize;
            let count = slot
                .bytes_at(plan.admission().frame_count())
                .map_err(|_| "graph-slot-size")? as usize;
            let end = base.checked_add(count).ok_or("graph-slot-overflow")?;
            arena
                .bytes_mut()
                .get_mut(base..end)
                .ok_or("graph-private-span")?
                .copy_from_slice(source.get(base..end).ok_or("graph-source-span")?);
            copied += count;
        }
        counters
            .snapshot_bytes
            .fetch_add(copied as u64, Ordering::Relaxed);
        counters
            .private_arena_bytes
            .fetch_add(source.len() as u64, Ordering::Relaxed);
        counters
            .snapshot_us
            .fetch_add(started.elapsed().as_micros() as u64, Ordering::Relaxed);
        Ok(Self {
            stage,
            branch,
            next: 0,
            plan: plan.clone(),
            shapes: Box::new(shapes.clone()),
            arena,
            workspace: Workspace::new(),
            execution: CpuExecutionState::serial_for_graph(),
            prepared,
        })
    }
}
impl BranchWork for Runner {
    fn step(&mut self) -> Result<Option<Vec<f32>>, Failure> {
        let index = BRANCH_OPS[self.stage][self.branch][self.next];
        let program = *self.plan.program();
        let mut externals = ExternalBindings::<1>::new();
        let mut memory = TensorMemory::<'_, '_, '_, SHAPES, 1, BINDINGS>::phase_one_prevalidated(
            &self.plan,
            &mut self.shapes,
            self.arena.bytes_mut(),
            &mut externals,
        )
        .map_err(|_| "graph-memory")?;
        let mut workspace = self.workspace.view()?;
        let result = {
            let dispatcher = CpuDispatcher::new_with_workspace(&mut memory, &mut workspace)
                .with_execution_state(&mut self.execution);
            let mut dispatcher = if let Some(prepared) = self.prepared.as_deref() {
                dispatcher.with_prepared_weights(prepared)
            } else {
                dispatcher
            };
            dispatcher
                .dispatch(
                    &program,
                    WorkSlice::whole_atomic(&program, index).ok_or("graph-nonatomic-op")?,
                )
                .map_err(|_| "graph-kernel")?
        };
        if result != DispatchResult::Completed {
            return Err("graph-unexpected-control");
        }
        self.next += 1;
        if self.next == BRANCH_OPS[self.stage][self.branch].len() {
            let output = memory
                .with_op(index, |a| a.output::<f32>(0).map(|v| v.to_vec()))
                .map_err(|_| "graph-output-memory")?
                .map_err(|_| "graph-output")?;
            Ok(Some(output))
        } else {
            Ok(None)
        }
    }
}

struct Task {
    branch: usize,
    work: Box<dyn BranchWork>,
}
struct Job {
    tasks: Vec<Task>,
    reply: Sender<Result<Vec<(usize, Vec<f32>)>, Failure>>,
    cancel: Arc<AtomicBool>,
}
struct Cancel(Arc<AtomicBool>);
impl Drop for Cancel {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Release);
    }
}
pub struct Pool {
    jobs: Sender<Job>,
    stop: Sender<()>,
    threads: Mutex<Vec<JoinHandle<()>>>,
    workers: usize,
    pub counters: Arc<Counters>,
}
impl Pool {
    pub fn new(cpus: &[usize]) -> Result<Arc<Self>, Failure> {
        if !(2..=3).contains(&cpus.len()) {
            return Err("graph-needs-two-or-three-cpus");
        }
        let unique: BTreeSet<_> = cpus.iter().copied().collect();
        if unique.len() != cpus.len() {
            return Err("graph-duplicate-cpu");
        }
        let executor = Arc::new(smol::Executor::new());
        let (jobs, incoming) = bounded::<Job>(cpus.len());
        let (stop, stopped) = bounded::<()>(1);
        let (ready, readiness) = bounded(cpus.len());
        let counters = Arc::new(Counters::default());
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
                    .name(std::format!("tts-mrf-{cpu}"))
                    .spawn(move || {
                        smol::block_on(ex.run(async move {
                            let pinned =
                                core_affinity::set_for_current(core_affinity::CoreId { id: cpu });
                            let _ = ready.send(pinned).await;
                            if pinned {
                                let _ = stopped.recv().await;
                            }
                        }));
                    })
                    .map_err(|_| "graph-thread-start")?,
            );
        }
        drop(ready);
        for _ in cpus {
            if !readiness
                .recv_blocking()
                .map_err(|_| "graph-thread-start")?
            {
                stop.close();
                for t in threads {
                    let _ = t.join();
                }
                return Err("graph-affinity");
            }
        }
        Ok(Arc::new(Self {
            jobs,
            stop,
            threads: Mutex::new(threads),
            workers: cpus.len(),
            counters,
        }))
    }
    async fn evaluate(&self, tasks: Vec<Task>) -> Result<[Vec<f32>; 3], Failure> {
        let started = Instant::now();
        let (reply, replies) = bounded(self.workers);
        let cancel = Cancel(Arc::new(AtomicBool::new(false)));
        let mut groups: Vec<Vec<Task>> = (0..self.workers).map(|_| Vec::new()).collect();
        for task in tasks {
            let index = if self.workers == 2 {
                usize::from(task.branch == 2)
            } else {
                task.branch
            };
            groups[index].push(task);
        }
        for tasks in groups {
            self.jobs
                .send(Job {
                    tasks,
                    reply: reply.clone(),
                    cancel: cancel.0.clone(),
                })
                .await
                .map_err(|_| "graph-pool-closed")?;
        }
        drop(reply);
        let mut output: [Option<Vec<f32>>; 3] = [None, None, None];
        for _ in 0..self.workers {
            for (branch, values) in replies.recv().await.map_err(|_| "graph-reply-closed")?? {
                if output[branch].replace(values).is_some() {
                    return Err("graph-duplicate-result");
                }
            }
        }
        self.counters.operations.fetch_add(1, Ordering::Relaxed);
        self.counters
            .wait_us
            .fetch_add(started.elapsed().as_micros() as u64, Ordering::Relaxed);
        Ok([
            output[0].take().ok_or("graph-missing-result")?,
            output[1].take().ok_or("graph-missing-result")?,
            output[2].take().ok_or("graph-missing-result")?,
        ])
    }
}
impl Drop for Pool {
    fn drop(&mut self) {
        self.jobs.close();
        self.stop.close();
        for t in self.threads.get_mut().unwrap().drain(..) {
            let _ = t.join();
        }
    }
}
async fn worker(incoming: Receiver<Job>, counters: Arc<Counters>) {
    while let Ok(mut job) = incoming.recv().await {
        let result = async {
            let mut results = Vec::new();
            for task in &mut job.tasks {
                loop {
                    if job.cancel.load(Ordering::Acquire) {
                        return Err("graph-cancelled");
                    }
                    let started = Instant::now();
                    let result = task.work.step();
                    counters
                        .worker_us
                        .fetch_add(started.elapsed().as_micros() as u64, Ordering::Relaxed);
                    if let Some(values) = result? {
                        results.push((task.branch, values));
                        counters.branches.fetch_add(1, Ordering::Relaxed);
                        break;
                    }
                    smol::future::yield_now().await;
                }
            }
            Ok(results)
        }
        .await;
        let _ = job.reply.send(result).await;
    }
}

pub struct Completed {
    stage: usize,
    outputs: [Option<Vec<f32>>; 3],
}
impl Completed {
    pub fn handles(&self, index: u32) -> bool {
        (stage_start(self.stage)..=stage_end(self.stage)).contains(&index)
    }
    pub fn take_output(&mut self, index: u32) -> Option<Vec<f32>> {
        ENDPOINTS[self.stage]
            .iter()
            .position(|&i| i == index)
            .and_then(|b| self.outputs[b].take())
    }
    pub fn finished(&self) -> bool {
        self.outputs.iter().all(Option::is_none)
    }
}
pub struct State {
    pool: Option<Arc<Pool>>,
    pending: Option<(usize, Vec<Task>)>,
    awaiting: bool,
    pub completed: Option<Completed>,
}
impl Default for State {
    fn default() -> Self {
        Self {
            pool: POOL.get().cloned(),
            pending: None,
            awaiting: false,
            completed: None,
        }
    }
}
impl State {
    pub fn enabled(&self) -> bool {
        self.pool.is_some()
    }
    pub fn stage(
        &mut self,
        stage: usize,
        plan: &PhaseOneMemoryPlan<'static, SLOTS>,
        shapes: &TensorShapeTable<SHAPES>,
        arena: &[u8],
        prepared: Option<Arc<PreparedWeights<'static>>>,
    ) -> Result<(), Failure> {
        let counters = &self.pool.as_ref().ok_or("graph-pool-missing")?.counters;
        let mut tasks = Vec::new();
        for branch in 0..3 {
            tasks.push(Task {
                branch,
                work: Box::new(Runner::new(
                    stage,
                    branch,
                    plan,
                    shapes,
                    arena,
                    prepared.clone(),
                    counters,
                )?),
            });
        }
        self.pending = Some((stage, tasks));
        Ok(())
    }
    pub async fn wait_ready(&mut self) -> Result<(), Failure> {
        if self.awaiting {
            return Err("graph-wait-cancelled");
        }
        if let Some((stage, tasks)) = self.pending.take() {
            self.awaiting = true;
            let values = self
                .pool
                .as_ref()
                .ok_or("graph-pool-missing")?
                .evaluate(tasks)
                .await?;
            self.awaiting = false;
            self.completed = Some(Completed {
                stage,
                outputs: values.map(Some),
            });
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    static TESTS: Mutex<()> = Mutex::new(());

    struct Math {
        branch: usize,
        steps: usize,
        values: Vec<f32>,
        _life: Arc<()>,
    }
    impl BranchWork for Math {
        fn step(&mut self) -> Result<Option<Vec<f32>>, Failure> {
            for value in &mut self.values {
                *value = *value * 0.999 + self.branch as f32 * 0.003;
            }
            self.steps -= 1;
            if self.steps == 0 {
                Ok(Some(core::mem::take(&mut self.values)))
            } else {
                Ok(None)
            }
        }
    }
    fn tasks(life: &Arc<()>) -> Vec<Task> {
        (0..3)
            .map(|branch| Task {
                branch,
                work: Box::new(Math {
                    branch,
                    steps: 512,
                    values: vec![branch as f32 + 0.25; 8192],
                    _life: life.clone(),
                }),
            })
            .collect()
    }
    fn expected(branch: usize) -> Vec<f32> {
        let mut value = branch as f32 + 0.25;
        for _ in 0..512 {
            value = value * 0.999 + branch as f32 * 0.003;
        }
        vec![value; 8192]
    }
    fn cpus(count: usize) -> Option<Vec<usize>> {
        let ids = core_affinity::get_core_ids()?;
        (ids.len() >= count).then(|| ids.into_iter().take(count).map(|c| c.id).collect())
    }
    fn drive<F: core::future::Future>(future: F) -> F::Output {
        smol::block_on(smol::future::race(future, async {
            smol::Timer::after(Duration::from_secs(20)).await;
            panic!("branch runtime did not finish");
        }))
    }
    fn check(values: [Vec<f32>; 3]) {
        for (branch, values) in values.into_iter().enumerate() {
            assert_eq!(
                values.iter().map(|v| v.to_bits()).collect::<Vec<_>>(),
                expected(branch)
                    .iter()
                    .map(|v| v.to_bits())
                    .collect::<Vec<_>>()
            );
        }
    }

    #[test]
    fn balanced_two_and_independent_three_workers_preserve_endpoint_order() {
        let _guard = TESTS.lock().unwrap();
        for count in [2, 3] {
            let Some(cpus) = cpus(count) else { continue };
            let pool = Pool::new(&cpus).unwrap();
            let life = Arc::new(());
            check(drive(pool.evaluate(tasks(&life))).unwrap());
            assert_eq!(pool.counters.operations.load(Ordering::Relaxed), 1);
            assert_eq!(pool.counters.branches.load(Ordering::Relaxed), 3);
            assert_eq!(pool.jobs.capacity(), Some(count));
        }
    }

    #[test]
    fn bounded_concurrent_batches_await_while_timer_runs() {
        let _guard = TESTS.lock().unwrap();
        let Some(cpus) = cpus(2) else { return };
        let pool = Pool::new(&cpus).unwrap();
        let life = Arc::new(());
        let done = AtomicBool::new(false);
        let ticks = AtomicU64::new(0);
        let ((first, second), ()) = drive(smol::future::zip(
            async {
                let result =
                    smol::future::zip(pool.evaluate(tasks(&life)), pool.evaluate(tasks(&life)))
                        .await;
                done.store(true, Ordering::Release);
                result
            },
            async {
                while !done.load(Ordering::Acquire) {
                    smol::Timer::after(Duration::from_millis(1)).await;
                    if !done.load(Ordering::Acquire) {
                        ticks.fetch_add(1, Ordering::Relaxed);
                    }
                }
            },
        ));
        check(first.unwrap());
        check(second.unwrap());
        assert!(ticks.load(Ordering::Relaxed) > 0);
        assert_eq!(pool.counters.operations.load(Ordering::Relaxed), 2);
        assert_eq!(pool.jobs.len(), 0);
    }

    #[test]
    fn dropped_wait_cancels_owned_work_and_pool_is_reusable() {
        let _guard = TESTS.lock().unwrap();
        let Some(cpus) = cpus(2) else { return };
        let pool = Pool::new(&cpus).unwrap();
        let life = Arc::new(());
        let weak = Arc::downgrade(&life);
        let mut state = State {
            pool: Some(pool.clone()),
            pending: Some((0, tasks(&life))),
            awaiting: false,
            completed: None,
        };
        drop(life);
        drive(async {
            let mut waiting = Box::pin(state.wait_ready());
            assert!(smol::future::poll_once(waiting.as_mut()).await.is_none());
            drop(waiting);
            assert_eq!(state.wait_ready().await, Err("graph-wait-cancelled"));
            while weak.upgrade().is_some() {
                smol::Timer::after(Duration::from_millis(1)).await;
            }
            check(pool.evaluate(tasks(&Arc::new(()))).await.unwrap());
        });
        assert_eq!(pool.counters.operations.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn endpoint_replay_keeps_all_three_results_until_original_producers() {
        let mut completed = Completed {
            stage: 0,
            outputs: [Some(vec![3.0]), Some(vec![7.0]), Some(vec![11.0])],
        };
        assert!(completed.handles(1646));
        assert!(!completed.handles(1904));
        assert!(completed.take_output(1700).is_none());
        assert_eq!(completed.take_output(1901), Some(vec![7.0]));
        assert_eq!(completed.take_output(1902), Some(vec![3.0]));
        assert!(!completed.finished());
        assert_eq!(completed.take_output(1903), Some(vec![11.0]));
        assert!(completed.finished());
    }
}

// Generated below from the hash-verified pinned AOT binding/DAG audit.

const EXPECTED_ARTIFACT: [u8; 32] = [
    241, 245, 204, 198, 104, 225, 113, 48, 30, 114, 32, 3, 57, 146, 239, 203, 38, 105, 249, 212, 1,
    89, 26, 172, 228, 241, 2, 90, 193, 227, 73, 152,
];
const ENDPOINTS: [[u32; 3]; 2] = [[1902, 1901, 1903], [2174, 2173, 2175]];
const BRANCH_OPS: [[&[u32]; 3]; 2] = [
    [
        &[
            1647, 1650, 1653, 1656, 1659, 1662, 1665, 1668, 1671, 1674, 1677, 1680, 1683, 1686,
            1689, 1692, 1695, 1698, 1701, 1704, 1707, 1710, 1713, 1716, 1719, 1722, 1725, 1728,
            1731, 1734, 1737, 1740, 1743, 1746, 1749, 1752, 1755, 1758, 1761, 1764, 1767, 1770,
            1773, 1776, 1779, 1782, 1785, 1788, 1791, 1794, 1797, 1800, 1803, 1806, 1809, 1812,
            1815, 1818, 1821, 1824, 1827, 1830, 1833, 1836, 1839, 1842, 1845, 1848, 1851, 1854,
            1857, 1860, 1863, 1866, 1869, 1872, 1875, 1878, 1881, 1884, 1887, 1890, 1893, 1896,
            1899, 1902,
        ],
        &[
            1646, 1649, 1652, 1655, 1658, 1661, 1664, 1667, 1670, 1673, 1676, 1679, 1682, 1685,
            1688, 1691, 1694, 1697, 1700, 1703, 1706, 1709, 1712, 1715, 1718, 1721, 1724, 1727,
            1730, 1733, 1736, 1739, 1742, 1745, 1748, 1751, 1754, 1757, 1760, 1763, 1766, 1769,
            1772, 1775, 1778, 1781, 1784, 1787, 1790, 1793, 1796, 1799, 1802, 1805, 1808, 1811,
            1814, 1817, 1820, 1823, 1826, 1829, 1832, 1835, 1838, 1841, 1844, 1847, 1850, 1853,
            1856, 1859, 1862, 1865, 1868, 1871, 1874, 1877, 1880, 1883, 1886, 1889, 1892, 1895,
            1898, 1901,
        ],
        &[
            1648, 1651, 1654, 1657, 1660, 1663, 1666, 1669, 1672, 1675, 1678, 1681, 1684, 1687,
            1690, 1693, 1696, 1699, 1702, 1705, 1708, 1711, 1714, 1717, 1720, 1723, 1726, 1729,
            1732, 1735, 1738, 1741, 1744, 1747, 1750, 1753, 1756, 1759, 1762, 1765, 1768, 1771,
            1774, 1777, 1780, 1783, 1786, 1789, 1792, 1795, 1798, 1801, 1804, 1807, 1810, 1813,
            1816, 1819, 1822, 1825, 1828, 1831, 1834, 1837, 1840, 1843, 1846, 1849, 1852, 1855,
            1858, 1861, 1864, 1867, 1870, 1873, 1876, 1879, 1882, 1885, 1888, 1891, 1894, 1897,
            1900, 1903,
        ],
    ],
    [
        &[
            1919, 1922, 1925, 1928, 1931, 1934, 1937, 1940, 1943, 1946, 1949, 1952, 1955, 1958,
            1961, 1964, 1967, 1970, 1973, 1976, 1979, 1982, 1985, 1988, 1991, 1994, 1997, 2000,
            2003, 2006, 2009, 2012, 2015, 2018, 2021, 2024, 2027, 2030, 2033, 2036, 2039, 2042,
            2045, 2048, 2051, 2054, 2057, 2060, 2063, 2066, 2069, 2072, 2075, 2078, 2081, 2084,
            2087, 2090, 2093, 2096, 2099, 2102, 2105, 2108, 2111, 2114, 2117, 2120, 2123, 2126,
            2129, 2132, 2135, 2138, 2141, 2144, 2147, 2150, 2153, 2156, 2159, 2162, 2165, 2168,
            2171, 2174,
        ],
        &[
            1918, 1921, 1924, 1927, 1930, 1933, 1936, 1939, 1942, 1945, 1948, 1951, 1954, 1957,
            1960, 1963, 1966, 1969, 1972, 1975, 1978, 1981, 1984, 1987, 1990, 1993, 1996, 1999,
            2002, 2005, 2008, 2011, 2014, 2017, 2020, 2023, 2026, 2029, 2032, 2035, 2038, 2041,
            2044, 2047, 2050, 2053, 2056, 2059, 2062, 2065, 2068, 2071, 2074, 2077, 2080, 2083,
            2086, 2089, 2092, 2095, 2098, 2101, 2104, 2107, 2110, 2113, 2116, 2119, 2122, 2125,
            2128, 2131, 2134, 2137, 2140, 2143, 2146, 2149, 2152, 2155, 2158, 2161, 2164, 2167,
            2170, 2173,
        ],
        &[
            1920, 1923, 1926, 1929, 1932, 1935, 1938, 1941, 1944, 1947, 1950, 1953, 1956, 1959,
            1962, 1965, 1968, 1971, 1974, 1977, 1980, 1983, 1986, 1989, 1992, 1995, 1998, 2001,
            2004, 2007, 2010, 2013, 2016, 2019, 2022, 2025, 2028, 2031, 2034, 2037, 2040, 2043,
            2046, 2049, 2052, 2055, 2058, 2061, 2064, 2067, 2070, 2073, 2076, 2079, 2082, 2085,
            2088, 2091, 2094, 2097, 2100, 2103, 2106, 2109, 2112, 2115, 2118, 2121, 2124, 2127,
            2130, 2133, 2136, 2139, 2142, 2145, 2148, 2151, 2154, 2157, 2160, 2163, 2166, 2169,
            2172, 2175,
        ],
    ],
];
