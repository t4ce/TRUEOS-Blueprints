#![no_std]

extern crate alloc;

use alloc::sync::Arc;
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
const PRESSURE_BLOCKERS: usize = TOKIO_LANE_TASKS;
const PRESSURE_SENTINELS: usize = TOKIO_LANE_TASKS;
const PRESSURE_START_WAIT_MS: usize = 16;
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

    logl::log(
        level::INFO,
        format_args!(
            "cross: tokio lane probe spawn-blocking start tid={} mono_ns={}",
            vsys::thread_current_id(),
            clock::monotonic_nanos()
        ),
    );

    let mut blocking_set = t::task::JoinSet::new();
    for index in 0..TOKIO_LANE_TASKS {
        blocking_set.spawn_blocking(move || {
            let tid = vsys::thread_current_id();
            let start = clock::monotonic_nanos();
            let mut acc = 0u64;
            for step in 0..1024u64 {
                acc = acc.wrapping_add(step ^ index as u64);
                core::hint::spin_loop();
            }
            let end = clock::monotonic_nanos();
            (index, tid, end.saturating_sub(start), acc)
        });
    }

    let mut blocking_sum = 0usize;
    let mut blocking_tid_min = usize::MAX;
    let mut blocking_tid_max = 0usize;
    let mut blocking_elapsed_ns = 0u64;
    for _ in 0..TOKIO_LANE_TASKS {
        match blocking_set.join_next().await {
            Some(Ok((index, tid, elapsed_ns, acc))) => {
                blocking_sum = blocking_sum.wrapping_add(index);
                blocking_tid_min = blocking_tid_min.min(tid);
                blocking_tid_max = blocking_tid_max.max(tid);
                blocking_elapsed_ns = blocking_elapsed_ns.wrapping_add(elapsed_ns);
                logl::log(
                    level::INFO,
                    format_args!(
                        "cross: tokio blocking task index={} tid={} elapsed_ns={} acc={}",
                        index, tid, elapsed_ns, acc
                    ),
                );
            }
            Some(Err(_)) => {
                logl::log(
                    level::WARN,
                    format_args!("cross: tokio blocking task join error"),
                );
            }
            None => {
                logl::log(
                    level::WARN,
                    format_args!("cross: tokio blocking task join missing"),
                );
            }
        }
    }
    logl::log(
        level::INFO,
        format_args!(
            "cross: tokio lane probe spawn-blocking done tasks={} sum={} tid_min={} tid_max={} elapsed_ns={}",
            TOKIO_LANE_TASKS, blocking_sum, blocking_tid_min, blocking_tid_max, blocking_elapsed_ns
        ),
    );
}

