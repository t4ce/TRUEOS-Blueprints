use trueos::{
    bp_error, bp_info,
    platform::{future, thread, Arc},
    t,
};

fn main() {
    bp_info!("tokio_rt: start");

    if let Err(stage) = probe_runtime_bootstrap_surfaces() {
        bp_error!("tokio_rt: bootstrap failed stage={}", stage);
        return;
    }

    bp_info!("tokio_rt: stage runtime.current_thread.build");
    let runtime = match t::runtime::current_thread().build() {
        Ok(rt) => rt,
        Err(err) => {
            bp_error!("tokio_rt: runtime build failed: {}", err);
            return;
        }
    };
    bp_info!("tokio_rt: success runtime.current_thread.build");

    runtime.block_on(async {
        match run_probe().await {
            Ok(()) => bp_info!("tokio_rt: done"),
            Err(stage) => bp_error!("tokio_rt: failed stage={}", stage),
        }
    });

    bp_info!("tokio_rt: stage runtime.current_thread.drop");
    drop(runtime);
    bp_info!("tokio_rt: success runtime.current_thread.drop");
}

fn probe_runtime_bootstrap_surfaces() -> Result<(), &'static str> {
    bp_info!("tokio_rt: stage thread.current.id");
    let thread_id = thread::current().id();
    bp_info!("tokio_rt: success thread.current.id id={:?}", thread_id);

    bp_info!("tokio_rt: stage thread.yield_now");
    thread::yield_now();
    bp_info!("tokio_rt: success thread.yield_now");

    bp_info!("tokio_rt: stage runtime.current_thread.builder_new_plain");
    let mut builder = t::tokio::runtime::Builder::new_current_thread();
    bp_info!("tokio_rt: success runtime.current_thread.builder_new_plain");

    bp_info!("tokio_rt: stage runtime.current_thread.builder_build_plain");
    let runtime = builder
        .build()
        .map_err(|_| "runtime.current_thread.builder_build_plain")?;
    bp_info!("tokio_rt: success runtime.current_thread.build_plain");

    bp_info!("tokio_rt: stage runtime.current_thread.drop_plain");
    drop(runtime);
    bp_info!("tokio_rt: success runtime.current_thread.drop_plain");

    bp_info!("tokio_rt: stage runtime.current_thread.build_time");
    let runtime = t::runtime::current_thread()
        .build()
        .map_err(|_| "runtime.current_thread.build_time")?;
    bp_info!("tokio_rt: success runtime.current_thread.build_time");

    bp_info!("tokio_rt: stage runtime.current_thread.block_on_smoke");
    let value = runtime.block_on(async { 0x5254_0001u32 });
    if value != 0x5254_0001 {
        return Err("runtime.current_thread.block_on_smoke.value");
    }
    bp_info!("tokio_rt: success runtime.current_thread.block_on_smoke");

    bp_info!("tokio_rt: stage runtime.current_thread.drop_time");
    drop(runtime);
    bp_info!("tokio_rt: success runtime.current_thread.drop_time");

    Ok(())
}

