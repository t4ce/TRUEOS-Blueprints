//! Tokio task execution for the retained parallel iterator algorithms.
//!
//! CPU closures never await. A scope retains each submitted job until its
//! closure has been dropped, including during unwinding or runtime shutdown.
//! A waiting caller may claim its own unstarted child; this prevents nested
//! joins from parking every Tokio worker behind work only those workers can
//! run.
#[allow(missing_debug_implementations)]
mod implementation {
    use std::{
        any::Any,
        cell::RefCell,
        collections::VecDeque,
        marker::PhantomData,
        panic::{AssertUnwindSafe, catch_unwind, resume_unwind},
        sync::{Arc, Condvar, Mutex, OnceLock},
    };
    use tokio::runtime::{Builder, Runtime};

    type Panic = Box<dyn Any + Send + 'static>;
    type Work<'a> = Box<dyn FnOnce() + Send + 'a>;

    #[derive(Clone)]
    /// Parallel executor using a shared Tokio runtime, with no separate CPU
    /// worker pool.
    pub struct ThreadPool {
        handle: tokio::runtime::Handle,
        _owner: Option<Arc<RuntimeOwner>>,
    }

    // User closures may retain a pool, even though our task execution context
    // only retains a Handle. The last owning closure can finish on a worker of
    // this very runtime (for example background renderer pipeline creation).
    // Share this guard across pool clones so concurrent drops have one shutdown
    // owner, rather than racing separate Arc::try_unwrap calls.
    struct RuntimeOwner(Option<Arc<Runtime>>);
    impl Drop for RuntimeOwner {
        fn drop(&mut self) {
            if let Some(runtime) = self.0.take().and_then(Arc::into_inner) {
                if tokio::runtime::Handle::try_current().is_ok() {
                    runtime.shutdown_background();
                } else {
                    drop(runtime);
                }
            }
        }
    }
    impl std::fmt::Debug for ThreadPool {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("TokioParallelExecutor")
                .field("workers", &self.current_num_threads())
                .finish()
        }
    }
    thread_local! { static CURRENT: RefCell<Option<ThreadPool>> = const { RefCell::new(None) }; }
    #[cfg(target_os = "trueos")]
    thread_local! {
        static LAST_CARRIER_TURN: std::cell::Cell<Option<std::time::Instant>> = const {
            std::cell::Cell::new(None)
        };
    }

    // Tokio's async poll budget cannot interrupt a synchronous CPU closure.
    // TRUEOS std workers also share cooperative carriers. Give their peers a
    // turn at our lock-free job boundaries, even when every child is claimed
    // inline and no condition-variable wait occurs. This bounds uninterrupted
    // sequences of small jobs, not the duration of an individual user closure.
    #[inline]
    fn carrier_checkpoint() {
        #[cfg(target_os = "trueos")]
        {
            let now = std::time::Instant::now();
            let yield_turn = LAST_CARRIER_TURN.with(|last| match last.get() {
                Some(previous) => now.duration_since(previous) >= std::time::Duration::from_millis(10),
                None => { last.set(Some(now)); false },
            });
            if yield_turn {
                std::thread::yield_now();
                // Time spent parked is not CPU time spent monopolizing a lane.
                LAST_CARRIER_TURN.with(|last| last.set(Some(std::time::Instant::now())));
            }
        }
    }
    static DEFAULT: OnceLock<ThreadPool> = OnceLock::new();
    static SHARED: Mutex<Option<tokio::runtime::Handle>> = Mutex::new(None);

    struct Enter(Option<ThreadPool>);
    impl Drop for Enter {
        fn drop(&mut self) { CURRENT.with(|slot| *slot.borrow_mut() = self.0.take()); }
    }
    impl ThreadPool {
        /// Select the application's existing runtime for parallel calls outside
        /// an explicit install scope. The application must keep this
        /// runtime alive.
        pub fn set_shared_runtime(runtime: &Runtime) {
            *SHARED.lock().unwrap() = Some(runtime.handle().clone());
        }

        /// Retain runtime ownership and execute work on its existing workers.
        pub fn from_runtime(runtime: Arc<Runtime>) -> Self {
            Self {
                handle: runtime.handle().clone(),
                _owner: Some(Arc::new(RuntimeOwner(Some(runtime)))),
            }
        }

        /// Bind an existing handle without owning its runtime. The owner
        /// controls runtime lifetime; scoped jobs are completed even if
        /// that runtime stops.
        pub fn from_handle(handle: tokio::runtime::Handle) -> Self {
            Self {
                handle,
                _owner: None,
            }
        }

        // Executing tasks retain a Handle, not ownership of Runtime: otherwise the
        // last completed task could drop Runtime from inside its own Tokio worker.
        fn context(&self) -> Self {
            Self {
                handle: self.handle.clone(),
                _owner: None,
            }
        }

        /// Number of workers in the Tokio runtime.
        pub fn current_num_threads(&self) -> usize { self.handle.metrics().num_workers() }

        /// Tokio has no stable iterator worker index; always returns None.
        pub fn current_thread_index(&self) -> Option<usize> { None }

        /// Execute a borrowed root inline and bind its nested jobs to this
        /// runtime.
        pub fn install<F: FnOnce() -> R, R>(&self, f: F) -> R {
            let _entered = Enter(CURRENT.with(|slot| slot.replace(Some(self.context()))));
            carrier_checkpoint();
            f()
        }

        /// Detached CPU work is submitted directly to Tokio. Scoped waits never
        /// claim these jobs, so a tick cannot inherit an unrelated slow job.
        pub fn spawn<F: FnOnce() + Send + 'static>(&self, f: F) {
            let pool = self.context();
            self.handle.spawn(async move {
                pool.install(f);
            });
        }

        /// Complete both borrowed branches before returning or propagating a
        /// panic.
        pub fn join<A, B, RA, RB>(&self, a: A, b: B) -> (RA, RB)
        where
            A: FnOnce() -> RA + Send,
            B: FnOnce() -> RB + Send,
            RA: Send,
            RB: Send,
        {
            self.install(|| join(a, b))
        }

        /// Run a borrowed task scope and complete all descendants before
        /// returning or unwinding.
        pub fn scope<'scope, F, R>(&self, f: F) -> R
        where
            F: FnOnce(&Scope<'scope>) -> R + Send,
            R: Send,
        {
            self.install(|| scope(f))
        }
    }
    fn current() -> ThreadPool {
        CURRENT
            .with(|slot| slot.borrow().clone())
            .unwrap_or_else(|| {
                if let Some(handle) = SHARED.lock().unwrap().clone() {
                    ThreadPool {
                        handle,
                        _owner: None,
                    }
                } else {
                    DEFAULT
                        .get_or_init(|| {
                            ThreadPoolBuilder::new()
                                .build()
                                .expect("Tokio parallel runtime")
                        })
                        .context()
                }
            })
    }
    /// Number of workers available to the current parallel operation.
    pub fn current_num_threads() -> usize { current().current_num_threads() }
    // There is no fixed iterator-worker index: Tokio owns worker assignment.
    // The backend only helps a join's own child, never a bridge's unrelated work.
    /// Tokio has no stable iterator worker index; always returns None.
    pub fn current_thread_index() -> Option<usize> { None }

    #[derive(Default)]
    /// Standalone Tokio runtime builder for desktop clients and examples.
    pub struct ThreadPoolBuilder {
        workers: Option<usize>,
        name: Option<Box<dyn FnMut(usize) -> String + Send + 'static>>,
    }
    #[derive(Debug)]
    /// Failure to create a standalone Tokio runtime.
    pub struct ThreadPoolBuildError(std::io::Error);
    impl std::fmt::Display for ThreadPoolBuildError {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { self.0.fmt(f) }
    }
    impl std::error::Error for ThreadPoolBuildError {}
    impl ThreadPoolBuilder {
        /// Construct a builder with Tokio defaults.
        pub fn new() -> Self { Self::default() }

        /// Set the number of Tokio workers, with a minimum of one.
        pub fn num_threads(mut self, n: usize) -> Self {
            self.workers = Some(n.max(1));
            self
        }

        /// Name workers in a standalone runtime.
        pub fn thread_name<F: FnMut(usize) -> String + Send + 'static>(mut self, f: F) -> Self {
            self.name = Some(Box::new(f));
            self
        }

        /// Standalone desktop/example fallback. The server uses from_runtime
        /// and therefore creates no additional worker pool here.
        pub fn build(self) -> Result<ThreadPool, ThreadPoolBuildError> {
            let mut builder = Builder::new_multi_thread();
            builder.enable_all();
            if let Some(n) = self.workers {
                builder.worker_threads(n);
            }
            if let Some(name) = self.name {
                let name = Mutex::new((name, 0));
                builder.thread_name_fn(move || {
                    let mut state = name.lock().unwrap();
                    let i = state.1;
                    state.1 += 1;
                    (state.0)(i)
                });
            }
            builder
                .build()
                .map(|runtime| ThreadPool::from_runtime(Arc::new(runtime)))
                .map_err(ThreadPoolBuildError)
        }
    }

    struct JobState {
        work: Option<Work<'static>>,
        done: bool,
        panic: Option<Panic>,
    }
    struct Job {
        state: Mutex<JobState>,
        finished: Condvar,
    }
    impl Job {
        /// SAFETY: caller must retain and wait for the returned job before 'a
        /// ends, even on panic. Waiting destroys the closure before
        /// publishing completion.
        unsafe fn borrowed<'a>(work: Work<'a>) -> Arc<Self> {
            // Only the lifetime of the closure is erased; its Send bound is retained.
            let work: Work<'static> = unsafe { std::mem::transmute(work) };
            Arc::new(Self {
                state: Mutex::new(JobState {
                    work: Some(work),
                    done: false,
                    panic: None,
                }),
                finished: Condvar::new(),
            })
        }

        fn run(&self) {
            carrier_checkpoint();
            let work = self.state.lock().unwrap().work.take();
            if let Some(work) = work {
                let result = catch_unwind(AssertUnwindSafe(work));
                let mut state = self.state.lock().unwrap();
                state.panic = result.err();
                state.done = true;
                drop(state);
                self.finished.notify_all();
            }
        }

        fn wait(&self) -> Option<Panic> {
            self.run();
            let mut state = self.state.lock().unwrap();
            while !state.done {
                state = self.finished.wait(state).unwrap();
            }
            state.panic.take()
        }
    }
    // This guard cannot be forgotten through the public API. It is created before
    // Tokio submission and covers submission failure as well as a panicking
    // sibling.
    struct Wait(Arc<Job>);
    impl Drop for Wait {
        fn drop(&mut self) { let _ = self.0.wait(); }
    }
    fn submit(pool: &ThreadPool, job: &Arc<Job>) {
        let job = job.clone();
        let executor = pool.context();
        pool.handle.spawn(async move {
            executor.install(|| job.run());
        });
    }
    #[derive(Clone, Copy)]
    /// Whether a joined branch ran on a different logical thread.
    pub struct FnContext {
        migrated: bool,
    }
    impl FnContext {
        /// Report whether execution moved away from the submitting thread.
        pub fn migrated(&self) -> bool { self.migrated }
    }
    /// Execute two borrowed branches, completing both even if either panics.
    pub fn join<A, B, RA, RB>(a: A, b: B) -> (RA, RB)
    where
        A: FnOnce() -> RA + Send,
        B: FnOnce() -> RB + Send,
        RA: Send,
        RB: Send,
    {
        join_context(|_| a(), |_| b())
    }
    /// Join two branches with migration information for iterator splitting.
    pub fn join_context<A, B, RA, RB>(a: A, b: B) -> (RA, RB)
    where
        A: FnOnce(FnContext) -> RA + Send,
        B: FnOnce(FnContext) -> RB + Send,
        RA: Send,
        RB: Send,
    {
        let pool = current();
        let output = Mutex::new(None);
        let home = std::thread::current().id();
        // SAFETY: guard waits before output, b, or any of their borrows are dropped.
        let job = unsafe {
            Job::borrowed(Box::new(|| {
                let migrated = std::thread::current().id() != home;
                *output.lock().unwrap() = Some(b(FnContext { migrated }));
            }))
        };
        let guard = Wait(job.clone());
        submit(&pool, &job);
        let left = catch_unwind(AssertUnwindSafe(|| a(FnContext { migrated: false })));
        let right_panic = job.wait();
        drop(guard);
        // Both sides complete before either panic is propagated.
        let left = left.unwrap_or_else(|panic| resume_unwind(panic));
        if let Some(panic) = right_panic {
            resume_unwind(panic);
        }
        (left, output.into_inner().unwrap().unwrap())
    }

    struct ScopeState {
        jobs: Mutex<VecDeque<Arc<Job>>>,
    }
    /// Borrowed jobs whose captures are drained before the enclosing scope
    /// returns.
    pub struct Scope<'scope> {
        state: Arc<ScopeState>,
        pool: ThreadPool,
        lifetime: PhantomData<&'scope mut &'scope ()>,
    }
    impl<'scope> Scope<'scope> {
        /// Submit a borrowed child; completion belongs to the enclosing scope.
        pub fn spawn<F>(&self, f: F)
        where
            F: FnOnce(&Scope<'scope>) + Send + 'scope,
        {
            let child = Scope {
                state: self.state.clone(),
                pool: self.pool.clone(),
                lifetime: PhantomData,
            };
            // SAFETY: scope's private guard drains all jobs, including dynamically
            // registered descendants, before any borrowed capture can expire.
            let job = unsafe { Job::borrowed(Box::new(move || f(&child))) };
            self.state.jobs.lock().unwrap().push_back(job.clone());
            submit(&self.pool, &job);
            carrier_checkpoint();
        }
    }
    struct ScopeWait(Arc<ScopeState>);
    impl ScopeWait {
        fn wait(&self) -> Option<Panic> {
            let mut panic = None;
            loop {
                let job = self.0.jobs.lock().unwrap().pop_front();
                let Some(job) = job else {
                    return panic;
                };
                if let Some(next) = job.wait() {
                    if panic.is_none() {
                        panic = Some(next);
                    }
                }
            }
        }
    }
    impl Drop for ScopeWait {
        fn drop(&mut self) { let _ = self.wait(); }
    }
    /// Run a scope and complete all descendants before returning or unwinding.
    pub fn scope<'scope, F, R>(f: F) -> R
    where
        F: FnOnce(&Scope<'scope>) -> R,
    {
        let state = Arc::new(ScopeState {
            jobs: Mutex::new(VecDeque::new()),
        });
        let guard = ScopeWait(state.clone());
        let scope = Scope {
            state,
            pool: current(),
            lifetime: PhantomData,
        };
        let result = catch_unwind(AssertUnwindSafe(|| f(&scope)));
        let panic = guard.wait();
        drop(guard);
        let result = result.unwrap_or_else(|panic| resume_unwind(panic));
        if let Some(panic) = panic {
            resume_unwind(panic);
        }
        result
    }
    /// Run a scope inline with the same borrowed completion guarantee.
    pub fn in_place_scope<'scope, F, R>(f: F) -> R
    where
        F: FnOnce(&Scope<'scope>) -> R,
    {
        scope(f)
    }
    /// Submit detached work directly to Tokio without adding it to a scope.
    pub fn spawn<F: FnOnce() + Send + 'static>(f: F) { current().spawn(f); }
}
pub use implementation::*;
