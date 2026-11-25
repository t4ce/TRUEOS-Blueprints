use glicol_parser::{get_ast, nodes::Component};
use hyper::{
    Method, Request, Response, StatusCode,
    body::{Body, Bytes, Frame, Incoming, SizeHint},
    header,
    server::conn::http1,
    service::service_fn,
};
use serde_json::{Value, json};
use std::{
    convert::Infallible,
    future::poll_fn,
    io,
    net::Ipv4Addr,
    pin::Pin,
    sync::{Arc, Mutex},
    task::{Context, Poll},
    time::Duration,
};
use tokio::{
    io::{AsyncRead, AsyncWrite, ReadBuf},
    net::TcpListener,
    sync::{Semaphore, oneshot},
};

use crate::{audio, audit};

pub const ADDRESS: (Ipv4Addr, u16) = (Ipv4Addr::LOCALHOST, 42127);
const MAX_PATCH_BYTES: usize = 32 * 1024;
const VERSIONS: &[&str] = &["2025-03-26", "2025-06-18", "2025-11-25"];

pub struct PatchRequest {
    pub code: String,
    pub summary: String,
    revision: u64,
    pub reply: oneshot::Sender<Result<u64, String>>,
}

pub struct Session {
    pub draft: String,
    pub playing: Option<String>,
    pub bpm: u16,
    pending_patch: Option<PatchRequest>,
    revision: u64,
}

impl Session {
    pub fn new(draft: String) -> Self {
        Self {
            draft,
            playing: None,
            bpm: audio::DEFAULT_BPM,
            pending_patch: None,
            revision: 1,
        }
    }

    pub fn update_draft(&mut self, draft: String) -> u64 {
        if draft != self.draft {
            self.draft = draft;
            self.revision += 1;
        }
        self.revision
    }

    pub fn take_patch(&mut self) -> Option<PatchRequest> {
        let request = self.pending_patch.take()?;
        if request.revision != self.revision {
            let _ = request.reply.send(Err(stale_draft()));
            return None;
        }
        (!request.reply.is_closed()).then_some(request)
    }
}

pub type SharedSession = Arc<Mutex<Session>>;

#[derive(Clone)]
struct Endpoint {
    session: SharedSession,
    audit_slot: Arc<Semaphore>,
}

pub async fn start(session: SharedSession) -> io::Result<tokio::task::JoinHandle<io::Result<()>>> {
    #[cfg(any(target_os = "trueos", target_os = "zkvm"))]
    if std::env::var("TRUEOS_LICO_MCP_ACL").as_deref() != Ok("1") {
        return Err(io::Error::other(
            "the running kernel needs the Lico socket policy",
        ));
    }
    let listener = TcpListener::bind(ADDRESS).await?;
    let endpoint = Endpoint {
        session,
        audit_slot: Arc::new(Semaphore::new(1)),
    };
    Ok(tokio::spawn(async move {
        let permits = Arc::new(Semaphore::new(4));
        loop {
            let (stream, _) = listener.accept().await?;
            let Ok(permit) = permits.clone().try_acquire_owned() else {
                continue;
            };
            let endpoint = endpoint.clone();
            tokio::spawn(async move {
                let _permit = permit;
                let connection = http1::Builder::new().keep_alive(false).serve_connection(
                    HyperIo(stream),
                    service_fn(move |request| {
                        let endpoint = endpoint.clone();
                        async move { Ok::<_, Infallible>(handle(endpoint, request).await) }
                    }),
                );
                let _ = tokio::time::timeout(Duration::from_secs(10), connection).await;
            });
        }
    }))
}

struct ReplyBody(Option<Bytes>);

impl Body for ReplyBody {
    type Data = Bytes;
    type Error = Infallible;

    fn poll_frame(
        mut self: Pin<&mut Self>,
        _: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Bytes>, Infallible>>> {
        Poll::Ready(self.0.take().map(|bytes| Ok(Frame::data(bytes))))
    }

    fn size_hint(&self) -> SizeHint {
        SizeHint::with_exact(self.0.as_ref().map_or(0, |bytes| bytes.len() as u64))
    }
}

type Reply = Response<ReplyBody>;

fn reply(status: StatusCode) -> Reply {
    let mut response = Response::new(ReplyBody(None));
    *response.status_mut() = status;
    response
}

fn json_reply(value: Value) -> Reply {
    let mut response = Response::new(ReplyBody(Some(Bytes::from(value.to_string()))));
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        header::HeaderValue::from_static("application/json"),
    );
    response
}

