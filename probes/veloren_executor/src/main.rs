use specs::{Builder, ParJoin, WorldExt};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};
use std::time::Duration;
use tokio_parallel::{join, prelude::*, scope, ThreadPool};
use trueos::logl::{self, level};
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
fn main() {
    logl::log(
        level::INFO,
        format_args!("veloren_executor: start shared-tokio-runtime"),
    );
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
