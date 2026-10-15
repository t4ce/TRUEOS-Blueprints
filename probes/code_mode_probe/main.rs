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
#[cfg(target_os = "trueos")]
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

#[derive(Default)]
struct Delegate {
    calls: Mutex<Vec<CodeModeNestedToolCall>>,
    closed: AtomicUsize,
    gri: bool,
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
            tokio::select! {
                _ = cancel.cancelled() => Err("cancelled".into()),
                _ = tokio::time::sleep(Duration::from_millis(2)) => Ok(json!({"output": call.input})),
            }
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
        output.push_str(&String::from_utf8_lossy(&buffer[..read.len]));
        if let State::Exited(code) = read.state {
            return Ok(json!({"output": output, "exit_code": code, "state": "exited"}));
        }
        if Instant::now() >= deadline {
            // A persistent Shell3/UI handoff is not a fabricated successful exit.
            return Ok(
                json!({"output": output, "exit_code": null, "state": "running_at_deadline"}),
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

async fn run(gri: bool) -> Result<(), String> {
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
            log(format_args!("gri result {text}"));
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

fn main() {
    let gri = cfg!(target_os = "trueos") || std::env::args().skip(1).any(|arg| arg == "--gri");
    #[cfg(target_os = "trueos")]
    let runtime = trueos::t::runtime::current_thread().build();
    #[cfg(not(target_os = "trueos"))]
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build();
    let runtime = match runtime {
        Ok(runtime) => runtime,
        Err(error) => {
            log(format_args!("FAIL runtime {error}"));
            return;
        }
    };
    match runtime.block_on(async { tokio::time::timeout(Duration::from_secs(60), run(gri)).await })
    {
        Ok(Ok(())) => log("ALL PASS"),
        Ok(Err(error)) => {
            log(format_args!("FAIL {error}"));
            #[cfg(not(target_os = "trueos"))]
            std::process::exit(1);
        }
        Err(_) => {
            log("FAIL 60s watchdog expired");
            #[cfg(not(target_os = "trueos"))]
            std::process::exit(1);
        }
    }
}
