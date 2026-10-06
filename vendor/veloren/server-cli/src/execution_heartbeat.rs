//! Native bringup diagnostics, independent of the tracing writer and game loop.
use std::{
    sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
    time::{Duration, Instant},
};

// Also retain diagnostics in server.log when worker console routing is absent.
macro_rules! emit {
    ($($arg:tt)*) => {{
        let message = format!($($arg)*);
        eprintln!("{}", message);
        tracing::info!(target: "velosrv::execution", "{}", message);
    }};
}

pub const STARTUP: usize = 0;
pub const READY: usize = 1;
pub const TICK: usize = 2;
pub const CLEANUP: usize = 3;
pub const COMMANDS: usize = 4;
pub const WAIT: usize = 5;

static PHASE: AtomicUsize = AtomicUsize::new(STARTUP);
static ENTERED: AtomicU64 = AtomicU64::new(0);
static COMPLETED: AtomicU64 = AtomicU64::new(0);
static EXECUTOR_COMPLETED: AtomicU64 = AtomicU64::new(0);
static EXECUTOR_PENDING: AtomicBool = AtomicBool::new(false);

pub fn phase(phase: usize) { PHASE.store(phase, Ordering::Relaxed); }
pub fn tick_enter(tick: u64) {
    ENTERED.store(tick, Ordering::Relaxed);
    phase(TICK);
}
pub fn tick_complete(tick: u64) { COMPLETED.store(tick, Ordering::Relaxed); }

pub fn start(runtime: &tokio::runtime::Runtime) {
    runtime.spawn(async {
        emit!("[velosrv:INFO] heartbeat source=tokio stage=task-enter");
        let started = Instant::now();
        let mut progress_at = started;
        let mut last_completed = 0;
        let mut sequence = 0u64;
        let mut interval = tokio::time::interval(Duration::from_secs(2));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            interval.tick().await;
            sequence += 1;
            let completed = COMPLETED.load(Ordering::Relaxed);
            if completed != last_completed {
                last_completed = completed;
                progress_at = Instant::now();
            }
            let phase = match PHASE.load(Ordering::Relaxed) {
                READY => "ready",
                TICK => "tick",
                CLEANUP => "cleanup",
                COMMANDS => "commands",
                WAIT => "wait",
                _ => "startup",
            };
            emit!(
                "[velosrv:INFO] heartbeat source=tokio seq={} uptime_s={} phase={} \
                 tick_entered={} tick_completed={} no_tick_progress_s={} executor_completed={} \
                 executor_pending={}",
                sequence,
                started.elapsed().as_secs(),
                phase,
                ENTERED.load(Ordering::Relaxed),
                completed,
                progress_at.elapsed().as_secs(),
                EXECUTOR_COMPLETED.load(Ordering::Relaxed),
                EXECUTOR_PENDING.load(Ordering::Relaxed),
            );
        }
    });
}

pub fn start_executor_probe<F>(runtime: &tokio::runtime::Runtime, submit: F)
where
    F: Fn(Box<dyn FnOnce() + Send>) + Send + 'static,
{
    runtime.spawn(async move {
        emit!("[velosrv:INFO] heartbeat source=executor-probe stage=task-enter");
        let mut interval = tokio::time::interval(Duration::from_secs(2));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            interval.tick().await;
            // A stalled pool retains only one probe, never a growing task queue.
            if EXECUTOR_PENDING
                .compare_exchange(false, true, Ordering::Relaxed, Ordering::Relaxed)
                .is_ok()
            {
                submit(Box::new(|| {
                    EXECUTOR_COMPLETED.fetch_add(1, Ordering::Relaxed);
                    EXECUTOR_PENDING.store(false, Ordering::Relaxed);
                }));
            }
        }
    });
}