async fn run_probe() -> Result<(), &'static str> {
    bp_info!("tokio_rt: stage rt.task.yield_now");
    t::task::yield_now().await;
    bp_info!("tokio_rt: success rt.task.yield_now");

    bp_info!("tokio_rt: stage rt.task.spawn_join");
    let join = t::task::spawn(async { 0xA11C_Eu32 });
    let join_value = join.await.map_err(|_| "rt.task.spawn_join.await")?;
    if join_value != 0xA11C_E {
        return Err("rt.task.spawn_join.value");
    }
    bp_info!("tokio_rt: success rt.task.spawn_join");

    bp_info!("tokio_rt: stage rt.task.localset_spawn_local");
    let local = t::task::LocalSet::new();
    let local_value = local
        .run_until(async {
            let local_join = t::tokio::task::spawn_local(async { 0x10CA_1E7u32 });
            local_join
                .await
                .map_err(|_| "rt.task.localset_spawn_local.join")
        })
        .await?;
    if local_value != 0x10CA_1E7 {
        return Err("rt.task.localset_spawn_local.value");
    }
    bp_info!("tokio_rt: success rt.task.localset_spawn_local");

    bp_info!("tokio_rt: stage rt.task.join_set");
    let mut join_set = t::task::JoinSet::new();
    join_set.spawn(async { 0x11u32 });
    join_set.spawn(async { 0x22u32 });
    let mut sum = 0u32;
    for _ in 0..2 {
        let joined = join_set.join_next().await.ok_or("rt.task.join_set.empty")?;
        sum = sum.wrapping_add(joined.map_err(|_| "rt.task.join_set.join")?);
    }
    if sum != 0x33 {
        return Err("rt.task.join_set.value");
    }
    bp_info!("tokio_rt: success rt.task.join_set");

    bp_info!("tokio_rt: stage rt.task.join_macro");
    let (left, right) = t::tokio::join!(
        async {
            t::task::yield_now().await;
            0x4A4F_494Eu32
        },
        async { 0x4D41_4352u32 },
    );
    if left != 0x4A4F_494E || right != 0x4D41_4352 {
        return Err("rt.task.join_macro.value");
    }
    bp_info!("tokio_rt: success rt.task.join_macro");

    bp_info!("tokio_rt: stage rt.task.try_join_macro");
    let (left, right) = t::tokio::try_join!(
        async {
            t::task::yield_now().await;
            Ok::<u32, &'static str>(0x5452_5931)
        },
        async { Ok::<u32, &'static str>(0x5452_5932) },
    )?;
    if left != 0x5452_5931 || right != 0x5452_5932 {
        return Err("rt.task.try_join_macro.value");
    }
    bp_info!("tokio_rt: success rt.task.try_join_macro");

    bp_info!("tokio_rt: stage rt.task.abort");
    let (_tx, rx) = t::sync::oneshot::channel::<()>();
    let abort_task = t::task::spawn(async move {
        let _ = rx.await;
        0x4142_4F52u32
    });
    abort_task.abort();
    match abort_task.await {
        Err(err) if err.is_cancelled() => bp_info!("tokio_rt: success rt.task.abort"),
        Err(_) => return Err("rt.task.abort.state"),
        Ok(_) => return Err("rt.task.abort.value"),
    }

    bp_info!("tokio_rt: stage rt.select");
    let select_value = t::tokio::select! {
        _ = t::time::sleep(t::time::Duration::from_millis(5)) => 0u32,
        value = async {
            t::task::yield_now().await;
            0x5345_4C45u32
        } => value,
    };
    if select_value != 0x5345_4C45 {
        return Err("rt.select.value");
    }
    bp_info!("tokio_rt: success rt.select");

    probe_io().await?;
    probe_sync().await?;
    probe_time().await?;

    Ok(())
}

