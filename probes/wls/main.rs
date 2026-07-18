use std::cell::Cell;
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};
use std::thread;

use trueos::{
    logl::{self, level},
    platform, t, vsys,
};

const THREAD_WAIT_LIMIT: usize = 4096;
const RUNTIME_MARK: u64 = 0xA11C_E000_0000_0001;
const STD_BASE_MARK: u64 = 0x57D0_0000_0000_0000;
const BLOCKING_BASE_MARK: u64 = 0xB10C_0000_0000_0000;

thread_local! {
    static WLS_MARKER: Cell<u64> = const { Cell::new(0) };
}

fn main() {
    logl::log(
        level::INFO,
        format_args!("wls: start tid={}", vsys::thread_current_id()),
    );

    match run_probe() {
        Ok(()) => logl::log(level::INFO, format_args!("wls: done")),
        Err(stage) => logl::log(level::ERROR, format_args!("wls: failed stage={}", stage)),
    }
}

fn run_probe() -> Result<(), &'static str> {
    runtime_wls_stability()?;
    std_thread_wls_freshness()?;
    std_thread_wls_isolation()?;
    tokio_blocking_wls_boundary()?;
    Ok(())
}

fn runtime_wls_stability() -> Result<(), &'static str> {
    logl::log(level::INFO, format_args!("wls: stage runtime.stability"));
    let initial = marker_get();
    if initial != 0 {
        return Err("runtime.stability.initial");
    }
    marker_set(RUNTIME_MARK);
    platform::thread::yield_now();
    let after = marker_get();
    if after != RUNTIME_MARK {
        return Err("runtime.stability.after");
    }
    logl::log(
        level::INFO,
        format_args!(
            "wls: success runtime.stability tid={} initial=0x{:X} after=0x{:X}",
            vsys::thread_current_id(),
            initial,
            after
        ),
    );
    Ok(())
}

fn std_thread_wls_freshness() -> Result<(), &'static str> {
    logl::log(level::INFO, format_args!("wls: stage std.freshness"));
    for index in 0..3u64 {
        let expected = STD_BASE_MARK | index;
        let worker = thread::Builder::new()
            .name(format!("wls-std-fresh-{index}"))
            .spawn(move || {
                let tid = vsys::thread_current_id();
                let initial = marker_get();
                marker_set(expected);
                let after = marker_get();
                (index, tid, initial, after)
            })
            .map_err(|_| "std.freshness.spawn")?;
        let (joined_index, tid, initial, after) =
            worker.join().map_err(|_| "std.freshness.join")?;
        logl::log(
            level::INFO,
            format_args!(
                "wls: std.freshness index={} tid={} initial=0x{:X} after=0x{:X}",
                joined_index, tid, initial, after
            ),
        );
        if initial != 0 {
            return Err("std.freshness.initial");
        }
        if after != expected {
            return Err("std.freshness.after");
        }
    }
    logl::log(level::INFO, format_args!("wls: success std.freshness"));
    Ok(())
}

fn std_thread_wls_isolation() -> Result<(), &'static str> {
    logl::log(level::INFO, format_args!("wls: stage std.isolation"));
    let ready = Arc::new(AtomicUsize::new(0));
    let release = Arc::new(AtomicBool::new(false));
    let mut workers = Vec::new();

    for index in 0..2u64 {
        let ready = Arc::clone(&ready);
        let release = Arc::clone(&release);
        workers.push(
            thread::Builder::new()
                .name(format!("wls-std-isolate-{index}"))
                .spawn(move || {
                    let expected = STD_BASE_MARK | 0x100 | index;
                    let tid = vsys::thread_current_id();
                    let initial = marker_get();
                    marker_set(expected);
                    ready.fetch_add(1, Ordering::AcqRel);
                    while !release.load(Ordering::Acquire) {
                        core::hint::spin_loop();
                    }
                    let after = marker_get();
                    (index, tid, initial, after, expected)
                })
                .map_err(|_| "std.isolation.spawn")?,
        );
    }

    for _ in 0..THREAD_WAIT_LIMIT {
        if ready.load(Ordering::Acquire) == 2 {
            break;
        }
        platform::thread::yield_now();
    }
    if ready.load(Ordering::Acquire) != 2 {
        release.store(true, Ordering::Release);
        return Err("std.isolation.ready");
    }
    release.store(true, Ordering::Release);

    let mut tid_min = usize::MAX;
    let mut tid_max = 0usize;
    for worker in workers {
        let (index, tid, initial, after, expected) =
            worker.join().map_err(|_| "std.isolation.join")?;
        logl::log(
            level::INFO,
            format_args!(
                "wls: std.isolation index={} tid={} initial=0x{:X} after=0x{:X}",
                index, tid, initial, after
            ),
        );
        if initial != 0 {
            return Err("std.isolation.initial");
        }
        if after != expected {
            return Err("std.isolation.after");
        }
        tid_min = tid_min.min(tid);
        tid_max = tid_max.max(tid);
    }

    logl::log(
        level::INFO,
        format_args!(
            "wls: success std.isolation tid_min={} tid_max={} distinct={}",
            tid_min,
            tid_max,
            tid_min != tid_max
        ),
    );
    Ok(())
}

