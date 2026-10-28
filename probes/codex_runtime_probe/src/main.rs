//! Finite native scheduling probe. No model, network, filesystem or UI setup.
//! Build/run commands and marker meanings are in ../BUILDING.txt.

use std::io;
use std::marker::PhantomData;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::{Duration, Instant};

const BUSY_MS: u64 = 50;
const HEARTBEAT_MS: u64 = 5;
const HEARTBEAT_WINDOW_MS: u64 = 125;
const DEADLINE: Duration = Duration::from_secs(2);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Config {
    mode: u32,
    cap: u32,
}

impl Config {
    fn parse(mut args: impl Iterator<Item = String>) -> io::Result<Self> {
        let mode = match args.next().as_deref() {
            None | Some("single") => 0,
            Some("isolated") => 1,
            Some("spread") => 2,
            _ => return Err(io::Error::other("mode must be single, isolated or spread")),
        };
        let cap = args.next().map_or(Ok(2), |value| {
            value.parse::<u32>().ok().filter(|cap| (1..=4).contains(cap))
                .ok_or_else(|| io::Error::other("carrier cap must be from 1 to 4"))
        })?;
        if args.next().is_some() {
            return Err(io::Error::other("usage: codex-runtime-probe [single|isolated|spread] [1..4]"));
        }
        Ok(Self { mode, cap })
    }
}

#[cfg(target_os = "trueos")]
mod platform {
    unsafe extern "C" {
        fn trueos_cabi_thread_configure_placement_v1(mode: u32, cap: u32) -> i32;
        fn trueos_cabi_thread_swap_child_role_v1(role: u32) -> u32;
        fn trueos_cabi_thread_swap_activity_v1(activity: u32) -> u32;
    }
    pub const BACKEND: &str = "trueos";
    pub const PLACEMENT_APPLIED: u32 = 1;
    pub fn configure(mode: u32, cap: u32) -> i32 { unsafe { trueos_cabi_thread_configure_placement_v1(mode, cap) } }
    pub fn child_role(role: u32) -> u32 { unsafe { trueos_cabi_thread_swap_child_role_v1(role) } }
    pub fn activity(activity: u32) -> u32 { unsafe { trueos_cabi_thread_swap_activity_v1(activity) } }
}

#[cfg(not(target_os = "trueos"))]
mod platform {
    // Host runs validate the finite probe logic, using host OS threads. They
    // cannot measure TRUEOS AP placement or validate the kernel's role ABI.
    std::thread_local! {
        static CHILD_ROLE: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
        static ACTIVITY: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
    }
    pub const BACKEND: &str = "host-model";
    pub const PLACEMENT_APPLIED: u32 = 0;
    pub fn configure(_: u32, _: u32) -> i32 { 0 }
    pub fn child_role(role: u32) -> u32 { CHILD_ROLE.with(|value| value.replace(role)) }
    pub fn activity(activity: u32) -> u32 { ACTIVITY.with(|value| value.replace(activity)) }
}

struct Scope {
    previous: u32,
    activity: bool,
    _thread: PhantomData<*mut ()>,
}

impl Scope {
    fn child(role: u32) -> Self {
        Self { previous: platform::child_role(role), activity: false, _thread: PhantomData }
    }
    fn activity(activity: u32) -> Self {
        Self { previous: platform::activity(activity), activity: true, _thread: PhantomData }
    }
}

impl Drop for Scope {
    fn drop(&mut self) {
        if self.previous != u32::MAX {
            if self.activity { platform::activity(self.previous); }
            else { platform::child_role(self.previous); }
        }
    }
}

fn spawn<T: Send + 'static>(name: &str, role: u32, job: impl FnOnce() -> T + Send + 'static) -> io::Result<thread::JoinHandle<T>> {
    let _role = Scope::child(role);
    thread::Builder::new().name(name.into()).stack_size(1024 * 1024).spawn(job)
}

fn elapsed_us(start: Instant) -> u64 { start.elapsed().as_micros().min(u64::MAX as u128) as u64 }

