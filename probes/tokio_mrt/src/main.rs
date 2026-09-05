use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
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

enum Event {
    Ready(usize, u32),
    Step(usize, u64),
}

fn main() {
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
                "tokio_mrt: PASS lanes={LANES} waves={WAVES} tasks={TASKS} rounds={ROUNDS}"
            ),
        ),
        Err(stage) => logl::log(level::ERROR, format_args!("tokio_mrt: FAIL stage={stage}")),
    }
}

async fn run_wave(wave: usize) -> Result<(), &'static str> {
    if t::worker::capacity() < LANES {
        return Err("insufficient-native-capacity");
    }
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