async fn probe_sync() -> Result<(), &'static str> {
    bp_info!("tokio_rt: stage sync.oneshot");
    let (oneshot_tx, oneshot_rx) = t::sync::oneshot::channel();
    let oneshot_task = t::task::spawn(async move {
        let _ = oneshot_tx.send(0x5155u32);
    });
    oneshot_task.await.map_err(|_| "sync.oneshot.task")?;
    let value = oneshot_rx.await.map_err(|_| "sync.oneshot.recv")?;
    if value != 0x5155 {
        return Err("sync.oneshot.value");
    }
    bp_info!("tokio_rt: success sync.oneshot");

    bp_info!("tokio_rt: stage sync.mpsc");
    let (mpsc_tx, mut mpsc_rx) = t::sync::mpsc::channel(2);
    mpsc_tx
        .send(0x4D50_5343u32)
        .await
        .map_err(|_| "sync.mpsc.send")?;
    let value = mpsc_rx.recv().await.ok_or("sync.mpsc.recv")?;
    if value != 0x4D50_5343 {
        return Err("sync.mpsc.value");
    }
    bp_info!("tokio_rt: success sync.mpsc");

    bp_info!("tokio_rt: stage sync.watch");
    let (watch_tx, mut watch_rx) = t::sync::watch::channel(0u32);
    let watch_task = t::task::spawn(async move {
        watch_rx.changed().await.map_err(|_| "sync.watch.changed")?;
        Ok::<u32, &'static str>(*watch_rx.borrow())
    });
    watch_tx.send(0x5743u32).map_err(|_| "sync.watch.send")?;
    let value = watch_task.await.map_err(|_| "sync.watch.task")??;
    if value != 0x5743 {
        return Err("sync.watch.value");
    }
    bp_info!("tokio_rt: success sync.watch");

    bp_info!("tokio_rt: stage sync.broadcast");
    let (broadcast_tx, mut broadcast_rx) = t::sync::broadcast::channel(2);
    let broadcast_task =
        t::task::spawn(async move { broadcast_rx.recv().await.map_err(|_| "sync.broadcast.recv") });
    broadcast_tx
        .send(0xB04D_C457u32)
        .map_err(|_| "sync.broadcast.send")?;
    let value = broadcast_task.await.map_err(|_| "sync.broadcast.task")??;
    if value != 0xB04D_C457 {
        return Err("sync.broadcast.value");
    }
    bp_info!("tokio_rt: success sync.broadcast");

    bp_info!("tokio_rt: stage sync.notify");
    let notify = Arc::new(t::sync::Notify::new());
    let notify_wait = notify.clone();
    let notify_task = t::task::spawn(async move {
        notify_wait.notified().await;
        0x4E4F_5449u32
    });
    notify.notify_one();
    let value = notify_task.await.map_err(|_| "sync.notify.task")?;
    if value != 0x4E4F_5449 {
        return Err("sync.notify.value");
    }
    bp_info!("tokio_rt: success sync.notify");

    bp_info!("tokio_rt: stage sync.mutex");
    let mutex = Arc::new(t::sync::Mutex::new(0u32));
    let mutex_task = t::task::spawn({
        let mutex = mutex.clone();
        async move {
            let mut guard = mutex.lock().await;
            *guard = 0x4D55_5445;
        }
    });
    mutex_task.await.map_err(|_| "sync.mutex.task")?;
    if *mutex.lock().await != 0x4D55_5445 {
        return Err("sync.mutex.value");
    }
    bp_info!("tokio_rt: success sync.mutex");

    bp_info!("tokio_rt: stage sync.rwlock");
    let rwlock = Arc::new(t::sync::RwLock::new(0u32));
    {
        let mut guard = rwlock.write().await;
        *guard = 0x5257_4C4B;
    }
    if *rwlock.read().await != 0x5257_4C4B {
        return Err("sync.rwlock.value");
    }
    bp_info!("tokio_rt: success sync.rwlock");

    bp_info!("tokio_rt: stage sync.semaphore");
    let semaphore = Arc::new(t::sync::Semaphore::new(0));
    let semaphore_task = t::task::spawn({
        let semaphore = semaphore.clone();
        async move {
            let permit = semaphore
                .acquire_owned()
                .await
                .map_err(|_| "sync.semaphore.acquire")?;
            drop(permit);
            Ok::<u32, &'static str>(0x53E4_A001)
        }
    });
    t::task::yield_now().await;
    semaphore.add_permits(1);
    let value = semaphore_task.await.map_err(|_| "sync.semaphore.task")??;
    if value != 0x53E4_A001 {
        return Err("sync.semaphore.value");
    }
    bp_info!("tokio_rt: success sync.semaphore");

    bp_info!("tokio_rt: stage sync.barrier");
    let barrier = Arc::new(t::sync::Barrier::new(2));
    let barrier_task = t::task::spawn({
        let barrier = barrier.clone();
        async move {
            barrier.wait().await;
            0xBA22_1E2u32
        }
    });
    let barrier_wait = barrier.wait().await;
    let value = barrier_task.await.map_err(|_| "sync.barrier.task")?;
    if value != 0xBA22_1E2 {
        return Err("sync.barrier.value");
    }
    let _ = barrier_wait.is_leader();
    bp_info!("tokio_rt: success sync.barrier");

    Ok(())
}