fn wait_for(flag: &AtomicBool) -> io::Result<()> {
    let start = Instant::now();
    while !flag.load(Ordering::Acquire) {
        if start.elapsed() >= DEADLINE { return Err(io::Error::other("probe gate deadline expired")); }
        // The TRUEOS std backend suspends this continuation. A synchronous
        // busy wait here would deadlock the single-carrier baseline.
        thread::sleep(Duration::from_millis(1));
    }
    Ok(())
}

fn cpu_heartbeat() -> io::Result<()> {
    println!("codex-runtime-probe v=1 stage=cpu_begin busy_ms={BUSY_MS} heartbeat_ms={HEARTBEAT_MS}");
    let entered = Arc::new(AtomicBool::new(false));
    let finished = Arc::new(AtomicBool::new(false));
    let busy_elapsed = Arc::new(AtomicU64::new(0));
    let started_worker = entered.clone();
    let finished_worker = finished.clone();
    let elapsed_worker = busy_elapsed.clone();
    let start = Instant::now();
    let worker = spawn("codex-probe-cpu", 3, move || {
        let _activity = Scope::activity(3);
        let began = Instant::now();
        started_worker.store(true, Ordering::Release);
        while began.elapsed() < Duration::from_millis(BUSY_MS) { std::hint::spin_loop(); }
        elapsed_worker.store(elapsed_us(began), Ordering::Release);
        finished_worker.store(true, Ordering::Release);
    })?;
    let mut previous = start;
    let mut maximum_gap_us = 0;
    let mut beats = 0u32;
    let mut during_busy = 0u32;
    while start.elapsed() < Duration::from_millis(HEARTBEAT_WINDOW_MS) {
        thread::sleep(Duration::from_millis(HEARTBEAT_MS));
        let now = Instant::now();
        maximum_gap_us = maximum_gap_us.max(now.duration_since(previous).as_micros() as u64);
        previous = now;
        beats += 1;
        if entered.load(Ordering::Acquire) && !finished.load(Ordering::Acquire) { during_busy += 1; }
    }
    worker.join().map_err(|_| io::Error::other("CPU probe thread panicked"))?;
    println!("codex-runtime-probe v=1 stage=cpu_done busy_us={} heartbeat_n={} heartbeat_during_busy={} heartbeat_max_gap_us={} elapsed_us={}",
        busy_elapsed.load(Ordering::Acquire), beats, during_busy, maximum_gap_us, elapsed_us(start));
    Ok(())
}

fn parked_wait() -> io::Result<()> {
    println!("codex-runtime-probe v=1 stage=park_begin timeout_ms=100");
    let state = Arc::new((Mutex::new(false), Condvar::new()));
    let entered = Arc::new(AtomicBool::new(false));
    let worker_state = state.clone();
    let worker_entered = entered.clone();
    let waiter = spawn("codex-probe-park", 3, move || {
        let _activity = Scope::activity(3);
        let start = Instant::now();
        let guard = worker_state.0.lock().unwrap();
        worker_entered.store(true, Ordering::Release);
        let (guard, timeout) = worker_state.1.wait_timeout_while(guard, Duration::from_millis(100), |ready| !*ready).unwrap();
        (*guard, timeout.timed_out(), elapsed_us(start))
    })?;
    let gate = wait_for(&entered);
    *state.0.lock().unwrap() = true;
    state.1.notify_all();
    let (notified, timed_out, elapsed) = waiter.join().map_err(|_| io::Error::other("park probe thread panicked"))?;
    println!("codex-runtime-probe v=1 stage=park_done notified={} timed_out={} elapsed_us={}", u32::from(notified), u32::from(timed_out), elapsed);
    gate?;
    if !notified || timed_out { return Err(io::Error::other("parked waiter did not observe foreground notification")); }
    Ok(())
}