async fn probe_tokio_worker_pressure() {
    logl::log(
        level::INFO,
        format_args!(
            "cross: tokio pressure start blockers={} sentinels={} tid={} mono_ns={}",
            PRESSURE_BLOCKERS,
            PRESSURE_SENTINELS,
            vsys::thread_current_id(),
            clock::monotonic_nanos()
        ),
    );

    let started = Arc::new(AtomicUsize::new(0));
    let release = Arc::new(AtomicUsize::new(0));
    let sentinel_started = Arc::new(AtomicUsize::new(0));
    let mut pressure_set = t::task::JoinSet::new();

    for index in 0..PRESSURE_BLOCKERS {
        let started = Arc::clone(&started);
        let release = Arc::clone(&release);
        pressure_set.spawn_blocking(move || {
            let tid = vsys::thread_current_id();
            let start_ns = clock::monotonic_nanos();
            let ordinal = started.fetch_add(1, Ordering::AcqRel).saturating_add(1);
            logl::log(
                level::INFO,
                format_args!(
                    "cross: pressure blocker start index={} ordinal={} tid={} mono_ns={}",
                    index, ordinal, tid, start_ns
                ),
            );

            let mut spins = 0u64;
            while release.load(Ordering::Acquire) == 0 {
                spins = spins.wrapping_add(1);
                core::hint::spin_loop();
            }

            let mut acc = spins;
            for step in 0..PRESSURE_WORK_ROUNDS {
                acc = acc.wrapping_add(step ^ index as u64);
                core::hint::spin_loop();
            }
            let end_ns = clock::monotonic_nanos();
            (
                0u8,
                index,
                tid,
                start_ns,
                end_ns.saturating_sub(start_ns),
                0usize,
                acc,
            )
        });
    }

    let mut wait_ms = 0usize;
    while started.load(Ordering::Acquire) < PRESSURE_BLOCKERS && wait_ms < PRESSURE_START_WAIT_MS {
        wait_ms = wait_ms.saturating_add(1);
        t::time::sleep(t::time::Duration::from_millis(1)).await;
    }

    let started_before_sentinels = started.load(Ordering::Acquire);
    logl::log(
        level::INFO,
        format_args!(
            "cross: tokio pressure queue sentinels blockers_started={} wait_ms={} mono_ns={}",
            started_before_sentinels,
            wait_ms,
            clock::monotonic_nanos()
        ),
    );

    for index in 0..PRESSURE_SENTINELS {
        let sentinel_started = Arc::clone(&sentinel_started);
        pressure_set.spawn_blocking(move || {
            let tid = vsys::thread_current_id();
            let start_ns = clock::monotonic_nanos();
            let ordinal = sentinel_started
                .fetch_add(1, Ordering::AcqRel)
                .saturating_add(1);
            let mut acc = 0u64;
            for step in 0..1024u64 {
                acc = acc.wrapping_add(step ^ index as u64);
                core::hint::spin_loop();
            }
            let end_ns = clock::monotonic_nanos();
            (
                1u8,
                index,
                tid,
                start_ns,
                end_ns.saturating_sub(start_ns),
                ordinal,
                acc,
            )
        });
    }

    let release_ns = clock::monotonic_nanos();
    let started_before_release = started.load(Ordering::Acquire);
    release.store(1, Ordering::Release);
    logl::log(
        level::INFO,
        format_args!(
            "cross: tokio pressure release blockers_started={} mono_ns={}",
            started_before_release, release_ns
        ),
    );

    let mut blocker_count = 0usize;
    let mut sentinel_count = 0usize;
    let mut sentinel_order_min = usize::MAX;
    let mut sentinel_order_max = 0usize;
    let mut sentinel_tid_min = usize::MAX;
    let mut sentinel_tid_max = 0usize;
    let mut blocker_elapsed_total = 0u64;
    let mut checksum = 0u64;

    for _ in 0..PRESSURE_BLOCKERS.saturating_add(PRESSURE_SENTINELS) {
        match pressure_set.join_next().await {
            Some(Ok((kind, index, tid, start_ns, elapsed_ns, ordinal, acc))) => {
                checksum = checksum.wrapping_add(acc);
                if kind == 0 {
                    blocker_count = blocker_count.saturating_add(1);
                    blocker_elapsed_total = blocker_elapsed_total.wrapping_add(elapsed_ns);
                    logl::log(
                        level::INFO,
                        format_args!(
                            "cross: pressure blocker done index={} tid={} start_ns={} elapsed_ns={} acc={}",
                            index, tid, start_ns, elapsed_ns, acc
                        ),
                    );
                } else {
                    sentinel_count = sentinel_count.saturating_add(1);
                    sentinel_order_min = sentinel_order_min.min(ordinal);
                    sentinel_order_max = sentinel_order_max.max(ordinal);
                    sentinel_tid_min = sentinel_tid_min.min(tid);
                    sentinel_tid_max = sentinel_tid_max.max(tid);
                    logl::log(
                        level::INFO,
                        format_args!(
                            "cross: pressure sentinel done index={} ordinal={} tid={} start_ns={} elapsed_ns={} acc={}",
                            index, ordinal, tid, start_ns, elapsed_ns, acc
                        ),
                    );
                }
            }
            Some(Err(_)) => {
                logl::log(level::WARN, format_args!("cross: pressure task join error"));
            }
            None => {
                logl::log(
                    level::WARN,
                    format_args!("cross: pressure task join missing"),
                );
            }
        }
    }

    if sentinel_count == 0 {
        sentinel_order_min = 0;
    }
    logl::log(
        level::INFO,
        format_args!(
            "cross: tokio pressure done blockers={}/{} sentinels={}/{} started_before_sentinels={} started_before_release={} sentinel_tid_min={} sentinel_tid_max={} sentinel_order_min={} sentinel_order_max={} blocker_elapsed_ns={} checksum={}",
            blocker_count,
            PRESSURE_BLOCKERS,
            sentinel_count,
            PRESSURE_SENTINELS,
            started_before_sentinels,
            started_before_release,
            sentinel_tid_min,
            sentinel_tid_max,
            sentinel_order_min,
            sentinel_order_max,
            blocker_elapsed_total,
            checksum
        ),
    );
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
