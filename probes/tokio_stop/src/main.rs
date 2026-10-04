use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};
use trueos::logl::{self, level};

static TLS_DROPS: AtomicUsize = AtomicUsize::new(0);
struct Tls;
impl Drop for Tls {
    fn drop(&mut self) {
        TLS_DROPS.fetch_add(1, Ordering::AcqRel);
    }
}
std::thread_local! { static TLS: Tls = const { Tls }; }

fn log(message: std::fmt::Arguments<'_>) {
    logl::log(level::INFO, message);
}

fn main() {
    if let Err(stage) = run() {
        log(format_args!("tokio_stop: FAIL stage={stage}"));
    }
}

fn await_stop() -> Result<(), &'static str> {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        if trueos::shutdown::requested().map_err(|_| "poll")? {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err("request-timeout");
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn run() -> Result<(), &'static str> {
    // This guard acknowledges the existing shutdown VMCALL after every later
    // local has dropped, including the Tokio runtime and all native workers.
    let shutdown = trueos::shutdown::ShutdownGuard::register().map_err(|_| "register")?;
    let instance = trueos::replication::current_identity()
        .ok_or("instance-identity")?
        .instance_guid();
    if trueos::shutdown::ShutdownGuard::register().is_ok() {
        return Err("duplicate-owner");
    }
    let started = Arc::new(AtomicUsize::new(0));
    let stopped = Arc::new(AtomicUsize::new(0));
    let start = started.clone();
    let stop = stopped.clone();
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .on_thread_start(move || {
            TLS.with(|_| ());
            start.fetch_add(1, Ordering::AcqRel);
        })
        .on_thread_stop(move || {
            stop.fetch_add(1, Ordering::AcqRel);
        })
        .build()
        .map_err(|_| "runtime-build")?;
    let (ready_tx, ready_rx) = std::sync::mpsc::channel();
    let native = std::thread::spawn(move || {
        TLS.with(|_| ());
        ready_tx.send(()).unwrap();
        await_stop()
    });
    let pool = Arc::new(tokio_parallel::ThreadPool::from_handle(
        runtime.handle().clone(),
    ));
    let retained_pool = pool.clone();
    let (cpu_tx, cpu_rx) = std::sync::mpsc::channel();
    pool.spawn(move || {
        let _retained_pool = retained_pool;
        cpu_tx.send(await_stop()).unwrap();
    });
    runtime.spawn(async {
        loop {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    });
    ready_rx
        .recv_timeout(Duration::from_secs(5))
        .map_err(|_| "native-ready")?;
    let deadline = Instant::now() + Duration::from_secs(5);
    while started.load(Ordering::Acquire) < 2 {
        if Instant::now() >= deadline {
            return Err("workers-ready");
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    log(format_args!(
        "tokio_stop: READY workers=2 std=1 cleanup_owner=1 instance={instance}"
    ));
    await_stop()?;
    if !shutdown.requested().map_err(|_| "owner-poll")? {
        return Err("owner-request");
    }
    log(format_args!("tokio_stop: observed host-stop"));
    native.join().map_err(|_| "native-join")??;
    cpu_rx
        .recv_timeout(Duration::from_secs(5))
        .map_err(|_| "cpu-finish")??;
    // Admission must remain open after a stop request: cleanup can require a
    // lazily-created Tokio blocking worker for persistence or log flushing.
    runtime
        .block_on(async {
            tokio::task::spawn_blocking(|| 42)
                .await
                .map_err(|_| "cleanup-blocking")
        })
        .and_then(|value| {
            if value == 42 {
                Ok(())
            } else {
                Err("cleanup-value")
            }
        })?;
    drop(pool);
    drop(runtime);
    let workers = started.load(Ordering::Acquire);
    let stopped = stopped.load(Ordering::Acquire);
    let tls = TLS_DROPS.load(Ordering::Acquire);
    if workers != 3 || stopped != workers || tls != 4 {
        return Err("thread-cleanup-counts");
    }
    log(format_args!(
        "tokio_stop: PASS started={workers} stopped={stopped} tls_destructors={tls} cpu=joined std=joined cleanup_blocking=42"
    ));
    log(format_args!("tokio_stop: DONE instance={instance}"));
    drop(shutdown);
    Ok(())
}
