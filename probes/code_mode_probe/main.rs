//! Native QuickJS -> Code Mode protocol -> Shell3 vertical-slice probe.
//! Native execution deliberately opens Gridpaper through the configured gri alias.
//! Host execution uses controlled delegates; no model or remote JS host is used.
use codex_code_mode_protocol::*;
use codex_code_mode_quickjs::QuickJsCodeModeSessionProvider;
use codex_protocol::ToolName;
use serde_json::{Value, json};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};
use std::time::Duration;
use std::time::Instant;
use tokio_util::sync::CancellationToken;

fn log(message: impl std::fmt::Display) {
    #[cfg(target_os = "trueos")]
    trueos::logl::log(
        trueos::logl::level::INFO,
        format_args!("code_mode_probe: {message}"),
    );
    #[cfg(not(target_os = "trueos"))]
    println!("code_mode_probe: {message}");
}

fn platform_nanos() -> u64 {
    #[cfg(target_os = "trueos")]
    {
        unsafe extern "Rust" {
            fn trueos_platform_monotonic_nanos() -> u64;
        }
        unsafe { trueos_platform_monotonic_nanos() }
    }
    #[cfg(not(target_os = "trueos"))]
    {
        static EPOCH: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
        EPOCH
            .get_or_init(Instant::now)
            .elapsed()
            .as_nanos()
            .min(u64::MAX as u128) as u64
    }
}

#[cfg(target_os = "trueos")]
fn native_clock_preflight() -> Result<(), String> {
    #[repr(C)]
    struct Timespec {
        seconds: i64,
        nanos: i64,
    }
    unsafe extern "C" {
        fn clock_gettime(clock: i32, out: *mut Timespec) -> i32;
    }
    let mut clock = Timespec {
        seconds: 0,
        nanos: 0,
    };
    let before = platform_nanos();
    let status = unsafe { clock_gettime(1, &mut clock) };
    let after = platform_nanos();
    if status != 0 || clock.seconds < 0 || !(0..1_000_000_000).contains(&clock.nanos) {
        return Err("C monotonic clock returned invalid timespec".into());
    }
    let nanos = (clock.seconds as u128 * 1_000_000_000 + clock.nanos as u128)
        .try_into()
        .map_err(|_| "C monotonic clock overflow")?;
    log(format_args!(
        "clock preflight platform_before_ns={before} c_monotonic_ns={nanos} platform_after_ns={after}"
    ));
    if !(before <= nanos && nanos <= after) {
        return Err("C and platform monotonic clocks disagree".into());
    }
    log("PASS C/platform monotonic clock bracket");
    Ok(())
}

fn condvar_preflight() -> Result<(), String> {
    let pair = Arc::new((Mutex::new(true), std::sync::Condvar::new()));
    let worker_pair = pair.clone();
    let (tx, rx) = std::sync::mpsc::channel();
    let before = platform_nanos();
    let _worker = std::thread::Builder::new()
        .name("condvar-preflight".into())
        .spawn(move || {
            let before = platform_nanos();
            let result = worker_pair
                .0
                .lock()
                .map_err(|_| "condvar mutex poisoned")
                .and_then(|guard| {
                    worker_pair
                        .1
                        .wait_timeout_while(guard, Duration::from_millis(2), |pending| *pending)
                        .map_err(|_| "condvar wait poisoned")
                })
                .map(|(_, result)| (result.timed_out(), platform_nanos().saturating_sub(before)));
            let _ = tx.send(result);
        })
        .map_err(|error| format!("condvar worker spawn failed: {error}"))?;
    // This bound uses the independent platform clock and nonblocking channel
    // reads, not Tokio timers or another condition-variable timed wait.
    loop {
        match rx.try_recv() {
            Ok(result) => {
                let (timed_out, elapsed_ns) = result.map_err(String::from)?;
                if !timed_out || !(2_000_000..=1_000_000_000).contains(&elapsed_ns) {
                    return Err(format!(
                        "condvar2ms contract timed_out={timed_out} elapsed_ns={elapsed_ns}"
                    ));
                }
                log(format_args!(
                    "PASS stdCondvar2ms timed_out={timed_out} elapsed_ns={elapsed_ns}"
                ));
                return Ok(());
            }
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                return Err("condvar worker ended without result".into());
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {}
        }
        if platform_nanos().saturating_sub(before) >= 1_000_000_000 {
            if let Ok(mut pending) = pair.0.try_lock() {
                *pending = false;
            }
            pair.1.notify_all();
            // Request cancellation without introducing an unbounded join on a
            // broken wait contract. The native VM also retires its threads on exit.
            return Err("stdCondvar2ms exceeded independent1s bound; cancellation notified".into());
        }
        std::thread::yield_now();
    }
}

