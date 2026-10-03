use std::cell::Cell;
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};
use std::time::Duration;
use trueos::{
    logl::{self, level},
    t,
};

const LANES: usize = 2;
const WAVES: usize = 2;
const TASKS: usize = 16;
const ROUNDS: usize = 32;
const DEADLINE: Duration = Duration::from_secs(10);

static TLS_DESTRUCTORS: AtomicUsize = AtomicUsize::new(0);

struct TlsProbe(Cell<usize>);

impl Drop for TlsProbe {
    fn drop(&mut self) {
        TLS_DESTRUCTORS.fetch_add(1, Ordering::AcqRel);
    }
}

std::thread_local! {
    static TLS_PROBE: TlsProbe = const { TlsProbe(Cell::new(0)) };
}

enum Event {
    Ready(usize, u32),
    Step(usize, u64),
}

fn main() {
    logl::log(
        level::INFO,
        format_args!("tokio_mrt: start std-and-multi-thread"),
    );
    if let Err(stage) = run_std_threads()
        .and_then(|()| run_scoped_threads())
        .and_then(|()| run_multi_thread_runtimes())
    {
        logl::log(level::ERROR, format_args!("tokio_mrt: FAIL stage={stage}"));
        return;
    }
    let runtime = match t::runtime::current_thread().build() {
        Ok(runtime) => runtime,
        Err(error) => {
            logl::log(
                level::ERROR,
                format_args!("tokio_mrt: FAIL runtime={error}"),
            );
            return;
        }
    };
    let result = runtime.block_on(async {
        for wave in 0..WAVES {
            run_wave(wave).await?;
        }
        Ok::<_, &'static str>(())
    });
    match result {
        Ok(()) => logl::log(
            level::INFO,
            format_args!(
                "tokio_mrt: PASS std_threads=2 scoped_threads=2 tokio_workers=2 rebuilds={WAVES} native_lanes={LANES} waves={WAVES} tasks={TASKS} rounds={ROUNDS}"
            ),
        ),
        Err(stage) => logl::log(level::ERROR, format_args!("tokio_mrt: FAIL stage={stage}")),
    }
}