fn tokio_blocking_wls_boundary() -> Result<(), &'static str> {
    logl::log(
        level::INFO,
        format_args!("wls: stage tokio.blocking.boundary"),
    );
    let runtime = t::runtime::current_thread()
        .build()
        .map_err(|_| "tokio.blocking.runtime")?;

    let result = runtime.block_on(async {
        let runtime_tid = vsys::thread_current_id();
        let runtime_before = marker_get();
        if runtime_before != RUNTIME_MARK {
            return Err("tokio.blocking.runtime_before");
        }

        let first = t::tokio::task::spawn_blocking(|| blocking_marker_job(0))
            .await
            .map_err(|_| "tokio.blocking.first_join")?;
        check_blocking_sample(first, 0, "tokio.blocking.first")?;

        let runtime_after_first = marker_get();
        if runtime_after_first != RUNTIME_MARK {
            return Err("tokio.blocking.runtime_after_first");
        }

        let second = t::tokio::task::spawn_blocking(|| blocking_marker_job(1))
            .await
            .map_err(|_| "tokio.blocking.second_join")?;
        check_blocking_sample(second, 1, "tokio.blocking.second")?;

        let runtime_after_second = marker_get();
        if runtime_after_second != RUNTIME_MARK {
            return Err("tokio.blocking.runtime_after_second");
        }

        logl::log(
            level::INFO,
            format_args!(
                "wls: tokio.blocking reuse_observed={} runtime_tid={} first_tid={} second_tid={}",
                second.initial == first.after,
                runtime_tid,
                first.tid,
                second.tid
            ),
        );
        Ok::<(), &'static str>(())
    });

    drop(runtime);
    result?;
    logl::log(
        level::INFO,
        format_args!("wls: success tokio.blocking.boundary"),
    );
    Ok(())
}

#[derive(Clone, Copy)]
struct BlockingSample {
    tid: usize,
    initial: u64,
    after: u64,
}

fn blocking_marker_job(index: u64) -> BlockingSample {
    let expected = BLOCKING_BASE_MARK | index;
    let sample = BlockingSample {
        tid: vsys::thread_current_id(),
        initial: marker_get(),
        after: {
            marker_set(expected);
            marker_get()
        },
    };
    logl::log(
        level::INFO,
        format_args!(
            "wls: tokio.blocking sample index={} tid={} initial=0x{:X} after=0x{:X}",
            index, sample.tid, sample.initial, sample.after
        ),
    );
    sample
}

fn check_blocking_sample(
    sample: BlockingSample,
    index: u64,
    stage: &'static str,
) -> Result<(), &'static str> {
    if sample.initial == RUNTIME_MARK {
        return Err(stage);
    }
    if sample.after != (BLOCKING_BASE_MARK | index) {
        return Err(stage);
    }
    Ok(())
}

fn marker_get() -> u64 {
    WLS_MARKER.with(Cell::get)
}

fn marker_set(value: u64) {
    WLS_MARKER.with(|marker| marker.set(value));
}