async fn handle(endpoint: Endpoint, request: Request<Incoming>) -> Reply {
    if request.uri().path() != "/mcp" {
        return reply(StatusCode::NOT_FOUND);
    }
    let headers = request.headers();
    // TRUEOS authorizes local peers when connecting to this port. Reject
    // browser origins and non-loopback authorities at the HTTP boundary.
    if headers.contains_key(header::ORIGIN)
        || headers
            .get(header::HOST)
            .and_then(|value| value.to_str().ok())
            != Some("127.0.0.1:42127")
    {
        return reply(StatusCode::FORBIDDEN);
    }
    if request.method() != Method::POST {
        let mut response = reply(StatusCode::METHOD_NOT_ALLOWED);
        response
            .headers_mut()
            .insert(header::ALLOW, header::HeaderValue::from_static("POST"));
        return response;
    }
    if let Some(version) = headers.get("mcp-protocol-version")
        && !version
            .to_str()
            .is_ok_and(|version| VERSIONS.contains(&version))
    {
        return reply(StatusCode::BAD_REQUEST);
    }
    let mut stream = request.into_body();
    let mut body = Vec::new();
    while let Some(frame) = poll_fn(|context| Pin::new(&mut stream).poll_frame(context)).await {
        let Ok(frame) = frame else {
            return reply(StatusCode::BAD_REQUEST);
        };
        if let Ok(data) = frame.into_data() {
            if data.len() > 128 * 1024 - body.len() {
                return reply(StatusCode::PAYLOAD_TOO_LARGE);
            }
            body.extend_from_slice(&data);
        }
    }
    let Ok(request) = serde_json::from_slice::<Value>(&body) else {
        return json_reply(rpc_error(Value::Null, -32700, "Parse error"));
    };
    let method = request["method"].as_str();
    if request["jsonrpc"] != "2.0" || method.is_none() {
        return json_reply(rpc_error(Value::Null, -32600, "Invalid Request"));
    }
    let Some(id) = request.get("id").cloned() else {
        return reply(StatusCode::ACCEPTED);
    };
    let params = &request["params"];
    let result = match method {
        Some("initialize") => {
            let version = params["protocolVersion"]
                .as_str()
                .filter(|version| VERSIONS.contains(version))
                .unwrap_or("2025-06-18");
            json!({
                "protocolVersion": version,
                "capabilities": {"tools": {}},
                "serverInfo": {"name": "lico", "version": env!("CARGO_PKG_VERSION")},
                "instructions": "Read the current Lico draft and revision with get_patch. Before applying a musical change, use audit_patch to compare a silent render with the current patch. Inspect the changed section, using start_seconds for later breaks. Use the measured rhythm and mix differences to refine the candidate. apply_patch restarts live playback. Ctrl+U in Lico undoes the edit."
            })
        }
        Some("ping") => json!({}),
        Some("tools/list") => tool_list(),
        Some("tools/call") => {
            let result = invoke(&endpoint, params).await;
            let (value, is_error) = match result {
                Ok(value) => (value, false),
                Err(error) => (json!({"error": error}), true),
            };
            json!({"content": [{"type": "text", "text": value.to_string()}], "isError": is_error})
        }
        _ => return json_reply(rpc_error(id, -32601, "Method not found")),
    };
    json_reply(json!({"jsonrpc": "2.0", "id": id, "result": result}))
}

fn rpc_error(id: Value, code: i32, message: &str) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}})
}

fn tool_list() -> Value {
    json!({"tools": [
        {"name": "get_patch", "description": "Read the live session draft and revision, playing patch, tempo, and sample names.",
         "inputSchema": {"type": "object", "properties": {}}},
        {"name": "validate_patch", "description": "Check Glicol syntax and supported nodes without applying or playing the patch.",
         "inputSchema": {"type": "object", "properties": {"code": {"type": "string"}}, "required": ["code"]}},
        {"name": "audit_patch", "description": "Render a candidate silently and compare it with the playing patch (or draft while stopped) at the session tempo. Reports levels, clipping, frequency balance, and a beat-subdivision grid. Use start_seconds to inspect a later section, and focus_chain to measure an instrument's signal alongside the mix. Silence is reported as null dBFS.",
         "annotations": {"readOnlyHint": true, "openWorldHint": false},
         "inputSchema": {"type": "object", "properties": {
             "code": {"type": "string"},
             "start_seconds": {"type": "number", "minimum": 0, "default": 0},
             "seconds": {"type": "number", "minimum": 0.25, "maximum": 16, "default": 8},
             "focus_chain": {"type": "string", "description": "A Glicol chain name, including its leading ~ if present."}
         }, "required": ["code"]}},
        {"name": "apply_patch", "description": "Replace the draft and restart playback from the beginning. Use the revision returned by get_patch.",
         "inputSchema": {"type": "object", "properties": {
             "code": {"type": "string"}, "revision": {"type": "integer"}, "summary": {"type": "string"}
         }, "required": ["code", "revision", "summary"]}}
    ]})
}