fn run_std_threads() -> Result<(), &'static str> {
    logl::log(level::INFO, format_args!("tokio_mrt: phase=std.start"));
    let initial_drops = TLS_DESTRUCTORS.load(Ordering::Acquire);
    TLS_PROBE.with(|probe| probe.0.set(0x55));
    let main_id = std::thread::current().id();
    let release = Arc::new(AtomicBool::new(false));
    let (tx, rx) = std::sync::mpsc::channel();
    let mut jobs = Vec::new();
    let mut error = None;
    for lane in 0..LANES {
        let release = release.clone();
        let tx = tx.clone();
        match std::thread::Builder::new()
            .name(format!("mrt-std-{lane}"))
            .stack_size(256 * 1024)
            .spawn(move || {
                let thread = std::thread::current();
                let id = thread.id();
                TLS_PROBE.with(|probe| {
                    if probe.0.get() != 0 {
                        return Err("std.tls.initial");
                    }
                    probe.0.set(lane + 1);
                    Ok(())
                })?;
                // An unpark issued before park must leave a remembered token.
                thread.unpark();
                let before = std::time::Instant::now();
                std::thread::park_timeout(Duration::from_secs(1));
                if before.elapsed() >= Duration::from_millis(500) {
                    return Err("std.park.wake-before-park");
                }
                tx.send((lane, thread.clone()))
                    .map_err(|_| "std.ready.send")?;
                let before = std::time::Instant::now();
                std::thread::park_timeout(Duration::from_secs(2));
                if before.elapsed() >= Duration::from_secs(1) {
                    return Err("std.park.cross-thread-unpark");
                }
                while !release.load(Ordering::Acquire) {
                    std::thread::yield_now();
                }
                if std::thread::current().id() != id
                    || TLS_PROBE.with(|probe| probe.0.get()) != lane + 1
                {
                    return Err("std.identity-or-tls");
                }
                Ok(id)
            }) {
            Ok(job) => jobs.push(job),
            Err(_) => {
                error = Some("std.spawn");
                break;
            }
        }
    }
    drop(tx);
    let mut ids = Vec::new();
    for _ in 0..jobs.len() {
        match rx.recv_timeout(DEADLINE) {
            Ok((_, thread)) => {
                ids.push(thread.id());
                thread.unpark();
            }
            Err(_) => {
                error.get_or_insert("std.ready.timeout");
                break;
            }
        }
    }
    release.store(true, Ordering::Release);
    for job in jobs {
        match job.join() {
            Ok(Ok(_)) => {}
            Ok(Err(stage)) => {
                error.get_or_insert(stage);
            }
            Err(_) => {
                error.get_or_insert("std.join");
            }
        }
    }
    if ids.len() != LANES || ids[0] == ids[1] || ids.contains(&main_id) {
        error.get_or_insert("std.distinct-identities");
    }
    if TLS_PROBE.with(|probe| probe.0.get()) != 0x55
        || TLS_DESTRUCTORS.load(Ordering::Acquire) != initial_drops + LANES
    {
        error.get_or_insert("std.tls-destructors");
    }
    let detached = std::thread::Builder::new()
        .name("mrt-std-detached".into())
        .spawn(|| TLS_PROBE.with(|probe| probe.0.set(0x77)))
        .map_err(|_| "std.detach.spawn")?;
    drop(detached);
    let until = std::time::Instant::now() + DEADLINE;
    while TLS_DESTRUCTORS.load(Ordering::Acquire) != initial_drops + LANES + 1 {
        if std::time::Instant::now() >= until {
            error.get_or_insert("std.detach-or-tls-destructors");
            break;
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    logl::log(
        level::INFO,
        format_args!(
            "tokio_mrt: std joined={} detached=1 tls_destructors={}",
            ids.len(),
            TLS_DESTRUCTORS.load(Ordering::Acquire) - initial_drops
        ),
    );
    error.map_or(Ok(()), Err)
}

fn run_scoped_threads() -> Result<(), &'static str> {
    logl::log(level::INFO, format_args!("tokio_mrt: phase=scoped.start"));
    let initial_drops = TLS_DESTRUCTORS.load(Ordering::Acquire);
    let main_id = std::thread::current().id();
    // These values live on the spawning thread's stack. Scoped children must
    // read and update the same backing while running on other carriers.
    let stack_read = 0x90usize;
    let stack_writes = AtomicUsize::new(0);
    let stack_values = std::sync::Mutex::new((0x100usize, 0usize));
    let ids = std::thread::scope(|scope| {
        let mut jobs = Vec::new();
        for lane in 0..LANES {
            let stack_read = &stack_read;
            let stack_writes = &stack_writes;
            let stack_values = &stack_values;
            jobs.push(
                std::thread::Builder::new()
                    .name(format!("mrt-scoped-{lane}"))
                    .stack_size(256 * 1024)
                    .spawn_scoped(scope, move || {
                        let id = std::thread::current().id();
                        if id == main_id || *stack_read != 0x90 {
                            return Err("std.scoped.stack-read-or-identity");
                        }
                        TLS_PROBE.with(|probe| {
                            if probe.0.get() != 0 {
                                return Err("std.scoped.tls.initial");
                            }
                            probe.0.set(lane + 10);
                            Ok(())
                        })?;
                        // Nested children borrow an outer child's guarded
                        // stack, covering carrier-to-carrier pointer stability.
                        let nested_read = lane + 0x200;
                        let nested_write = AtomicUsize::new(0);
                        let nested_id = std::thread::scope(|nested_scope| {
                            let nested_read = &nested_read;
                            let nested_write = &nested_write;
                            std::thread::Builder::new()
                                .name(format!("mrt-scoped-nested-{lane}"))
                                .stack_size(256 * 1024)
                                .spawn_scoped(nested_scope, move || {
                                    let nested_id = std::thread::current().id();
                                    if nested_id == id
                                        || nested_id == main_id
                                        || *nested_read != lane + 0x200
                                    {
                                        return Err("std.scoped.nested-stack-read-or-identity");
                                    }
                                    nested_write.store(lane + 0x400, Ordering::Release);
                                    std::thread::yield_now();
                                    if std::thread::current().id() != nested_id {
                                        return Err("std.scoped.nested-stable-identity");
                                    }
                                    Ok(nested_id)
                                })
                                .map_err(|_| "std.scoped.nested-spawn")?
                                .join()
                                .map_err(|_| "std.scoped.nested-join")?
                        })?;
                        if nested_id == id || nested_write.load(Ordering::Acquire) != lane + 0x400 {
                            return Err("std.scoped.nested-stack-write");
                        }
                        let mut values =
                            stack_values.lock().map_err(|_| "std.scoped.stack-lock")?;
                        values.0 += lane + 1;
                        std::thread::yield_now();
                        values.1 += 1;
                        drop(values);
                        stack_writes.fetch_add(lane + 1, Ordering::AcqRel);
                        if std::thread::current().id() != id
                            || TLS_PROBE.with(|probe| probe.0.get()) != lane + 10
                        {
                            return Err("std.scoped.identity-or-tls");
                        }
                        Ok(id)
                    })
                    .map_err(|_| "std.scoped.spawn")?,
            );
        }
        jobs.into_iter()
            .map(|job| job.join().map_err(|_| "std.scoped.join")?)
            .collect::<Result<Vec<_>, &'static str>>()
    })?;
    if ids.len() != LANES || ids[0] == ids[1] || ids.contains(&main_id) {
        return Err("std.scoped.distinct-identities");
    }
    if stack_writes.load(Ordering::Acquire) != 3
        || *stack_values.lock().map_err(|_| "std.scoped.stack-lock")? != (0x103, LANES)
    {
        return Err("std.scoped.stack-writes");
    }
    if TLS_PROBE.with(|probe| probe.0.get()) != 0x55
        || TLS_DESTRUCTORS.load(Ordering::Acquire) != initial_drops + LANES
    {
        return Err("std.scoped.tls-destructors");
    }
    logl::log(
        level::INFO,
        format_args!(
            "tokio_mrt: std scoped={LANES} borrowed_stack=PASS tls_destructors={LANES} nested_threads={LANES}"
        ),
    );
    Ok(())
}