#[derive(Default)]
struct Delegate {
    calls: Mutex<Vec<CodeModeNestedToolCall>>,
    closed: AtomicUsize,
    timer_returned: AtomicUsize,
    gri: bool,
}

#[derive(Default)]
struct Progress {
    repeated: Arc<Delegate>,
    quiet_completed: AtomicUsize,
    direct_completed: AtomicUsize,
    pure: Arc<PureProgress>,
    cancel: CancellationToken,
}

#[derive(Default)]
struct PureProgress {
    completed: AtomicUsize,
    calls: AtomicUsize,
    timer_returned: AtomicUsize,
    closed: AtomicUsize,
    active: AtomicUsize,
}

struct ActivePureWorker(Arc<PureProgress>);
impl Drop for ActivePureWorker {
    fn drop(&mut self) {
        self.0.active.fetch_sub(1, Ordering::Release);
    }
}

async fn direct_timer_control(progress: &Progress) {
    log("BEGIN direct Tokio timers count=64");
    let began = Instant::now();
    for index in 0..64 {
        tokio::time::sleep(Duration::from_millis(2)).await;
        progress
            .direct_completed
            .store(index + 1, Ordering::Relaxed);
    }
    log(format_args!(
        "PASS direct Tokio timers count=64 elapsed_ms={}",
        began.elapsed().as_millis()
    ));
}

async fn pure_timer_control(progress: Arc<Progress>) -> Result<(), String> {
    log("BEGIN no-JS timer bridge count=64");
    let began = Instant::now();
    let runtime = tokio::runtime::Handle::current();
    for index in 0..64 {
        let (completed_tx, completed_rx) = tokio::sync::oneshot::channel();
        let runtime = runtime.clone();
        let cancel = progress.cancel.clone();
        let counters = progress.pure.clone();
        counters.active.fetch_add(1, Ordering::Relaxed);
        let spawn = std::thread::Builder::new()
            .name("no-js-bridge".into())
            .spawn(move || {
                let _active = ActivePureWorker(counters.clone());
                let (tx, rx) = std::sync::mpsc::channel();
                let mut tasks = Vec::new();
                let result = (|| {
                    for _ in 0..2 {
                        let tx = tx.clone();
                        let task_cancel = cancel.clone();
                        let counters = counters.clone();
                        tasks.push(runtime.spawn(async move {
                            counters.calls.fetch_add(1, Ordering::Relaxed);
                            tokio::select! {
                                _ = task_cancel.cancelled() => {},
                                _ = tokio::time::sleep(Duration::from_millis(2)) => {
                                    counters.timer_returned.fetch_add(1, Ordering::Relaxed);
                                    let _ = tx.send(());
                                }
                            }
                        }));
                        loop {
                            if cancel.is_cancelled() {
                                return Err("no-JS bridge cancelled".to_string());
                            }
                            std::thread::yield_now();
                            match rx.recv_timeout(Duration::from_millis(1)) {
                                Ok(()) => break,
                                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                                Err(_) => return Err("no-JS bridge channel closed".into()),
                            }
                        }
                    }
                    Ok(())
                })();
                for task in &tasks {
                    task.abort();
                }
                runtime.block_on(async {
                    for task in tasks {
                        let _ = task.await;
                    }
                });
                counters.closed.fetch_add(1, Ordering::Relaxed);
                let _ = completed_tx.send(result);
            });
        if let Err(error) = spawn {
            progress.pure.active.fetch_sub(1, Ordering::Release);
            return Err(format!("no-JS worker spawn failed: {error}"));
        }
        completed_rx
            .await
            .map_err(|_| "no-JS worker ended unexpectedly")??;
        progress.pure.completed.store(index + 1, Ordering::Relaxed);
    }
    if progress.pure.calls.load(Ordering::Relaxed) != 128
        || progress.pure.timer_returned.load(Ordering::Relaxed) != 128
        || progress.pure.closed.load(Ordering::Relaxed) != 64
    {
        return Err("no-JS bridge counter mismatch".into());
    }
    log(format_args!(
        "PASS no-JS timer bridge count=64 calls=128 timer_returned=128 closed=64 elapsed_ms={}",
        began.elapsed().as_millis()
    ));
    Ok(())
}

