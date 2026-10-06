use std::{
    panic::{AssertUnwindSafe, catch_unwind},
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use veloren_tokio_parallel::{ThreadPool, join, prelude::*, scope};
#[test]
fn background_task_can_release_last_standalone_runtime_owner() {
    let pool = Arc::new(
        veloren_tokio_parallel::ThreadPoolBuilder::new()
            .num_threads(1)
            .build()
            .unwrap(),
    );
    let last_owner = Arc::clone(&pool);
    let (release, wait) = std::sync::mpsc::channel();
    let (completed, result) = std::sync::mpsc::channel();
    pool.spawn(move || {
        wait.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(tokio::runtime::Handle::try_current().is_ok());
        let dropped = catch_unwind(AssertUnwindSafe(|| drop(last_owner)));
        completed.send(dropped.is_ok()).unwrap();
    });
    drop(pool);
    release.send(()).unwrap();
    assert!(
        result.recv_timeout(Duration::from_secs(5)).unwrap(),
        "dropping the pipeline task's last runtime owner panicked"
    );
}

fn executor(workers: usize) -> (Arc<tokio::runtime::Runtime>, Arc<ThreadPool>) {
    let runtime = Arc::new(
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(workers)
            .enable_all()
            .build()
            .unwrap(),
    );
    let pool = Arc::new(ThreadPool::from_runtime(runtime.clone()));
    (runtime, pool)
}
#[test]
fn borrowed_iterators_and_nested_work_on_one_worker() {
    let (runtime, pool) = executor(1);
    let (tx, rx) = std::sync::mpsc::channel();
    runtime.spawn(async move {
        let mut values = vec![0usize; 1024];
        pool.install(|| {
            values.par_iter_mut().enumerate().for_each(|(i, value)| {
                *value = join(|| i * 2, || (0..16).into_par_iter().sum::<usize>()).0;
            })
        });
        tx.send(values).unwrap();
    });
    let values = rx
        .recv_timeout(Duration::from_secs(5))
        .expect("nested work deadlocked");
    assert_eq!(values, (0..1024).map(|i| i * 2).collect::<Vec<_>>());
}
#[test]
fn jobs_execute_on_shared_tokio_workers() {
    let (runtime, pool) = executor(2);
    let owner = std::thread::current().id();
    let (tx, rx) = std::sync::mpsc::channel();
    pool.install(|| {
        join(
            move || rx.recv_timeout(Duration::from_secs(5)).unwrap(),
            || {
                assert_ne!(std::thread::current().id(), owner);
                assert_eq!(tokio::runtime::Handle::current().metrics().num_workers(), 2);
                tx.send(()).unwrap();
            },
        )
    });
    assert_eq!(runtime.metrics().num_workers(), pool.current_num_threads());
}

#[test]
fn already_claimed_children_wake_hull_and_worker_callers() {
    let mut builder = tokio::runtime::Builder::new_multi_thread();
    builder.worker_threads(4).enable_all();
    // The TRUEOS vendor enables this by default. On the host, opt into the
    // upstream setting when testing worker-local synchronous dependencies.
    #[cfg(tokio_unstable)]
    builder.disable_lifo_slot();
    let runtime = builder.build().unwrap();
    let pool = Arc::new(ThreadPool::from_handle(runtime.handle().clone()));
    for worker_caller in [false, true] {
        if worker_caller && !cfg!(any(tokio_unstable, target_os = "trueos")) { continue; }
        let pool = pool.clone();
        let work = move || pool.install(|| {
            for round in 0..128 {
                let (claimed, claim) = std::sync::mpsc::channel();
                let left_done = std::sync::atomic::AtomicBool::new(false);
                let left_done = &left_done;
                let values = join(move || {
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
                assert_eq!(values, (round, round * 3));
            }
        });
        let (done, completed) = std::sync::mpsc::channel();
        if worker_caller {
            runtime.spawn(async move { work(); done.send(()).unwrap(); });
        } else {
            std::thread::spawn(move || { work(); done.send(()).unwrap(); });
        }
        completed.recv_timeout(Duration::from_secs(5)).expect("claimed child failed to wake its caller");
    }
}
#[test]
fn scope_drains_descendants_and_borrowed_writes() {
    let (_runtime, pool) = executor(2);
    let mut left = 0;
    let mut right = 0;
    pool.install(|| {
        scope(|s| {
            s.spawn(|s| {
                s.spawn(|_| left = 7);
            });
            s.spawn(|_| right = 11);
        })
    });
    assert_eq!((left, right), (7, 11));
}
#[test]
fn panicking_join_and_scope_complete_all_borrowed_children() {
    let (_runtime, pool) = executor(2);
    let completed = AtomicUsize::new(0);
    assert!(
        catch_unwind(AssertUnwindSafe(|| pool.install(|| join(
            || panic!("left"),
            || {
                completed.fetch_add(1, Ordering::SeqCst);
            }
        ))))
        .is_err()
    );
    assert!(
        catch_unwind(AssertUnwindSafe(|| pool.install(|| scope(|s| {
            s.spawn(|_| panic!("child"));
            s.spawn(|s| {
                s.spawn(|_| {
                    completed.fetch_add(1, Ordering::SeqCst);
                });
            });
            panic!("body");
        }))))
        .is_err()
    );
    assert_eq!(completed.load(Ordering::SeqCst), 2);
}
#[test]
fn iterator_algorithms_preserve_results() {
    let (_runtime, pool) = executor(3);
    pool.install(|| {
        let result: Vec<_> = (0..400)
            .into_par_iter()
            .filter(|i| i % 3 == 0)
            .map(|i| i * i)
            .collect();
        assert_eq!(
            result,
            (0..400)
                .filter(|i| i % 3 == 0)
                .map(|i| i * i)
                .collect::<Vec<_>>()
        );
        let mut values = vec![5, 3, 8, 1, 1];
        values.par_sort_unstable();
        assert_eq!(values, vec![1, 1, 3, 5, 8]);
        assert_eq!(
            (0..400).par_bridge().sum::<usize>(),
            (0..400).sum::<usize>()
        );
        assert_eq!(
            (0..100).into_par_iter().skip(50).sum::<usize>(),
            (50..100).sum::<usize>()
        );
    });
}
struct WriteA;
impl<'a> shred::System<'a> for WriteA {
    type SystemData = shred::Write<'a, usize>;

    fn run(&mut self, mut data: Self::SystemData) { *data = 9; }
}
struct ReadA;
impl<'a> shred::System<'a> for ReadA {
    type SystemData = (shred::Read<'a, usize>, shred::Write<'a, Vec<usize>>);

    fn run(&mut self, (data, mut output): Self::SystemData) { output.push(*data); }
}
#[test]
fn ecs_dispatch_preserves_dependencies_and_resource_borrows() {
    let (_runtime, pool) = executor(2);
    let mut world = shred::World::empty();
    world.insert(0usize);
    world.insert(Vec::<usize>::new());
    let mut dispatcher = shred::DispatcherBuilder::new()
        .with_pool(pool)
        .with(WriteA, "write", &[])
        .with(ReadA, "read", &["write"])
        .build();
    dispatcher.setup(&mut world);
    for _ in 0..20 {
        dispatcher.dispatch(&world);
    }
    assert_eq!(&*world.fetch::<Vec<usize>>(), &vec![9; 20]);
}
#[test]
fn specs_parallel_join_mutates_every_component() {
    use specs::{Builder, ParJoin, WorldExt};
    struct Position(usize);
    impl specs::Component for Position {
        type Storage = specs::VecStorage<Self>;
    }
    let (_runtime, pool) = executor(2);
    let mut world = specs::World::new();
    world.register::<Position>();
    for i in 0..500 {
        world.create_entity().with(Position(i)).build();
    }
    pool.install(|| {
        (&mut world.write_storage::<Position>())
            .par_join()
            .for_each(|p| p.0 += 2)
    });
    let sum = pool.install(|| {
        world
            .read_storage::<Position>()
            .par_join()
            .map(|p| p.0)
            .sum::<usize>()
    });
    assert_eq!(sum, (0..500).map(|i| i + 2).sum::<usize>());
}

#[test]
fn borrowed_jobs_complete_after_runtime_shutdown() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()
        .unwrap();
    let pool = ThreadPool::from_handle(runtime.handle().clone());
    runtime.shutdown_background();
    let text = String::from("borrowed");
    let result = pool.install(|| join(|| &text[..], || &text[..]));
    assert_eq!(result, ("borrowed", "borrowed"));
    let mut value = 0;
    pool.install(|| scope(|s| s.spawn(|_| value = 19)));
    assert_eq!(value, 19);
}