fn run_multi_thread_runtimes() -> Result<(), &'static str> {
    for wave in 0..WAVES {
        let before = std::time::Instant::now();
        logl::log(
            level::INFO,
            format_args!("tokio_mrt: phase=multi-thread.build.start wave={wave}"),
        );
        let initial_drops = TLS_DESTRUCTORS.load(Ordering::Acquire);
        let started = Arc::new(AtomicUsize::new(0));
        let stopped = Arc::new(AtomicUsize::new(0));
        let starts = started.clone();
        let stops = stopped.clone();
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(LANES)
            .max_blocking_threads(4)
            .thread_name(format!("mrt-tokio-{wave}"))
            .on_thread_start(move || {
                starts.fetch_add(1, Ordering::AcqRel);
                TLS_PROBE.with(|probe| probe.0.set(wave + 100));
            })
            .on_thread_stop(move || {
                stops.fetch_add(1, Ordering::AcqRel);
            })
            .enable_all()
            .build()
            .map_err(|error| {
                logl::log(
                    level::ERROR,
                    format_args!("tokio_mrt: multi-thread.build wave={wave} error={error:?}"),
                );
                "multi-thread.build"
            })?;
        logl::log(
            level::INFO,
            format_args!("tokio_mrt: phase=multi-thread.build.ready wave={wave}"),
        );
        let result = runtime.block_on(async {
            tokio::time::timeout(DEADLINE, probe_multi_thread(wave))
                .await
                .map_err(|_| "multi-thread.timeout")?
        });
        logl::log(
            level::INFO,
            format_args!(
                "tokio_mrt: phase=multi-thread.shutdown.start wave={wave} result={result:?} elapsed_ms={}",
                before.elapsed().as_millis()
            ),
        );
        runtime.shutdown_timeout(DEADLINE);
        let starts = started.load(Ordering::Acquire);
        let stops = stopped.load(Ordering::Acquire);
        let drops = TLS_DESTRUCTORS.load(Ordering::Acquire) - initial_drops;
        logl::log(
            level::INFO,
            format_args!(
                "tokio_mrt: phase=multi-thread.shutdown.done wave={wave} started={starts} stopped={stops} tls_destructors={drops} elapsed_ms={}",
                before.elapsed().as_millis()
            ),
        );
        if starts < LANES || starts != stops || starts != drops {
            return Err("multi-thread.shutdown-or-tls-destructors");
        }
        result?;
        logl::log(
            level::INFO,
            format_args!(
                "tokio_mrt: multi_thread wave={wave} started={starts} stopped={stops} tls_destructors={drops} blocking=16 socket=PASS"
            ),
        );
    }
    Ok(())
}