impl CodeModeSessionDelegate for Delegate {
    fn invoke_tool<'a>(
        &'a self,
        call: CodeModeNestedToolCall,
        cancel: CancellationToken,
    ) -> ToolInvocationFuture<'a> {
        Box::pin(async move {
            if call.tool_name != ToolName::new(None, "exec_command")
                || call.tool_kind != CodeModeToolKind::Function
            {
                return Err("unexpected authoritative tool identity".into());
            }
            self.calls.lock().unwrap().push(call.clone());
            if self.gri {
                let script = call
                    .input
                    .as_ref()
                    .and_then(|v| v.get("cmd"))
                    .and_then(Value::as_str)
                    .ok_or("missing native command script")?;
                if script != "gri" {
                    return Err("this probe only authorizes the gri alias".into());
                }
                return native_command(script.to_owned(), cancel).await;
            }
            let result = tokio::select! {
                _ = cancel.cancelled() => Err("cancelled".into()),
                _ = tokio::time::sleep(Duration::from_millis(2)) => {
                    self.timer_returned.fetch_add(1, Ordering::Relaxed);
                    Ok(json!({"output": call.input}))
                },
            };
            result
        })
    }
    fn notify<'a>(
        &'a self,
        _: String,
        _: CellId,
        _: String,
        _: CancellationToken,
    ) -> NotificationFuture<'a> {
        Box::pin(async { Ok(()) })
    }
    fn cell_closed(&self, _: &CellId) {
        self.closed.fetch_add(1, Ordering::SeqCst);
    }
}

#[cfg(not(target_os = "trueos"))]
async fn native_command(_: String, _: CancellationToken) -> Result<Value, String> {
    Err("--gri requires the TRUEOS KernelBackend; host launch refused".into())
}

