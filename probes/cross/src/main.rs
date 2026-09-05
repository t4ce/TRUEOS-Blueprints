#![no_std]

extern crate alloc;

use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicUsize, Ordering};
use crossbeam::{
    atomic::AtomicCell,
    queue::{ArrayQueue, SegQueue},
    utils::{Backoff, CachePadded},
};
use trueos::{
    clock,
    logl::{self, level},
    t, vshell, vsys,
};

const ATOMIC_ROUNDS: u32 = 256;
const QUEUE_ROUNDS: u32 = 512;
const ARRAY_QUEUE_CAP: usize = 32;
const FIB_COUNT: usize = 20;
const TOKIO_LANE_TASKS: usize = 4;
const PRESSURE_BLOCKERS: usize = 2;
const PRESSURE_START_WAIT_MS: usize = 2_000;
const PRESSURE_WORK_ROUNDS: u64 = 8192;

fn main() {
    logl::log(
        level::INFO,
        format_args!(
            "cross: start tid={} mono_ns={}",
            vsys::thread_current_id(),
            clock::monotonic_nanos()
        ),
    );

    run_proof("atomic-cell", prove_crossbeam_atomic_cell);
    run_proof("cache-padded", prove_crossbeam_cache_padded);
    run_proof("backoff", prove_crossbeam_backoff);
    run_proof("array-queue", prove_crossbeam_array_queue);
    run_proof("seg-queue", prove_crossbeam_seg_queue);
    run_proof("fibonacci-queue", prove_crossbeam_fibonacci_queue);
    run_proof("epoch-pin", prove_crossbeam_epoch_pin);

    logl::log(
        level::WARN,
        format_args!(
            "cross: sync worker proof skipped: channel/parker/sharded-lock/wait-group require std/thread substrate"
        ),
    );

    let _ = t::runtime::current_thread().build().map(|runtime| {
        runtime.block_on(async {
            logl::log(
                level::INFO,
                format_args!(
                    "cross: executor checkpoint before-yield tid={} mono_ns={}",
                    vsys::thread_current_id(),
                    clock::monotonic_nanos()
                ),
            );
            t::task::yield_now().await;
            logl::log(
                level::INFO,
                format_args!(
                    "cross: executor checkpoint after-yield tid={} mono_ns={}",
                    vsys::thread_current_id(),
                    clock::monotonic_nanos()
                ),
            );
            probe_tokio_lane_state().await;
            probe_tokio_worker_pressure().await;
            t::fs::write("/cross_smoke.txt", b"ok").await
        })
    });

    let mut shell_input = [0u8; 64];
    loop {
        let read = vshell::read_blocking(&mut shell_input);
        let input = &shell_input[..read];
        vshell::write(b"cross: ");
        vshell::write(input);
        if !input.ends_with(b"\n") {
            vshell::write(b"\n");
        }
    }
}

async fn probe_tokio_lane_state() {
    logl::log(
        level::INFO,
        format_args!(
            "cross: tokio lane probe async-spawn start tid={} mono_ns={}",
            vsys::thread_current_id(),
            clock::monotonic_nanos()
        ),
    );

    let mut async_set = t::task::JoinSet::new();
    for index in 0..TOKIO_LANE_TASKS {
        async_set.spawn(async move {
            let before = vsys::thread_current_id();
            t::task::yield_now().await;
            let after = vsys::thread_current_id();
            let stamp = clock::monotonic_nanos();
            (index, before, after, stamp)
        });
    }

    let mut async_sum = 0usize;
    let mut async_tid_min = usize::MAX;
    let mut async_tid_max = 0usize;
    let mut async_changed = 0usize;
    for _ in 0..TOKIO_LANE_TASKS {
        match async_set.join_next().await {
            Some(Ok((index, before, after, stamp))) => {
                async_sum = async_sum.wrapping_add(index);
                async_tid_min = async_tid_min.min(before).min(after);
                async_tid_max = async_tid_max.max(before).max(after);
                if before != after {
                    async_changed += 1;
                }
                logl::log(
                    level::INFO,
                    format_args!(
                        "cross: tokio async task index={} tid_before={} tid_after={} mono_ns={}",
                        index, before, after, stamp
                    ),
                );
            }
            Some(Err(_)) => {
                logl::log(
                    level::WARN,
                    format_args!("cross: tokio async task join error"),
                );
            }
            None => {
                logl::log(
                    level::WARN,
                    format_args!("cross: tokio async task join missing"),
                );
            }
        }
    }
    logl::log(
        level::INFO,
        format_args!(
            "cross: tokio lane probe async-spawn done tasks={} sum={} tid_min={} tid_max={} changed={}",
            TOKIO_LANE_TASKS, async_sum, async_tid_min, async_tid_max, async_changed
        ),
    );

}