async fn probe_multi_thread(wave: usize) -> Result<(), &'static str> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    logl::log(
        level::INFO,
        format_args!("tokio_mrt: phase=multi-thread.tasks.start wave={wave}"),
    );
    let mut tasks = tokio::task::JoinSet::new();
    for task in 0..TASKS {
        tasks.spawn(async move {
            let mut sum = 0u64;
            for round in 0..8 {
                tokio::task::yield_now().await;
                tokio::time::sleep(Duration::from_millis(1)).await;
                sum += (task * 8 + round + 1) as u64;
            }
            sum
        });
    }
    let mut sum = 0u64;
    while let Some(result) = tasks.join_next().await {
        sum += result.map_err(|_| "multi-thread.task.join")?;
    }
    let count = (TASKS * 8) as u64;
    if sum != count * (count + 1) / 2 {
        return Err("multi-thread.task.checksum");
    }
    logl::log(
        level::INFO,
        format_args!("tokio_mrt: phase=multi-thread.tasks.done wave={wave} checksum={sum}"),
    );

    logl::log(
        level::INFO,
        format_args!("tokio_mrt: phase=multi-thread.blocking.start wave={wave}"),
    );
    let mut blocking = tokio::task::JoinSet::new();
    for task in 0..TASKS {
        blocking.spawn_blocking(move || {
            std::thread::sleep(Duration::from_millis(2));
            if TLS_PROBE.with(|probe| probe.0.get()) != wave + 100 {
                return Err("multi-thread.blocking.tls");
            }
            Ok((task + 1) as u64)
        });
    }
    let mut blocking_sum = 0u64;
    while let Some(result) = blocking.join_next().await {
        blocking_sum += result.map_err(|_| "multi-thread.blocking.join")??;
    }
    if blocking_sum != (TASKS * (TASKS + 1) / 2) as u64 {
        return Err("multi-thread.blocking.checksum");
    }
    logl::log(
        level::INFO,
        format_args!(
            "tokio_mrt: phase=multi-thread.blocking.done wave={wave} checksum={blocking_sum}"
        ),
    );
    logl::log(
        level::INFO,
        format_args!("tokio_mrt: phase=multi-thread.block-in-place.start wave={wave}"),
    );
    let in_place = tokio::spawn(async move {
        tokio::task::block_in_place(|| {
            std::thread::sleep(Duration::from_millis(2));
            TLS_PROBE.with(|probe| probe.0.get())
        })
    })
    .await
    .map_err(|_| "multi-thread.block-in-place.join")?;
    if in_place != wave + 100 {
        return Err("multi-thread.block-in-place.tls");
    }
    logl::log(
        level::INFO,
        format_args!("tokio_mrt: phase=multi-thread.block-in-place.done wave={wave}"),
    );

    logl::log(
        level::INFO,
        format_args!("tokio_mrt: phase=multi-thread.tcp.bind.start wave={wave}"),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .map_err(|error| {
            logl::log(
                level::ERROR,
                format_args!("tokio_mrt: tcp.bind wave={wave} error={error:?}"),
            );
            "multi-thread.tcp.bind"
        })?;
    let address = listener
        .local_addr()
        .map_err(|_| "multi-thread.tcp.address")?;
    logl::log(
        level::INFO,
        format_args!(
            "tokio_mrt: phase=multi-thread.tcp.connect.start wave={wave} address={address}"
        ),
    );
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener
            .accept()
            .await
            .map_err(|_| "multi-thread.tcp.accept")?;
        let mut message = [0; 4];
        socket
            .read_exact(&mut message)
            .await
            .map_err(|_| "multi-thread.tcp.server.read")?;
        if message != *b"ping" {
            return Err("multi-thread.tcp.server.message");
        }
        socket
            .write_all(b"pong")
            .await
            .map_err(|_| "multi-thread.tcp.server.write")?;
        Ok::<_, &'static str>(())
    });
    let mut socket = match tokio::net::TcpStream::connect(address).await {
        Ok(socket) => socket,
        Err(error) => {
            logl::log(
                level::ERROR,
                format_args!(
                    "tokio_mrt: tcp.connect wave={wave} address={address} kind={:?} errno={:?} error={error}",
                    error.kind(),
                    error.raw_os_error()
                ),
            );
            server.abort();
            let _ = server.await;
            return Err("multi-thread.tcp.connect");
        }
    };
    logl::log(
        level::INFO,
        format_args!("tokio_mrt: phase=multi-thread.tcp.connect.done wave={wave}"),
    );
    socket
        .write_all(b"ping")
        .await
        .map_err(|_| "multi-thread.tcp.client.write")?;
    let mut message = [0; 4];
    socket
        .read_exact(&mut message)
        .await
        .map_err(|_| "multi-thread.tcp.client.read")?;
    if message != *b"pong" {
        return Err("multi-thread.tcp.client.message");
    }
    server.await.map_err(|_| "multi-thread.tcp.server.join")??;
    logl::log(
        level::INFO,
        format_args!("tokio_mrt: phase=multi-thread.tcp.done wave={wave}"),
    );
    Ok(())
}