fn blocking_pool() -> io::Result<()> {
    println!("codex-runtime-probe v=1 stage=blocking_begin max_blocking_threads=1 hold_limit_ms=500");
    let runtime = tokio::runtime::Builder::new_current_thread().enable_time().max_blocking_threads(1).build()?;
    let entered = Arc::new(AtomicBool::new(false));
    let release = Arc::new(AtomicBool::new(false));
    let first_entered = entered.clone();
    let first_release = release.clone();
    let first = runtime.spawn_blocking(move || {
        let start = Instant::now();
        first_entered.store(true, Ordering::Release);
        while !first_release.load(Ordering::Acquire) && start.elapsed() < Duration::from_millis(500) {
            // Sleeping frees the carrier but intentionally occupies this
            // Tokio blocking-pool slot until this closure returns.
            thread::sleep(Duration::from_millis(1));
        }
        elapsed_us(start)
    });
    let result = runtime.block_on(async move {
        let start = Instant::now();
        while !entered.load(Ordering::Acquire) {
            if start.elapsed() >= DEADLINE { return Err(io::Error::other("Tokio worker admission deadline expired")); }
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
        let queued_at = Instant::now();
        let second_started = Arc::new(AtomicBool::new(false));
        let second_flag = second_started.clone();
        let second = tokio::task::spawn_blocking(move || {
            second_flag.store(true, Ordering::Release);
            elapsed_us(queued_at)
        });
        tokio::time::sleep(Duration::from_millis(20)).await;
        let queued_before_release = !second_started.load(Ordering::Acquire);
        first.abort();
        release.store(true, Ordering::Release);
        let first_us = first.await.map_err(|error| io::Error::other(format!("first blocking join: {error}")))?;
        let queued_us = second.await.map_err(|error| io::Error::other(format!("second blocking join: {error}")))?;
        println!("codex-runtime-probe v=1 stage=blocking_done queued_before_release={} queued_us={} first_us={} abort_returned_value=1",
            u32::from(queued_before_release), queued_us, first_us);
        if !queued_before_release { return Err(io::Error::other("pool did not remain saturated through the observation window")); }
        Ok(())
    });
    drop(runtime);
    result
}

fn foreground() -> io::Result<()> {
    let _activity = Scope::activity(1);
    println!("codex-runtime-probe v=1 stage=foreground_begin");
    cpu_heartbeat()?;
    parked_wait()?;
    blocking_pool()?;
    println!("codex-runtime-probe v=1 stage=complete status=0");
    Ok(())
}

fn run() -> io::Result<()> {
    let config = Config::parse(std::env::args().skip(1))?;
    let rc = platform::configure(config.mode, config.cap);
    println!("codex-runtime-probe v=1 stage=config mode={} cap={} backend={} placement_applied={} rc={}",
        config.mode, config.cap, platform::BACKEND, platform::PLACEMENT_APPLIED, rc);
    if rc != 0 { return Err(io::Error::from_raw_os_error(rc)); }
    spawn("codex-probe-main", 1, foreground)?.join().map_err(|_| io::Error::other("foreground probe thread panicked"))?
}

fn main() {
    if let Err(error) = run() {
        eprintln!("codex-runtime-probe v=1 stage=failed status=1 error={error}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn accepts_only_bounded_placement_arguments() {
        assert_eq!(Config::parse(std::iter::empty()).unwrap(), Config { mode: 0, cap: 2 });
        for (mode, expected) in [("single", 0), ("isolated", 1), ("spread", 2)] {
            for cap in 1..=4 {
                assert_eq!(Config::parse([mode.into(), cap.to_string()].into_iter()).unwrap(), Config { mode: expected, cap });
            }
        }
        for args in [vec!["other"], vec!["single", "0"], vec!["isolated", "5"], vec!["spread", "two"], vec!["single", "2", "extra"]] {
            assert!(Config::parse(args.into_iter().map(str::to_owned)).is_err());
        }
    }

    #[test]
    fn finite_experiments_complete_on_host() {
        cpu_heartbeat().unwrap();
        parked_wait().unwrap();
        blocking_pool().unwrap();
    }
}
