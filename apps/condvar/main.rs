use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::Duration;

use trueos::{
    logl::{self, level},
    platform,
    t,
};

const WAIT_TICK: Duration = Duration::from_millis(1);
const WAIT_LIMIT: usize = 512;

#[derive(Default)]
struct Shared {
    state: Mutex<State>,
    condvar: Condvar,
}

#[derive(Clone, Copy, Default)]
struct State {
    stage: u32,
    value: u32,
}

fn main() {
    logl::log(level::INFO, format_args!("condvar: start"));

    match run_probe() {
        Ok(()) => logl::log(level::INFO, format_args!("condvar: done")),
        Err(stage) => logl::log(level::ERROR, format_args!("condvar: failed stage={}", stage)),
    }
}

fn run_probe() -> Result<(), &'static str> {
    std_thread_signal_roundtrip()?;
    std_thread_broadcast_roundtrip()?;
    tokio_spawn_blocking_wait_roundtrip()?;
    Ok(())
}

fn std_thread_signal_roundtrip() -> Result<(), &'static str> {
    logl::log(level::INFO, format_args!("condvar: stage std.signal.spawn"));
    let shared = Arc::new(Shared::default());
    let worker_shared = Arc::clone(&shared);
    let worker = thread::Builder::new()
        .name("condvar-std-signal".to_string())
        .spawn(move || {
            notify_stage(&worker_shared, 1, 0xC0DE_0001);
            0x51A1_u32
        })
        .map_err(|_| "std.signal.spawn")?;

    logl::log(level::INFO, format_args!("condvar: stage std.signal.wait"));
    let value = wait_for_stage(&shared, 1, "std.signal.wait")?;
    if value != 0xC0DE_0001 {
        return Err("std.signal.value");
    }

    logl::log(level::INFO, format_args!("condvar: stage std.signal.join"));
    let joined = worker.join().map_err(|_| "std.signal.join")?;
    if joined != 0x51A1 {
        return Err("std.signal.join_value");
    }

    logl::log(level::INFO, format_args!("condvar: success std.signal"));
    Ok(())
}

fn std_thread_broadcast_roundtrip() -> Result<(), &'static str> {
    logl::log(level::INFO, format_args!("condvar: stage std.broadcast.spawn"));
    let shared = Arc::new(Shared::default());
    let mut workers = Vec::new();
    for index in 0..2u32 {
        let worker_shared = Arc::clone(&shared);
        workers.push(
            thread::Builder::new()
                .name(format!("condvar-std-broadcast-{index}"))
                .spawn(move || {
                    let value = wait_for_stage(&worker_shared, 2, "std.broadcast.worker")?;
                    Ok::<u32, &'static str>(value ^ index)
                })
                .map_err(|_| "std.broadcast.spawn")?,
        );
    }

    logl::log(level::INFO, format_args!("condvar: stage std.broadcast.notify"));
    notify_stage(&shared, 2, 0xC0DE_0002);

    let mut sum = 0u32;
    for worker in workers {
        let value = worker
            .join()
            .map_err(|_| "std.broadcast.join")?
            .map_err(|_| "std.broadcast.worker")?;
        sum = sum.wrapping_add(value);
    }
    let expected = (0xC0DE_0002u32 ^ 0).wrapping_add(0xC0DE_0002u32 ^ 1);
    if sum != expected {
        return Err("std.broadcast.value");
    }

    logl::log(level::INFO, format_args!("condvar: success std.broadcast"));
    Ok(())
}

fn tokio_spawn_blocking_wait_roundtrip() -> Result<(), &'static str> {
    logl::log(
        level::INFO,
        format_args!("condvar: stage tokio.runtime.build"),
    );
    let runtime = t::runtime::current_thread()
        .build()
        .map_err(|_| "tokio.runtime.build")?;

    let result = runtime.block_on(async {
        logl::log(
            level::INFO,
            format_args!("condvar: stage tokio.spawn_blocking.waiter"),
        );
        let shared = Arc::new(Shared::default());
        let waiter_shared = Arc::clone(&shared);
        let waiter = t::tokio::task::spawn_blocking(move || {
            wait_for_stage(&waiter_shared, 3, "tokio.spawn_blocking.wait")
        });

        for _ in 0..8 {
            t::task::yield_now().await;
        }

        logl::log(
            level::INFO,
            format_args!("condvar: stage tokio.spawn_blocking.notify"),
        );
        notify_stage(&shared, 3, 0xC0DE_0003);

        let value = waiter
            .await
            .map_err(|_| "tokio.spawn_blocking.join")?
            .map_err(|_| "tokio.spawn_blocking.wait")?;
        if value != 0xC0DE_0003 {
            return Err("tokio.spawn_blocking.value");
        }

        Ok::<(), &'static str>(())
    });

    drop(runtime);
    result?;

    logl::log(
        level::INFO,
        format_args!("condvar: success tokio.spawn_blocking"),
    );
    Ok(())
}

fn notify_stage(shared: &Shared, stage: u32, value: u32) {
    let mut state = shared.state.lock().unwrap();
    state.stage = stage;
    state.value = value;
    shared.condvar.notify_all();
}

fn wait_for_stage(
    shared: &Shared,
    expected_stage: u32,
    stage_name: &'static str,
) -> Result<u32, &'static str> {
    let mut state = shared.state.lock().map_err(|_| stage_name)?;
    for _ in 0..WAIT_LIMIT {
        if state.stage >= expected_stage {
            return Ok(state.value);
        }

        let (next_state, timeout) = shared
            .condvar
            .wait_timeout(state, WAIT_TICK)
            .map_err(|_| stage_name)?;
        state = next_state;

        if timeout.timed_out() {
            drop(state);
            platform::thread::yield_now();
            state = shared.state.lock().map_err(|_| stage_name)?;
        }
    }

    Err(stage_name)
}
