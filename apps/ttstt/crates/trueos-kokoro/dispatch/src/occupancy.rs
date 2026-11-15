//! Optional Ubuntu trace. Kernel overlap and thread CPU time are distinct.
//! Per-thread buffers avoid a contended global lock in every kernel window.
use alloc::{sync::Arc, vec::Vec};
use core::{
    cell::RefCell,
    sync::atomic::{AtomicBool, Ordering},
};
use cpu_time::ThreadTime;
use serde::Serialize;
use std::{
    sync::{Mutex, OnceLock},
    time::Instant,
};

pub const WORKER_KERNEL: u64 = 0;
pub const SERIAL_CONV: u64 = 1;
pub const DISPATCH: u64 = 2;
pub const QUANT_PREPARE: u64 = 3;
pub const SNAPSHOT: u64 = 4;
pub const ARENA_COMMIT: u64 = 5;
pub const AWAIT_OPERATION: u64 = 6;
pub const SHARD_ALLOC: u64 = 7;
pub const TILE_COPY: u64 = 8;
pub const ASSEMBLE: u64 = 9;
pub const QUEUE: u64 = 10;
pub const RESULT_ALLOC: u64 = 11;
pub const KIND_NAMES: [&str; 12] = [
    "worker_kernel",
    "serial_conv",
    "coordinator_dispatch",
    "quant_prepare",
    "activation_snapshot",
    "arena_commit",
    "await_operation",
    "shard_allocate",
    "tile_copy",
    "assemble",
    "queue",
    "result_allocate",
];
// [wall_start_ns, wall_end_ns, thread_cpu_start_ns, thread_cpu_end_ns,
//  kind, sealed_op_index, logical_cpu]
type Event = [u64; 7];
struct Buffer {
    cpu: usize,
    events: Mutex<Vec<Event>>,
}
struct Trace {
    origin: Instant,
    active: AtomicBool,
    buffers: Vec<Arc<Buffer>>,
}
static TRACE: OnceLock<Trace> = OnceLock::new();
std::thread_local! { static BUFFER: RefCell<Option<Arc<Buffer>>> = const { RefCell::new(None) }; }

pub fn configure(coordinator: usize, workers: &[usize]) -> bool {
    if std::env::var("TTSTT_OCCUPANCY").as_deref() != Ok("1") {
        return false;
    }
    let buffers = core::iter::once(coordinator)
        .chain(workers.iter().copied())
        .map(|cpu| {
            Arc::new(Buffer {
                cpu,
                events: Mutex::new(Vec::new()),
            })
        })
        .collect();
    TRACE
        .set(Trace {
            origin: Instant::now(),
            active: AtomicBool::new(false),
            buffers,
        })
        .unwrap_or_else(|_| panic!("occupancy trace already configured"));
    register(coordinator);
    true
}
pub fn register(cpu: usize) {
    if let Some(trace) = TRACE.get() {
        BUFFER
            .with(|slot| *slot.borrow_mut() = trace.buffers.iter().find(|b| b.cpu == cpu).cloned());
    }
}
pub fn stamp() -> u64 {
    TRACE
        .get()
        .filter(|t| t.active.load(Ordering::Relaxed))
        .map_or(0, |t| t.origin.elapsed().as_nanos() as u64)
}
fn cpu_ns() -> u64 {
    ThreadTime::now().as_duration().as_nanos() as u64
}
pub struct Span {
    buffer: Arc<Buffer>,
    start: u64,
    cpu_start: u64,
    kind: u64,
    op: u32,
}
pub fn start(kind: u64, op: u32) -> Option<Span> {
    let trace = TRACE.get().filter(|t| t.active.load(Ordering::Relaxed))?;
    let buffer = BUFFER.with(|slot| slot.borrow().clone())?;
    let start = trace.origin.elapsed().as_nanos() as u64;
    Some(Span {
        buffer,
        start,
        cpu_start: cpu_ns(),
        kind,
        op,
    })
}
impl Drop for Span {
    fn drop(&mut self) {
        let cpu_end = cpu_ns();
        let end = TRACE.get().unwrap().origin.elapsed().as_nanos() as u64;
        self.buffer.events.lock().unwrap().push([
            self.start,
            end,
            self.cpu_start,
            cpu_end,
            self.kind,
            u64::from(self.op),
            self.buffer.cpu as u64,
        ]);
    }
}
pub fn measure<T>(kind: u64, op: u32, f: impl FnOnce() -> T) -> T {
    let _span = start(kind, op);
    f()
}
pub fn queued(op: u32, submitted: u64) {
    if submitted == 0 {
        return;
    }
    let end = stamp();
    if end == 0 {
        return;
    }
    BUFFER.with(|slot| {
        if let Some(buffer) = slot.borrow().as_ref() {
            buffer.events.lock().unwrap().push([
                submitted,
                end,
                0,
                0,
                QUEUE,
                u64::from(op),
                buffer.cpu as u64,
            ]);
        }
    });
}
pub struct CaseStart {
    wall: u64,
    cpu: u64,
}
pub fn begin_case() -> Option<CaseStart> {
    let trace = TRACE.get()?;
    assert!(!trace.active.swap(false, Ordering::Relaxed));
    for buffer in &trace.buffers {
        buffer.events.lock().unwrap().clear();
    }
    let wall = trace.origin.elapsed().as_nanos() as u64;
    let cpu = cpu_ns();
    trace.active.store(true, Ordering::Release);
    Some(CaseStart { wall, cpu })
}
#[derive(Serialize)]
pub struct CaseTrace {
    pub case: alloc::string::String,
    pub start_ns: u64,
    pub end_ns: u64,
    pub coordinator_cpu_ns: u64,
    pub kind_names: [&'static str; 12],
    pub events: Vec<Event>,
}
pub fn finish_case(case: &str, begin: Option<CaseStart>) -> Option<CaseTrace> {
    let begin = begin?;
    let trace = TRACE.get().unwrap();
    let cpu_end = cpu_ns();
    let end = trace.origin.elapsed().as_nanos() as u64;
    trace.active.store(false, Ordering::Release);
    let mut events = Vec::new();
    for buffer in &trace.buffers {
        events.extend(buffer.events.lock().unwrap().drain(..));
    }
    events.sort_unstable_by_key(|e| (e[0], e[1]));
    Some(CaseTrace {
        case: case.into(),
        start_ns: begin.wall,
        end_ns: end,
        coordinator_cpu_ns: cpu_end.saturating_sub(begin.cpu),
        kind_names: KIND_NAMES,
        events,
    })
}