#[cfg(target_os = "trueos")]
async fn native_command(script: String, cancel: CancellationToken) -> Result<Value, String> {
    use trueos::command_session::{Geometry, KernelBackend, Launch, Session, State};
    let mut session = Session::open(
        &KernelBackend,
        Launch {
            script,
            stdin_open: false,
            terminal: Some(Geometry {
                rows: 24,
                cols: 100,
            }),
        },
    )
    .map_err(|e| e.to_string())?;
    log("Shell3 gri admitted; observing captured transport for at most 30s");
    let deadline = Instant::now() + Duration::from_secs(30);
    let mut output = String::new();
    let mut captured_bytes = 0usize;
    let mut buffer = [0u8; 4096];
    loop {
        if cancel.is_cancelled() {
            return Err("native command cancelled".into());
        }
        let read = session
            .transport()
            .read(&mut buffer)
            .map_err(|e| e.to_string())?;
        if output.len() + read.len > 64 * 1024 {
            return Err("native command output limit exceeded".into());
        }
        captured_bytes += read.len;
        output.push_str(&String::from_utf8_lossy(&buffer[..read.len]));
        if let State::Exited(code) = read.state {
            return Ok(
                json!({"output": output, "exit_code": code, "state": "exited", "captured_bytes": captured_bytes}),
            );
        }
        if Instant::now() >= deadline {
            // A persistent Shell3/UI handoff is not a fabricated successful exit.
            return Ok(
                json!({"output": output, "exit_code": null, "state": "running_at_deadline", "captured_bytes": captured_bytes}),
            );
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
}

fn request(source: &str, tools: bool) -> ExecuteRequest {
    ExecuteRequest {
        tool_call_id: "code-mode-probe".into(),
        source: source.into(),
        enabled_tools: if tools {
            vec![ToolDefinition {
                name: "exec_command".into(),
                tool_name: ToolName::new(None, "exec_command"),
                kind: CodeModeToolKind::Function,
                description: "probe-controlled native command session".into(),
                input_schema: None,
                input_schema_max_bytes: None,
                output_schema: None,
            }]
        } else {
            vec![]
        },
        yield_time_ms: Some(100),
        max_output_tokens: Some(20_000),
    }
}

async fn execute(
    session: &Arc<dyn CodeModeSession>,
    request: ExecuteRequest,
    delegate: Arc<Delegate>,
) -> Result<Vec<String>, String> {
    let started = session.execute(request, delegate, None).await?;
    let mut response = started.initial_response().await?;
    let mut text = Vec::new();
    loop {
        let (items, next) = match response {
            RuntimeResponse::Result {
                content_items,
                error_text,
                ..
            } => {
                if let Some(error) = error_text {
                    return Err(error);
                }
                (content_items, None)
            }
            RuntimeResponse::Yielded {
                cell_id,
                content_items,
                ..
            } => (content_items, Some(cell_id)),
            RuntimeResponse::Terminated { .. } => return Err("unexpected terminated cell".into()),
        };
        for item in items {
            match item {
                FunctionCallOutputContentItem::InputText { text: value } => text.push(value),
                _ => return Err("unexpected nontext output".into()),
            }
        }
        match next {
            None => return Ok(text),
            Some(cell_id) => {
                response = session
                    .wait(
                        WaitRequest {
                            cell_id,
                            yield_time_ms: 100,
                        },
                        None,
                    )
                    .await?
                    .into()
            }
        }
    }
}

async fn run(gri: bool, progress: Arc<Progress>) -> Result<(), String> {
    let session = QuickJsCodeModeSessionProvider.create_session().await?;
    let delegate = Arc::new(Delegate::default());
    if execute(
        &session,
        request(
            "const answer = await Promise.resolve(6*7); text(answer)",
            false,
        ),
        delegate.clone(),
    )
    .await?
        != ["42"]
    {
        return Err("ES module/await result mismatch".into());
    }
    log("PASS real ES module 42");
    let output = execute(&session, request("const a = await tools.exec_command({n:6});const b = await tools.exec_command({n:a.output.n*7});text(b.output.n)", true), delegate.clone()).await?;
    if output != ["42"] || delegate.calls.lock().unwrap().len() != 2 {
        return Err("nested delegate mismatch".into());
    }
    log("PASS async nested tools with authoritative ToolName and computed arguments");
    execute(
        &session,
        request("store('answer',{n:42})", false),
        delegate.clone(),
    )
    .await?;
    if execute(
        &session,
        request("text(load('answer').n)", false),
        delegate.clone(),
    )
    .await?
        != ["42"]
    {
        return Err("session persistence mismatch".into());
    }
    log("PASS session store/load");
    let mut looping = request("while(true){}", false);
    looping.yield_time_ms = Some(5);
    let started = session.execute(looping, delegate.clone(), None).await?;
    let id = started.cell_id.clone();
    if !matches!(
        started.initial_response().await?,
        RuntimeResponse::Yielded { .. }
    ) {
        return Err("infinite loop failed to yield observation".into());
    }
    if !matches!(
        session.terminate(id).await?,
        WaitOutcome::LiveCell(RuntimeResponse::Terminated { .. })
    ) {
        return Err("infinite loop termination mismatch".into());
    }
    if delegate.closed.load(Ordering::SeqCst) != 5 {
        return Err("cell cleanup callback mismatch".into());
    }
    log("PASS infinite-loop cancellation and cleanup");
    // No per-cell logging: this control deliberately removes diagnostic I/O
    // perturbation from repeated worker creation and asynchronous host handoffs.
    let repeated = progress.repeated.clone();
    let began = Instant::now();
    let mut max_cell_ms = 0;
    log("BEGIN quiet repeated asynchronous cells count=64");
    for cell_index in 0..64 {
        let before = Instant::now();
        let output = execute(&session, request(
            "const a=await tools.exec_command({n:6});const b=await tools.exec_command({n:a.output.n*7});text(b.output.n)",
            true,
        ), repeated.clone()).await?;
        if output != ["42"] {
            return Err("repeated asynchronous cell result mismatch".into());
        }
        progress
            .quiet_completed
            .store(cell_index + 1, Ordering::Relaxed);
        max_cell_ms = max_cell_ms.max(before.elapsed().as_millis());
    }
    let calls = repeated.calls.lock().unwrap().len();
    let closed = repeated.closed.load(Ordering::SeqCst);
    if calls != 128 || closed != 64 {
        return Err("repeated asynchronous cell cleanup/count mismatch".into());
    }
    log(format_args!(
        "PASS quiet repeated asynchronous cells count=64 calls={calls} closed={closed} elapsed_ms={} max_cell_ms={max_cell_ms}",
        began.elapsed().as_millis()
    ));
    if gri {
        let native = Arc::new(Delegate {
            gri: true,
            ..Delegate::default()
        });
        let output = execute(
            &session,
            request(
                "const result=await tools.exec_command({cmd:'gri'});text(result)",
                true,
            ),
            native.clone(),
        )
        .await?;
        if native.calls.lock().unwrap().len() != 1 {
            return Err("gri delegate count mismatch".into());
        }
        for text in output {
            let result: Value =
                serde_json::from_str(&text).map_err(|_| "gri result is not JSON")?;
            let state = result
                .get("state")
                .and_then(Value::as_str)
                .ok_or("gri result has no state")?;
            let exit_code = result
                .get("exit_code")
                .ok_or("gri result has no exit code")?;
            let bytes = result
                .get("captured_bytes")
                .and_then(Value::as_u64)
                .ok_or("gri result has no captured byte count")?;
            log(format_args!(
                "gri result state={state} exit_code={exit_code} captured_bytes={bytes}"
            ));
        }
        log(
            "PASS real JavaScript delegated gri to native transport; UI4 visibility requires observation",
        );
    } else {
        log("SKIP native Shell3 launch (pass --gri to authorize)");
    }
    session.shutdown().await?;
    log("PASS shutdown");
    Ok(())
}

async fn cancel_pure_workers(progress: &Progress) -> bool {
    progress.cancel.cancel();
    let began = Instant::now();
    while progress.pure.active.load(Ordering::Acquire) != 0 {
        if began.elapsed() >= Duration::from_secs(2) {
            return false;
        }
        // Cleanup must continue driving tasks even when timed parking is the
        // contract under investigation; do not depend on another timer here.
        std::thread::yield_now();
        tokio::task::yield_now().await;
    }
    true
}

fn run_profile(workers: usize, gri: bool) -> bool {
    let multi_thread = workers != 0;
    let profile = match workers {
        1 => "multi_thread_1",
        4 => "multi_thread_4",
        _ => "current_thread",
    };
    log(format_args!("BEGIN runtime profile={profile}"));
    let runtime = if multi_thread {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(workers)
            .enable_all()
            .build()
    } else {
        #[cfg(target_os = "trueos")]
        {
            trueos::t::runtime::current_thread().build()
        }
        #[cfg(not(target_os = "trueos"))]
        {
            tokio::runtime::Builder::new_current_thread()
                .enable_time()
                .build()
        }
    };
    let runtime = match runtime {
        Ok(runtime) => runtime,
        Err(error) => {
            log(format_args!("FAIL runtime profile={profile} {error}"));
            return false;
        }
    };
    // Retain progress outside the timed future: cancellation must not discard
    // the only evidence of where a quiet native run stopped making progress.
    let progress = Arc::new(Progress::default());
    let passed = match runtime.block_on(async {
        tokio::time::timeout(Duration::from_secs(60), async {
            if multi_thread {
                run(gri, progress.clone()).await?;
                direct_timer_control(&progress).await;
                pure_timer_control(progress.clone()).await
            } else {
                direct_timer_control(&progress).await;
                pure_timer_control(progress.clone()).await?;
                run(false, progress.clone()).await
            }
        })
        .await
    }) {
        Ok(Ok(())) => {
            log(format_args!("PASS runtime profile={profile}"));
            true
        }
        Ok(Err(error)) => {
            log(format_args!("FAIL profile={profile} {error}"));
            false
        }
        Err(_) => {
            log(format_args!(
                "FAIL 60s watchdog expired profile={profile} quiet_completed={} calls={} timer_returned={} closed={} direct_completed={} pure_completed={} pure_calls={} pure_timer_returned={} pure_closed={}",
                progress.quiet_completed.load(Ordering::Relaxed),
                progress.repeated.calls.lock().unwrap().len(),
                progress.repeated.timer_returned.load(Ordering::Relaxed),
                progress.repeated.closed.load(Ordering::Relaxed),
                progress.direct_completed.load(Ordering::Relaxed),
                progress.pure.completed.load(Ordering::Relaxed),
                progress.pure.calls.load(Ordering::Relaxed),
                progress.pure.timer_returned.load(Ordering::Relaxed),
                progress.pure.closed.load(Ordering::Relaxed),
            ));
            false
        }
    };
    let cleanup_ok = runtime.block_on(cancel_pure_workers(&progress));
    if !cleanup_ok {
        log(format_args!(
            "FAIL no-JS cancellation cleanup profile={profile}"
        ));
    }
    runtime.shutdown_timeout(Duration::from_secs(2));
    passed && cleanup_ok
}

fn main() {
    #[cfg(target_os = "trueos")]
    if let Err(error) = native_clock_preflight() {
        log(format_args!("FAIL clock preflight {error}"));
        return;
    }
    if let Err(error) = condvar_preflight() {
        log(format_args!("FAIL condvar preflight {error}"));
        #[cfg(not(target_os = "trueos"))]
        std::process::exit(1);
        #[cfg(target_os = "trueos")]
        return;
    }
    let gri = cfg!(target_os = "trueos") || std::env::args().skip(1).any(|arg| arg == "--gri");
    // Production uses a multithreaded Tokio runtime with all drivers enabled.
    // Exercise it before the separate current-thread diagnostic control.
    if !run_profile(1, gri) || !run_profile(4, false) || !run_profile(0, false) {
        #[cfg(not(target_os = "trueos"))]
        std::process::exit(1);
        return;
    }
    log("ALL PASS all runtime profiles");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standard_condvar_preflight_observes_a_real_timeout() {
        condvar_preflight().unwrap();
    }

    #[tokio::test]
    async fn cancelling_pending_no_js_bridge_joins_tasks_and_releases_worker() {
        let progress = Arc::new(Progress::default());
        let task = tokio::spawn(pure_timer_control(progress.clone()));
        tokio::time::timeout(Duration::from_secs(1), async {
            while progress.pure.calls.load(Ordering::Relaxed) == 0 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("delegate did not enter");
        progress.cancel.cancel();
        let result = tokio::time::timeout(Duration::from_secs(1), task)
            .await
            .expect("cancelled control timed out")
            .expect("control panicked");
        assert!(result.is_err());
        assert!(cancel_pure_workers(&progress).await);
        assert_eq!(progress.pure.active.load(Ordering::Acquire), 0);
        assert!(progress.pure.closed.load(Ordering::Relaxed) >= 1);
        assert!(
            progress.pure.timer_returned.load(Ordering::Relaxed)
                <= progress.pure.calls.load(Ordering::Relaxed)
        );
    }
}
