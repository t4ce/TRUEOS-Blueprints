use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};
use trueos::{
    logl::{self, level},
    t,
};

const DEADLINE: Duration = Duration::from_secs(5);
#[derive(Default)]
struct Shared {
    state: Mutex<u32>,
    wake: Condvar,
}

fn notify(shared: &Shared, value: u32, all: bool) {
    *shared.state.lock().unwrap() = value;
    if all {
        shared.wake.notify_all();
    } else {
        shared.wake.notify_one();
    }
}

fn wait(shared: &Shared, expected: u32) -> Result<u32, &'static str> {
    let deadline = Instant::now() + DEADLINE;
    let mut state = shared.state.lock().map_err(|_| "mutex.lock")?;
    while *state < expected {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err("condvar.timeout");
        }
        (state, _) = shared
            .wake
            .wait_timeout(state, remaining.min(Duration::from_millis(10)))
            .map_err(|_| "condvar.wait")?;
    }
    Ok(*state)
}

async fn run_probe() -> Result<(), &'static str> {
    match std::thread::Builder::new().spawn(|| ()) {
        Err(error) if error.kind() == std::io::ErrorKind::Unsupported => {}
        _ => return Err("std.spawn.must-be-unsupported"),
    }
    if t::worker::capacity() < 2 {
        return Err("insufficient-native-capacity");
    }
    let shared = Arc::new(Shared::default());
    let worker_shared = shared.clone();
    let job = t::worker::spawn(move || {
        notify(&worker_shared, 1, false);
        0x51A1u32
    })
    .map_err(|_| "signal.submit")?;
    let observed = wait(&shared, 1);
    let joined = job.await.map_err(|_| "signal.join")?;
    if observed? != 1 || joined != 0x51A1 {
        return Err("signal.value");
    }

    // Both workers wait on the same predicate; notify-before-wait must work too.
    let mut jobs = Vec::new();
    let mut error = None;
    for index in 0..2u32 {
        let worker_shared = shared.clone();
        match t::worker::spawn(move || wait(&worker_shared, 2).map(|value| value ^ index)) {
            Ok(job) => jobs.push(job),
            Err(_) => {
                error = Some("broadcast.submit");
                break;
            }
        }
    }
    t::task::yield_now().await;
    notify(&shared, 2, true); // always release accepted work, including partial admission
    let mut sum = 0;
    let mut completed = 0;
    for mut job in jobs {
        let joined = match t::time::timeout(DEADLINE, &mut job).await {
            Ok(result) => result,
            Err(_) => {
                error.get_or_insert("broadcast.join.timeout");
                job.await
            }
        };
        match joined {
            Ok(Ok(value)) => {
                sum += value;
                completed += 1;
            }
            Ok(Err(stage)) => {
                error.get_or_insert(stage);
            }
            Err(_) => {
                error.get_or_insert("broadcast.join");
            }
        }
    }
    if completed != 2 || sum != 5 {
        error.get_or_insert("broadcast.value");
    }
    error.map_or(Ok(()), Err)
}

fn main() {
    let runtime = match t::runtime::current_thread().build() {
        Ok(runtime) => runtime,
        Err(error) => {
            logl::log(level::ERROR, format_args!("condvar: FAIL runtime={error}"));
            return;
        }
    };
    match runtime.block_on(run_probe()) {
        Ok(()) => logl::log(
            level::INFO,
            format_args!("condvar: PASS signal broadcast native-completion std.spawn.unsupported"),
        ),
        Err(stage) => logl::log(level::ERROR, format_args!("condvar: FAIL stage={stage}")),
    }
}
