use specs::{Builder, ParJoin, WorldExt};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc, Condvar, Mutex,
};
use std::time::Duration;
use tokio_parallel::{join, prelude::*, scope, ThreadPool};
use trueos::logl::{self, level};
mod generation {
    include!(concat!(env!("CARGO_WORKSPACE_DIR"), "/world/src/generation.rs"));
}
struct Position(usize);
impl specs::Component for Position {
    type Storage = specs::VecStorage<Self>;
}
struct Increment;
impl<'a> specs::System<'a> for Increment {
    type SystemData = specs::WriteStorage<'a, Position>;
    fn run(&mut self, mut data: Self::SystemData) {
        (&mut data).par_join().for_each(|p| p.0 += 1);
    }
}
struct Sum;
impl<'a> specs::System<'a> for Sum {
    type SystemData = (specs::ReadStorage<'a, Position>, specs::Write<'a, usize>);
    fn run(&mut self, (data, mut result): Self::SystemData) {
        *result = (&data).par_join().map(|p| p.0).sum();
    }
}

// No logging or manual yields inside these loops: VMCall boundaries must not
// hide a failed completion wake or a continuation that never resumes.
fn completion_progress() {
    const THREADS: usize = 8;
    const ROUNDS: usize = 128;
    let ring = Arc::new((Mutex::new(0usize), Condvar::new()));
    let mut threads = Vec::new();
    for lane in 1..THREADS {
        let ring = ring.clone();
        threads.push(std::thread::spawn(move || ring_lane(&ring, lane, THREADS, ROUNDS)));
    }
    ring_lane(&ring, 0, THREADS, ROUNDS);
    for thread in threads { thread.join().unwrap(); }
    assert_eq!(*ring.0.lock().unwrap(), THREADS * ROUNDS);
    logl::log(level::INFO, format_args!(
        "veloren_executor: progress condvar_threads=8 handoffs=1024 hull_and_native=PASS"));

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(4).enable_all().build().unwrap();
    let pool = Arc::new(ThreadPool::from_handle(runtime.handle().clone()));
    // The left branch cannot return until a different worker has taken the
    // right branch. Thus Job::wait cannot satisfy this join by claiming it.
    for caller in 0..2 {
        let executor = pool.clone();
        let work = move || executor.install(|| {
            for round in 0..ROUNDS {
                let (claimed, claim) = std::sync::mpsc::channel();
                let left_done = std::sync::atomic::AtomicBool::new(false);
                let left_done = &left_done;
                let (left, right) = join(move || {
                    claim.recv_timeout(Duration::from_secs(5)).unwrap();
                    left_done.store(true, Ordering::Release);
                    round
                }, || {
                    claimed.send(()).unwrap();
                    while !left_done.load(Ordering::Acquire) {
                        std::thread::sleep(Duration::from_millis(1));
                    }
                    std::thread::sleep(Duration::from_millis(2));
                    round * 3
                });
                assert_eq!((left, right), (round, round * 3));
            }
        });
        if caller == 0 { work(); }
        else { runtime.block_on(runtime.spawn(async move { work(); })).unwrap(); }
    }
    drop(pool);
    drop(runtime);
    logl::log(level::INFO, format_args!(
        "veloren_executor: progress claimed_joins=256 callers=hull,native workers=4 PASS"));
}

fn ring_lane(ring: &(Mutex<usize>, Condvar), lane: usize, lanes: usize, rounds: usize) {
    for _ in 0..rounds {
        let mut turn = ring.0.lock().unwrap();
        while *turn % lanes != lane { turn = ring.1.wait(turn).unwrap(); }
        *turn += 1;
        ring.1.notify_all();
    }
}

fn cpu_progress(mode: usize) {
    use std::sync::atomic::AtomicBool;
    let running = Arc::new(AtomicBool::new(true));
    let beats = Arc::new([AtomicUsize::new(0), AtomicUsize::new(0)]);
    let mut monitors = Vec::new();
    for lane in 0..2 {
        let running = running.clone();
        let beats = beats.clone();
        monitors.push(std::thread::spawn(move || {
            while running.load(Ordering::Acquire) {
                beats[lane].fetch_add(1, Ordering::Release);
                std::thread::sleep(Duration::from_millis(2));
            }
        }));
    }
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2).enable_all().build().unwrap();
    let pool = Arc::new(ThreadPool::from_handle(runtime.handle().clone()));
    let gate = Arc::new(std::sync::Barrier::new(3));
    let mut tasks = Vec::new();
    for _ in 0..2 {
        let gate = gate.clone();
        let pool = pool.clone();
        let beats = beats.clone();
        tasks.push(runtime.spawn(async move {
            gate.wait();
            let initial = [beats[0].load(Ordering::Acquire), beats[1].load(Ordering::Acquire)];
            let deadline = std::time::Instant::now() + Duration::from_secs(1);
            if mode == 1 {
                while std::time::Instant::now() < deadline {
                    let slice = std::time::Instant::now() + Duration::from_micros(250);
                    while std::time::Instant::now() < slice { std::hint::black_box(17usize); }
                    // Tokio yields its task, but the native worker must also
                    // eventually give sleeping std peers a carrier turn.
                    tokio::task::yield_now().await;
                }
            } else if mode == 2 {
                let mut total = 0usize;
                generation::for_each_site(0..4000, |site| {
                    let slice = std::time::Instant::now() + Duration::from_micros(250);
                    while std::time::Instant::now() < slice { std::hint::black_box(site); }
                    total += site;
                });
                assert_eq!(total, (0..4000).sum::<usize>());
            } else if mode == 3 {
                let samples = generation::collect_ordered(0..4000, |position| {
                    let slice = std::time::Instant::now() + Duration::from_micros(250);
                    while std::time::Instant::now() < slice { std::hint::black_box(position); }
                    (position % 3 != 0).then_some(position)
                });
                assert_eq!(samples.len(), 4000);
                for (position, sample) in samples.into_iter().enumerate() {
                    assert_eq!(sample, (position % 3 != 0).then_some(position));
                }
            } else { pool.install(|| {
                while std::time::Instant::now() < deadline {
                    join(|| {
                        let slice = std::time::Instant::now() + Duration::from_micros(250);
                        while std::time::Instant::now() < slice { std::hint::black_box(17usize); }
                    }, || std::hint::black_box(7usize));
                }
            }); }
            [beats[0].load(Ordering::Acquire) - initial[0], beats[1].load(Ordering::Acquire) - initial[1]]
        }));
    }
    gate.wait();
    let counts = tasks.into_iter().map(|task| runtime.block_on(task).unwrap()).collect::<Vec<_>>();
    running.store(false, Ordering::Release);
    for monitor in monitors { monitor.join().unwrap(); }
    drop(pool);
    drop(runtime);
    let mode = match mode { 1 => "async-yield", 2 => "worldgen-sites", 3 => "worldgen-map", _ => "parallel-jobs" };
    if counts.iter().any(|counts| counts.iter().any(|count| *count < 10)) {
        logl::log(level::ERROR, format_args!("veloren_executor: FAIL mode={mode} cpu-bound heartbeat_counts={counts:?}"));
        panic!("CPU jobs starved carrier peers");
    }
    logl::log(level::INFO, format_args!(
        "veloren_executor: progress cpu_jobs=2 heartbeat_peers=2 bounded_turns=PASS mode={mode} counts={counts:?}"));
}