async fn probe_tokio_worker_pressure() {
    if t::worker::capacity() < PRESSURE_BLOCKERS {
        logl::log(level::ERROR, format_args!("cross: native FAIL insufficient-capacity"));
        return;
    }
    let ready = Arc::new(AtomicUsize::new(0));
    let release = Arc::new(AtomicUsize::new(0));
    let mut jobs = Vec::new();
    let mut failed = false;
    for index in 0..PRESSURE_BLOCKERS {
        let ready = ready.clone();
        let release = release.clone();
        match t::worker::spawn(move || {
            let slot = t::worker::local_slot();
            let runtime = t::runtime::current_thread().build().map_err(|_| "runtime")?;
            let result = runtime.block_on(async {
                ready.fetch_add(1, Ordering::AcqRel);
                t::time::timeout(t::time::Duration::from_millis(PRESSURE_START_WAIT_MS as u64), async {
                    while release.load(Ordering::Acquire) == 0 {
                        t::time::sleep(t::time::Duration::from_millis(1)).await;
                    }
                }).await.map_err(|_| "release.timeout")?;
                let mut checksum = 0u64;
                for step in 0..PRESSURE_WORK_ROUNDS {
                    checksum = checksum.wrapping_add(step ^ index as u64);
                    if step % 256 == 0 { t::task::yield_now().await; }
                }
                Ok::<_, &'static str>((slot, checksum))
            });
            drop(runtime);
            result
        }) {
            Ok(job) => jobs.push(job),
            Err(_) => { failed = true; break; }
        }
    }
    if !failed && t::time::timeout(t::time::Duration::from_millis(PRESSURE_START_WAIT_MS as u64), async {
        while ready.load(Ordering::Acquire) != PRESSURE_BLOCKERS {
            t::time::sleep(t::time::Duration::from_millis(1)).await;
        }
    }).await.is_err() { failed = true; }
    release.store(1, Ordering::Release);
    let mut slots = Vec::new();
    for mut job in jobs {
        let joined = match t::time::timeout(t::time::Duration::from_secs(5), &mut job).await {
            Ok(result) => result,
            Err(_) => { failed = true; job.await }
        };
        match joined {
            Ok(Ok((slot, checksum))) => {
                let expected = PRESSURE_WORK_ROUNDS * (PRESSURE_WORK_ROUNDS - 1) / 2;
                failed |= checksum != expected;
                slots.push(slot);
            }
            _ => failed = true,
        }
    }
    failed |= slots.len() != PRESSURE_BLOCKERS || slots[0] == slots[1];
    logl::log(if failed { level::ERROR } else { level::INFO },
        format_args!("cross: native {} lanes={} rounds={PRESSURE_WORK_ROUNDS}", if failed { "FAIL" } else { "PASS" }, slots.len()));
}

fn run_proof(name: &'static str, proof: fn() -> Result<(), &'static str>) {
    let tid_start = vsys::thread_current_id();
    let start_ns = clock::monotonic_nanos();
    logl::log(
        level::INFO,
        format_args!(
            "cross: proof {} start tid={} mono_ns={}",
            name, tid_start, start_ns
        ),
    );
    match proof() {
        Ok(()) => {
            let end_ns = clock::monotonic_nanos();
            logl::log(
                level::INFO,
                format_args!(
                    "cross: proof {} ok tid={} elapsed_ns={} elapsed_us={}",
                    name,
                    vsys::thread_current_id(),
                    end_ns.saturating_sub(start_ns),
                    end_ns.saturating_sub(start_ns) / 1_000
                ),
            );
        }
        Err(stage) => logl::log(
            level::ERROR,
            format_args!(
                "cross: proof {} failed stage={} tid={} elapsed_ns={}",
                name,
                stage,
                vsys::thread_current_id(),
                clock::monotonic_nanos().saturating_sub(start_ns)
            ),
        ),
    }
}

fn prove_crossbeam_atomic_cell() -> Result<(), &'static str> {
    let cell = AtomicCell::new(7u64);
    if cell.load() != 7 {
        return Err("initial-load");
    }
    for round in 0..ATOMIC_ROUNDS {
        let expected = 7 + u64::from(round);
        if cell.compare_exchange(expected, expected + 1).is_err() {
            return Err("compare-exchange-loop");
        }
    }
    if cell.load() != 7 + u64::from(ATOMIC_ROUNDS) {
        return Err("loop-final-load");
    }
    cell.store(11);
    if cell.swap(13) != 11 {
        return Err("swap-old");
    }
    if cell.compare_exchange(13, 17).is_err() {
        return Err("compare-exchange");
    }
    if cell.load() != 17 {
        return Err("final-load");
    }
    Ok(())
}

fn prove_crossbeam_cache_padded() -> Result<(), &'static str> {
    let padded = CachePadded::new(0xC0FFEEu64);
    if *padded != 0xC0FFEE {
        return Err("deref");
    }
    Ok(())
}

fn prove_crossbeam_backoff() -> Result<(), &'static str> {
    let backoff = Backoff::new();
    if backoff.is_completed() {
        return Err("initially-completed");
    }
    for _ in 0..8 {
        backoff.spin();
    }
    backoff.reset();
    for _ in 0..16 {
        backoff.snooze();
    }
    if !backoff.is_completed() {
        return Err("not-completed");
    }
    Ok(())
}