async fn run_wave(wave: usize) -> Result<(), &'static str> {
    // Completion can arrive just before the host releases its physical lease.
    // Wait a bounded interval before the next wave, without serial fallback.
    t::time::timeout(DEADLINE, async {
        while t::worker::capacity() < LANES {
            t::time::sleep(Duration::from_millis(1)).await;
        }
    })
    .await
    .map_err(|_| "insufficient-native-capacity")?;
    let main_slot = t::worker::local_slot();
    let (tx, mut rx) = t::sync::mpsc::channel(16);
    let release = Arc::new(AtomicBool::new(false));
    let cancel = Arc::new(AtomicBool::new(false));
    let mut jobs = Vec::new();
    let mut error = None;
    for lane in 0..LANES {
        let tx = tx.clone();
        let release = release.clone();
        let cancel = cancel.clone();
        match t::worker::spawn(move || {
            let runtime = t::runtime::current_thread()
                .build()
                .map_err(|_| "worker.runtime")?;
            let result = runtime.block_on(async {
                let slot = t::worker::local_slot();
                tx.send(Event::Ready(lane, slot))
                    .await
                    .map_err(|_| "ready.send")?;
                t::time::timeout(DEADLINE, async {
                    while !release.load(Ordering::Acquire) {
                        t::time::sleep(Duration::from_millis(1)).await;
                    }
                })
                .await
                .map_err(|_| "release.timeout")?;
                if cancel.load(Ordering::Acquire) {
                    return Err("cancelled-before-start");
                }
                let mut tasks = t::task::JoinSet::new();
                for task in 0..TASKS {
                    let tx = tx.clone();
                    let cancel = cancel.clone();
                    tasks.spawn(async move {
                        for round in 0..ROUNDS {
                            if cancel.load(Ordering::Acquire) {
                                return Err("cancelled");
                            }
                            t::task::yield_now().await;
                            t::time::sleep(Duration::from_millis(1 + (round % 5) as u64)).await;
                            if t::worker::local_slot() != slot {
                                return Err("unstable-worker-slot");
                            }
                            let value = (((wave * LANES + lane) * TASKS + task) * ROUNDS
                                + round
                                + 1) as u64;
                            tx.send(Event::Step(lane, value))
                                .await
                                .map_err(|_| "step.send")?;
                        }
                        Ok::<_, &'static str>(())
                    });
                }
                while let Some(result) = tasks.join_next().await {
                    result.map_err(|_| "task.join")??;
                }
                Ok(slot)
            });
            drop(runtime);
            result
        }) {
            Ok(job) => jobs.push(job),
            Err(_) => {
                error = Some("worker.submit");
                break;
            }
        }
    }
    drop(tx);
    let mut slots = [None; LANES];
    if error.is_none() {
        for _ in 0..LANES {
            match t::time::timeout(DEADLINE, rx.recv()).await {
                Ok(Some(Event::Ready(lane, slot))) if lane < LANES && slots[lane].is_none() => {
                    slots[lane] = Some(slot)
                }
                _ => {
                    error = Some("ready.timeout-or-protocol");
                    break;
                }
            }
        }
    }
    if error.is_some() {
        cancel.store(true, Ordering::Release);
        rx.close();
    }
    release.store(true, Ordering::Release);
    let mut counts = [0usize; LANES];
    let mut checksum = 0u64;
    let started = t::time::Instant::now();
    let receive_result = t::time::timeout(DEADLINE, async {
        while let Some(event) = rx.recv().await {
            match event {
                Event::Step(lane, value) if lane < LANES => {
                    counts[lane] += 1;
                    checksum += value;
                }
                _ => {
                    error.get_or_insert("event.protocol");
                }
            }
        }
    })
    .await;
    if receive_result.is_err() {
        error.get_or_insert("progress.timeout");
        cancel.store(true, Ordering::Release);
        rx.close(); // release bounded sends before draining accepted workers
    }
    for (lane, mut job) in jobs.into_iter().enumerate() {
        let joined = match t::time::timeout(DEADLINE, &mut job).await {
            Ok(result) => result,
            Err(_) => {
                error.get_or_insert("join.timeout");
                logl::log(
                    level::ERROR,
                    format_args!(
                        "tokio_mrt: FAIL wave={wave} lane={lane} stage=join.timeout action=draining"
                    ),
                );
                cancel.store(true, Ordering::Release);
                job.await
            }
        };
        match joined {
            Ok(Ok(slot)) if slots[lane] == Some(slot) => {}
            Ok(Err(stage)) => {
                error.get_or_insert(stage);
            }
            _ => {
                error.get_or_insert("join.result");
            }
        }
    }
    let count = (LANES * TASKS * ROUNDS) as u64;
    let first = wave as u64 * count + 1;
    let expected_checksum = count * (2 * first + count - 1) / 2;
    if counts != [TASKS * ROUNDS; LANES] || checksum != expected_checksum {
        error.get_or_insert("count-or-checksum");
    }
    if slots[0].is_none()
        || slots[1].is_none()
        || slots[0] == slots[1]
        || slots.contains(&Some(main_slot))
    {
        error.get_or_insert("distinct-worker-slots");
    }
    logl::log(
        level::INFO,
        format_args!(
            "tokio_mrt: wave={wave} counts={counts:?} checksum={checksum} elapsed_ms={} slots={slots:?}",
            started.elapsed().as_millis()
        ),
    );
    error.map_or(Ok(()), Err)
}