fn main() {
    logl::log(
        level::INFO,
        format_args!("veloren_executor: start shared-tokio-runtime"),
    );
    completion_progress();
    cpu_progress(0);
    cpu_progress(1);
    cpu_progress(2);
    cpu_progress(3);
    for workers in [1, 2] {
        logl::log(
            level::INFO,
            format_args!("veloren_executor: stage=build workers={workers}"),
        );
        let runtime = Arc::new(
            tokio::runtime::Builder::new_multi_thread()
                .worker_threads(workers)
                .enable_all()
                .build()
                .unwrap(),
        );
        logl::log(
            level::INFO,
            format_args!("veloren_executor: stage=built workers={workers}"),
        );
        logl::log(
            level::INFO,
            format_args!("veloren_executor: stage=pool workers={workers}"),
        );
        let pool = Arc::new(ThreadPool::from_runtime(runtime.clone()));
        let complete = Arc::new(AtomicUsize::new(0));
        let result = complete.clone();
        let stage = Arc::new(AtomicUsize::new(0));
        let progress = stage.clone();
        let executor = pool.clone();
        logl::log(
            level::INFO,
            format_args!("veloren_executor: stage=spawn workers={workers}"),
        );
        let task = runtime.spawn(async move {
            progress.store(10, Ordering::Release);
            let mut world = specs::World::new();
            progress.store(20, Ordering::Release);
            world.register::<Position>();
            world.insert(0usize);
            for i in 0..256 {
                world.create_entity().with(Position(i)).build();
            }
            progress.store(30, Ordering::Release);
            let mut dispatcher = specs::DispatcherBuilder::new()
                .with_pool(executor.clone())
                .with(Increment, "increment", &[])
                .with(Sum, "sum", &["increment"])
                .build();
            dispatcher.setup(&mut world);
            progress.store(40, Ordering::Release);
            for tick in 1..=32 {
                progress.store(50, Ordering::Release);
                dispatcher.dispatch(&world);
                progress.store(60, Ordering::Release);
                assert_eq!(
                    *world.read_resource::<usize>(),
                    (0..256).sum::<usize>() + tick * 256
                );
                executor.install(|| {
                    progress.store(70, Ordering::Release);
                    let mut values = [0usize; 64];
                    values.par_iter_mut().enumerate().for_each(|(i, value)| {
                        *value = join(|| i, || (0..32).into_par_iter().sum::<usize>()).0;
                    });
                    assert_eq!(values.iter().sum::<usize>(), (0..64).sum::<usize>());
                    let mut left = 0;
                    let mut right = 0;
                    progress.store(80, Ordering::Release);
                    scope(|s| {
                        s.spawn(|s| s.spawn(|_| left = 7));
                        s.spawn(|_| right = 11);
                    });
                    assert_eq!((left, right), (7, 11));
                    progress.store(90, Ordering::Release);
                });
                result.fetch_add(1, Ordering::Release);
            }
        });
        logl::log(
            level::INFO,
            format_args!("veloren_executor: stage=block-on workers={workers}"),
        );
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        let mut previous = usize::MAX;
        while complete.load(Ordering::Acquire) != 32 {
            let current = stage.load(Ordering::Acquire);
            if current != previous {
                logl::log(
                    level::INFO,
                    format_args!(
                        "veloren_executor: progress workers={workers} stage={current} ticks={}",
                        complete.load(Ordering::Acquire)
                    ),
                );
                previous = current;
            }
            if std::time::Instant::now() >= deadline {
                logl::log(
                    level::ERROR,
                    format_args!(
                        "veloren_executor: FAIL workers={workers} stage={current} ticks={}",
                        complete.load(Ordering::Acquire)
                    ),
                );
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        runtime.block_on(task).unwrap();
        assert_eq!(runtime.metrics().num_workers(), workers);
        logl::log(
            level::INFO,
            format_args!("veloren_executor: wave workers={workers} ticks=32 borrowed=64 scopes=32"),
        );
    }
    logl::log(
        level::INFO,
        format_args!("veloren_executor: PASS workers=1,2 ecs_ticks=64 borrowed=4096 scopes=64"),
    );
}