fn prove_crossbeam_array_queue() -> Result<(), &'static str> {
    let queue = ArrayQueue::new(ARRAY_QUEUE_CAP);
    if !queue.is_empty() {
        return Err("initial-not-empty");
    }
    let mut produced = 0u64;
    let mut consumed = 0u64;
    let bursts = QUEUE_ROUNDS / ARRAY_QUEUE_CAP as u32;
    for burst in 0..bursts {
        for index in 0..ARRAY_QUEUE_CAP as u32 {
            let value = burst
                .wrapping_mul(ARRAY_QUEUE_CAP as u32)
                .wrapping_add(index)
                .wrapping_add(1);
            queue.push(value).map_err(|_| "push-loop")?;
            produced = produced.wrapping_add(u64::from(value));
        }
        if queue.push(u32::MAX).is_ok() {
            return Err("over-capacity-push");
        }
        for _ in 0..ARRAY_QUEUE_CAP {
            let got = queue.pop().ok_or("pop-loop")?;
            consumed = consumed.wrapping_add(u64::from(got));
        }
    }
    if queue.pop().is_some() {
        return Err("pop-empty");
    }
    if produced != consumed {
        return Err("checksum");
    }
    Ok(())
}

fn prove_crossbeam_seg_queue() -> Result<(), &'static str> {
    let queue = SegQueue::new();
    if !queue.is_empty() {
        return Err("initial-not-empty");
    }
    let mut produced = 0u64;
    for round in 0..QUEUE_ROUNDS {
        let value = round.wrapping_mul(3).wrapping_add(1);
        queue.push(value);
        produced = produced.wrapping_add(u64::from(value));
    }
    let mut consumed = 0u64;
    for _ in 0..QUEUE_ROUNDS {
        let got = queue.pop().ok_or("pop-loop")?;
        consumed = consumed.wrapping_add(u64::from(got));
    }
    if queue.pop().is_some() {
        return Err("pop-empty");
    }
    if produced != consumed {
        return Err("checksum");
    }
    Ok(())
}

fn prove_crossbeam_fibonacci_queue() -> Result<(), &'static str> {
    let queue = ArrayQueue::new(FIB_COUNT);
    let (mut x, mut y) = (0u64, 1u64);

    for _ in 0..FIB_COUNT {
        queue.push(x).map_err(|_| "push")?;
        let next = x.checked_add(y).ok_or("producer-overflow")?;
        x = y;
        y = next;
    }

    let (mut expected_x, mut expected_y) = (0u64, 1u64);
    let mut last = 0u64;
    let mut sum = 0u64;
    for index in 0..FIB_COUNT {
        let got = queue.pop().ok_or("pop")?;
        if got != expected_x {
            return Err("sequence");
        }
        last = got;
        sum = sum.checked_add(got).ok_or("sum-overflow")?;
        let next = expected_x
            .checked_add(expected_y)
            .ok_or("consumer-overflow")?;
        expected_x = expected_y;
        expected_y = next;
        if index == FIB_COUNT - 1 && got != 4181 {
            return Err("last");
        }
    }

    if sum != 10_945 {
        return Err("sum");
    }
    if queue.pop().is_some() {
        return Err("pop-empty");
    }

    logl::log(
        level::INFO,
        format_args!(
            "cross: fibonacci adapted count={} last={} sum={}",
            FIB_COUNT, last, sum
        ),
    );
    Ok(())
}

fn prove_crossbeam_epoch_pin() -> Result<(), &'static str> {
    logl::log(
        level::INFO,
        format_args!(
            "cross: stage epoch.is_pinned.before tid={} mono_ns={}",
            vsys::thread_current_id(),
            clock::monotonic_nanos()
        ),
    );
    let collector = crossbeam::epoch::Collector::new();
    let handle = collector.register();

    if handle.is_pinned() {
        return Err("initially-pinned");
    }

    logl::log(
        level::INFO,
        format_args!(
            "cross: stage epoch.pin tid={} mono_ns={}",
            vsys::thread_current_id(),
            clock::monotonic_nanos()
        ),
    );
    let guard = handle.pin();

    logl::log(
        level::INFO,
        format_args!(
            "cross: stage epoch.is_pinned.during tid={} mono_ns={}",
            vsys::thread_current_id(),
            clock::monotonic_nanos()
        ),
    );
    if !handle.is_pinned() {
        return Err("not-pinned-during-guard");
    }

    guard.flush();
    drop(guard);

    logl::log(
        level::INFO,
        format_args!(
            "cross: stage epoch.is_pinned.after tid={} mono_ns={}",
            vsys::thread_current_id(),
            clock::monotonic_nanos()
        ),
    );
    if handle.is_pinned() {
        return Err("still-pinned-after-drop");
    }

    Ok(())
}