async fn probe_time() -> Result<(), &'static str> {
    bp_info!("tokio_rt: stage time.instant_now");
    let now = t::time::Instant::now();
    let _ = now.checked_add(t::time::Duration::from_millis(1));
    bp_info!("tokio_rt: success time.instant_now");

    bp_info!("tokio_rt: stage time.instant_delta.spin");
    let spin_start = t::time::Instant::now();
    for _ in 0..1024 {
        core::hint::spin_loop();
    }
    let spin_delta = t::time::Instant::now().saturating_duration_since(spin_start);
    bp_info!(
        "tokio_rt: success time.instant_delta.spin nanos={}",
        spin_delta.as_nanos()
    );

    bp_info!("tokio_rt: stage time.instant_delta.after_yield");
    let yield_start = t::time::Instant::now();
    t::task::yield_now().await;
    let yield_delta = t::time::Instant::now().saturating_duration_since(yield_start);
    bp_info!(
        "tokio_rt: success time.instant_delta.after_yield nanos={}",
        yield_delta.as_nanos()
    );

    bp_info!("tokio_rt: stage time.sleep_zero.build");
    let sleep_zero = t::time::sleep(t::time::Duration::from_millis(0));
    bp_info!("tokio_rt: success time.sleep_zero.build");

    bp_info!("tokio_rt: stage time.sleep_zero.await");
    let sleep_zero_start = t::time::Instant::now();
    sleep_zero.await;
    let sleep_zero_delta = t::time::Instant::now().saturating_duration_since(sleep_zero_start);
    bp_info!(
        "tokio_rt: success time.sleep_zero.await nanos={}",
        sleep_zero_delta.as_nanos()
    );

    bp_info!("tokio_rt: stage time.sleep_one.build");
    let sleep_one = t::time::sleep(t::time::Duration::from_millis(1));
    bp_info!("tokio_rt: success time.sleep_one.build");

    bp_info!("tokio_rt: stage time.sleep_one.await");
    let sleep_one_start = t::time::Instant::now();
    sleep_one.await;
    let sleep_one_delta = t::time::Instant::now().saturating_duration_since(sleep_one_start);
    bp_info!(
        "tokio_rt: success time.sleep_one.await nanos={}",
        sleep_one_delta.as_nanos()
    );

    bp_info!("tokio_rt: stage time.timeout");
    let value = t::time::timeout(t::time::Duration::from_millis(5), async {
        t::task::yield_now().await;
        0x5449_4D45u32
    })
    .await
    .map_err(|_| "time.timeout.ok")?;
    if value != 0x5449_4D45 {
        return Err("time.timeout.value");
    }
    bp_info!("tokio_rt: success time.timeout");

    bp_info!("tokio_rt: stage time.timeout_elapsed_pending.build");
    let pending_elapsed = t::time::timeout(
        t::time::Duration::from_millis(1),
        future::pending::<u32>(),
    );
    bp_info!("tokio_rt: success time.timeout_elapsed_pending.build");

    bp_info!("tokio_rt: stage time.timeout_elapsed_pending.await");
    match pending_elapsed.await {
        Err(_) => bp_info!("tokio_rt: success time.timeout_elapsed_pending.await"),
        Ok(_) => return Err("time.timeout_elapsed_pending.value"),
    }

    bp_info!("tokio_rt: stage time.timeout_elapsed_sleep.inner_build");
    let inner_sleep = t::time::sleep(t::time::Duration::from_millis(100));
    bp_info!("tokio_rt: success time.timeout_elapsed_sleep.inner_build");

    bp_info!("tokio_rt: stage time.timeout_elapsed_sleep.outer_build");
    let timeout_sleep = t::time::timeout(t::time::Duration::from_millis(5), async {
        inner_sleep.await;
        0x4445_4144u32
    });
    bp_info!("tokio_rt: success time.timeout_elapsed_sleep.outer_build");

    bp_info!("tokio_rt: stage time.timeout_elapsed_sleep.await");
    match timeout_sleep.await {
        Err(_) => bp_info!("tokio_rt: success time.timeout_elapsed_sleep.await"),
        Ok(_) => return Err("time.timeout_elapsed_sleep.value"),
    }

    bp_info!("tokio_rt: stage time.interval");
    let mut interval = t::time::interval(t::time::Duration::from_millis(1));
    interval.tick().await;
    interval.tick().await;
    bp_info!("tokio_rt: success time.interval");

    Ok(())
}

async fn probe_io() -> Result<(), &'static str> {
    use t::io::{AsyncReadExt, AsyncWriteExt};

    bp_info!("tokio_rt: stage io.duplex");
    let (mut write_half, mut read_half) = t::io::duplex(32);
    write_half
        .write_all(b"ok")
        .await
        .map_err(|_| "io.duplex.write")?;
    let mut buf = [0u8; 2];
    read_half
        .read_exact(&mut buf)
        .await
        .map_err(|_| "io.duplex.read")?;
    if buf != *b"ok" {
        return Err("io.duplex.value");
    }
    bp_info!("tokio_rt: success io.duplex");

    bp_info!("tokio_rt: stage io.std.surface");
    let _stdin = t::io::stdin();
    let _stdout = t::io::stdout();
    let _stderr = t::io::stderr();
    bp_info!("tokio_rt: success io.std.surface");

    Ok(())
}