async fn invoke(endpoint: &Endpoint, params: &Value) -> Result<Value, String> {
    let session = &endpoint.session;
    let args = &params["arguments"];
    match params["name"].as_str() {
        Some("get_patch") => {
            let session = session.lock().expect("Lico session");
            Ok(json!({
                "revision": session.revision, "draft": session.draft, "playing": session.playing,
                "bpm": session.bpm, "samples": audio::sample_names().collect::<Vec<_>>()
            }))
        }
        Some("audit_patch") => audit_patch(endpoint, args).await,
        Some("validate_patch" | "apply_patch" | "propose_patch") => {
            let code = args["code"].as_str().ok_or("code must be a string")?;
            validate(code)?;
            if params["name"] == "validate_patch" {
                return Ok(json!({"valid": true}));
            }
            let revision = args["revision"]
                .as_u64()
                .ok_or("revision must be a nonnegative integer")?;
            let summary = args["summary"].as_str().ok_or("summary must be a string")?;
            if summary.is_empty() || summary.len() > 256 || summary.chars().any(char::is_control) {
                return Err("summary must be 1–256 bytes of printable text".into());
            }
            let (reply, result) = oneshot::channel();
            {
                let mut session = session.lock().expect("Lico session");
                if session.revision != revision {
                    return Err(stale_draft());
                }
                if session.pending_patch.is_some() {
                    return Err("Lico is handling another patch".into());
                }
                session.pending_patch = Some(PatchRequest {
                    code: code.to_owned(),
                    summary: summary.to_owned(),
                    revision,
                    reply,
                });
            }
            let revision = result.await.map_err(|_| "Lico editor closed")??;
            Ok(json!({"applied": true, "revision": revision}))
        }
        _ => Err("unknown Lico tool".into()),
    }
}

async fn audit_patch(endpoint: &Endpoint, args: &Value) -> Result<Value, String> {
    let code = args["code"].as_str().ok_or("code must be a string")?;
    let seconds_arg = |key: &str, default| {
        args.get(key).map_or(Ok(default), |value| {
            value
                .as_f64()
                .ok_or_else(|| format!("{key} must be a number"))
        })
    };
    let start = seconds_arg("start_seconds", 0.0)?;
    let seconds = seconds_arg("seconds", 8.0)?;
    let focus = args
        .get("focus_chain")
        .map(|value| value.as_str().ok_or("focus_chain must be a string"))
        .transpose()?;
    let (baseline, baseline_source, bpm, revision) = {
        let session = endpoint.session.lock().expect("Lico session");
        let (baseline, source) = session
            .playing
            .as_ref()
            .map_or((&session.draft, "draft"), |code| (code, "playing"));
        (baseline.clone(), source, session.bpm, session.revision)
    };
    let window = audit::Window::new(start, seconds, bpm)?;
    validate(code)?;
    let permit = endpoint
        .audit_slot
        .clone()
        .try_acquire_owned()
        .map_err(|_| "a Lico audit is already running")?;
    let code = code.to_owned();
    let focus = focus.map(str::to_owned);
    tokio::task::spawn_blocking(move || {
        let _permit = permit;
        let mut report = audit::compare(
            &code,
            &baseline,
            baseline_source,
            bpm,
            window,
            focus.as_deref(),
        )?;
        report["revision"] = json!(revision);
        Ok(report)
    })
    .await
    .map_err(|error| format!("Lico audit worker failed: {error}"))?
}

fn stale_draft() -> String {
    "the draft changed; read get_patch again before applying an edit".into()
}

fn validate(code: &str) -> Result<(), String> {
    if code.len() > MAX_PATCH_BYTES
        || code
            .chars()
            .any(|c| c.is_control() && !matches!(c, '\t' | '\r' | '\n'))
    {
        return Err("patch must be at most 32 KiB and contain only text".into());
    }
    let ast = get_ast(code).map_err(|error| error.to_string())?;
    for component in ast.nodes.values().flatten() {
        match component {
            Component::Reverb(_) | Component::Expr(_) | Component::Adc(_) => {
                return Err("patch uses a node unavailable in Lico (reverb, expr, or adc)".into());
            }
            Component::Sp(node)
                if !audio::sample_names().any(|name| {
                    name.trim_start_matches('\\')
                        == node.sample_sym.trim_start_matches('\\').trim_matches('\'')
                }) =>
            {
                return Err(format!("sample {} is unavailable in Lico", node.sample_sym));
            }
            _ => {}
        }
    }
    Ok(())
}

struct HyperIo<T>(T);

impl<T: AsyncRead + Unpin> hyper::rt::Read for HyperIo<T> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
        mut output: hyper::rt::ReadBufCursor<'_>,
    ) -> Poll<io::Result<()>> {
        // The inner reader initializes exactly its filled portion. Advance
        // Hyper's cursor only after it reports those bytes as initialized.
        let count = unsafe {
            let mut input = ReadBuf::uninit(output.as_mut());
            match Pin::new(&mut self.0).poll_read(context, &mut input) {
                Poll::Ready(Ok(())) => input.filled().len(),
                Poll::Ready(Err(error)) => return Poll::Ready(Err(error)),
                Poll::Pending => return Poll::Pending,
            }
        };
        unsafe { output.advance(count) };
        Poll::Ready(Ok(()))
    }
}

impl<T: AsyncWrite + Unpin> hyper::rt::Write for HyperIo<T> {
    fn poll_write(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.0).poll_write(context, bytes)
    }
    fn poll_flush(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.0).poll_flush(context)
    }
    fn poll_shutdown(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.0).poll_shutdown(context)
    }
}
