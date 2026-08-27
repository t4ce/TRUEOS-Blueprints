extern crate alloc;

use alloc::{
    format,
    string::{String, ToString},
    vec::Vec,
};
use core::{
    net::SocketAddr,
    sync::atomic::{AtomicU16, Ordering},
    time::Duration,
};

use axum::{
    Router,
    body::{Body, Bytes},
    extract::State,
    http::{
        StatusCode, Uri,
        header::{CACHE_CONTROL, CONTENT_LENGTH, CONTENT_TYPE},
    },
    response::Response,
    routing::{get, post},
    serve::ListenerExt,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use strudel_core::{
    BLOCK_FRAMES, CPS_DENOMINATOR, CPS_NUMERATOR, CoreSnapshot, MAX_SOURCE_BYTES, SAMPLE_RATE_HZ,
    StrudelCore,
};
use trueos::{
    clock, logl,
    logl::level,
    platform::io,
    tokio::{
        self,
        sync::{mpsc, oneshot},
    },
};

mod monaco_assets {
    include!(concat!(env!("OUT_DIR"), "/strudel_monaco_assets.rs"));
}

const STRUDEL_HTTP_TCP_PORT: u16 = 1012;
const STRUDEL_BODY_MAX: usize = MAX_SOURCE_BYTES * 6 + 16 * 1024;
const ENGINE_CHANNEL_CAPACITY: usize = 32;
const ENGINE_COMMAND_BURST: usize = 16;
const ENGINE_POLL_INTERVAL: Duration = Duration::from_millis(4);
const ENGINE_REPLY_TIMEOUT: Duration = Duration::from_secs(5);
const INDEX_HTML: &str = include_str!("../web/index.html");
const APP_JS: &str = include_str!("../web/app.js");
const APP_CSS: &str = include_str!("../web/app.css");
const INSTRUMENT_CATALOG_JS: &str = include_str!("../js/instrument_catalog.js");

static STRUDEL_HTTP_PORT: AtomicU16 = AtomicU16::new(0);

#[derive(Clone)]
struct AppState {
    engine: mpsc::Sender<EngineCommand>,
}

enum EngineCommand {
    Snapshot {
        reply: oneshot::Sender<CoreSnapshot>,
    },
    Submit {
        source: String,
        reply: oneshot::Sender<SubmitOutcome>,
    },
}

enum SubmitOutcome {
    Committed(CoreSnapshot),
    Rejected { error: String, state: CoreSnapshot },
}

#[derive(Debug, Deserialize)]
struct SubmitRequest {
    source: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PublicState {
    revision: u64,
    source: String,
    runtime: Value,
    absolute_frame: u64,
    queued_frames: usize,
    target_queue_frames: usize,
    buffer_frames: usize,
    sample_rate_hz: u32,
    block_frames: usize,
    cps_numerator: u32,
    cps_denominator: u32,
}

impl From<CoreSnapshot> for PublicState {
    fn from(snapshot: CoreSnapshot) -> Self {
        let CoreSnapshot {
            revision,
            source,
            runtime_status_json,
            absolute_frame,
            queued_frames,
            target_queue_frames,
            buffer_frames,
            cps_numerator,
            cps_denominator,
        } = snapshot;
        let runtime = serde_json::from_str::<Value>(runtime_status_json.as_str())
            .unwrap_or_else(|_| serde_json::json!({ "raw": runtime_status_json }));
        Self {
            revision,
            source,
            runtime,
            absolute_frame,
            queued_frames,
            target_queue_frames,
            buffer_frames,
            sample_rate_hz: SAMPLE_RATE_HZ,
            block_frames: BLOCK_FRAMES,
            cps_numerator,
            cps_denominator,
        }
    }
}

pub async fn run() -> Result<(), String> {
    logl::log(
        level::INFO,
        "strudel-core-http: startup stage=core-boot-begin",
    );
    let mut core = StrudelCore::boot()?;
    logl::log(
        level::INFO,
        "strudel-core-http: startup stage=core-boot-ready",
    );
    logl::log(
        level::INFO,
        "strudel-core-http: startup stage=initial-pump-begin",
    );
    let initial_pump = core.pump()?;
    logl::log(
        level::INFO,
        "strudel-core-http: startup stage=initial-pump-ready",
    );
    if !initial_pump.diagnostics.is_empty() {
        logl::log(
            level::DEBUG,
            format_args!("strudel_core/qjs: {}", initial_pump.diagnostics),
        );
    }
    let ready = core.snapshot();
    logl::log(
        level::INFO,
        format_args!(
            "strudel_core: temporal VM + PCM stream ready sample_rate={} block_frames={} queue_target={} buffer_frames={} cps={}/{} revision={} runtime={}",
            SAMPLE_RATE_HZ,
            BLOCK_FRAMES,
            ready.target_queue_frames,
            ready.buffer_frames,
            CPS_NUMERATOR,
            CPS_DENOMINATOR,
            ready.revision,
            ready.runtime_status_json,
        ),
    );

    let (engine, commands) = mpsc::channel(ENGINE_CHANNEL_CAPACITY);
    let _engine = tokio::task::spawn_local(engine_loop(core, commands));
    logl::log(
        level::INFO,
        "strudel-core-http: startup stage=http-bind-begin",
    );
    http_runtime(AppState { engine })
        .await
        .map_err(|error| format!("strudel HTTP runtime failed: {error:?}"))
}

async fn engine_loop(mut core: StrudelCore, mut commands: mpsc::Receiver<EngineCommand>) {
    let mut pump_enabled = true;
    loop {
        for _ in 0..ENGINE_COMMAND_BURST {
            match commands.try_recv() {
                Ok(command) => {
                    if handle_engine_command(&mut core, command) {
                        pump_enabled = true;
                    }
                }
                Err(mpsc::error::TryRecvError::Empty) => break,
                Err(mpsc::error::TryRecvError::Disconnected) => return,
            }
        }

        if pump_enabled {
            match core.pump() {
                Ok(report) => {
                    if !report.diagnostics.is_empty() {
                        logl::log(
                            level::DEBUG,
                            format_args!("strudel_core/qjs: {}", report.diagnostics),
                        );
                    }
                }
                Err(error) => {
                    logl::log(
                        level::ERROR,
                        format_args!(
                            "strudel_core: audio engine paused until next valid submission: {error}"
                        ),
                    );
                    // Keep HTTP and the VM alive. A later successful commit
                    // rearms pumping, so one invalid native block can never
                    // terminate the Blueprint.
                    pump_enabled = false;
                }
            }
        }

        tokio::task::yield_now().await;
        tokio::time::sleep(ENGINE_POLL_INTERVAL).await;
    }
}

/// Returns true only when a new pattern was committed and should rearm a
/// previously paused audio pump.
fn handle_engine_command(core: &mut StrudelCore, command: EngineCommand) -> bool {
    match command {
        EngineCommand::Snapshot { reply } => {
            let _ = reply.send(core.snapshot());
            false
        }
        EngineCommand::Submit { source, reply } => {
            let (outcome, committed) = match core.commit_expression(source.as_str()) {
                Ok(report) => {
                    logl::log(
                        level::INFO,
                        format_args!(
                            "strudel_core: committed browser pattern revision={} bytes={} runtime={}",
                            report.revision,
                            source.len(),
                            report.runtime_status_json,
                        ),
                    );
                    (SubmitOutcome::Committed(core.snapshot()), true)
                }
                Err(error) => {
                    logl::log(
                        level::WARN,
                        format_args!("strudel_core: rejected browser pattern: {error}"),
                    );
                    (
                        SubmitOutcome::Rejected {
                            error,
                            state: core.snapshot(),
                        },
                        false,
                    )
                }
            };
            let _ = reply.send(outcome);
            committed
        }
    }
}

async fn http_runtime(state: AppState) -> Result<(), io::Error> {
    let app = router(state);
    let addr = SocketAddr::from(([0, 0, 0, 0], STRUDEL_HTTP_TCP_PORT));
    let listener =
        trueos::lifecycle_axum_listener!("strudel-core-http", addr, &STRUDEL_HTTP_PORT).await;
    let listener = listener.tap_io(|_| logl::log(level::INFO, "strudel-core-http: tcp accepted"));
    logl::log(
        level::INFO,
        format_args!(
            "strudel-core-http: listening port={} monaco_assets={}",
            current_port().unwrap_or(STRUDEL_HTTP_TCP_PORT),
            monaco_assets::STATIC_ASSETS.len(),
        ),
    );
    axum::serve(listener, app).await
}

fn router(state: AppState) -> Router {
    Router::new()
        .route("/", get(handle_index))
        .route("/index.html", get(handle_index))
        .route("/app.js", get(handle_app_js))
        .route("/app.css", get(handle_app_css))
        .route("/instrument-catalog.js", get(handle_instrument_catalog_js))
        .route("/monaco/vs/{*asset}", get(handle_monaco_asset))
        .route("/healthz", get(handle_health))
        .route("/api/healthz", get(handle_health))
        .route("/api/strudel/state", get(handle_state))
        .route("/api/strudel/submit", post(handle_submit))
        .with_state(state)
}

async fn handle_index() -> Response {
    text_response(200, "text/html; charset=utf-8", INDEX_HTML)
}

async fn handle_app_js() -> Response {
    text_response(200, "application/javascript; charset=utf-8", APP_JS)
}

async fn handle_app_css() -> Response {
    text_response(200, "text/css; charset=utf-8", APP_CSS)
}

async fn handle_instrument_catalog_js() -> Response {
    text_response(
        200,
        "application/javascript; charset=utf-8",
        INSTRUMENT_CATALOG_JS,
    )
}

async fn handle_monaco_asset(uri: Uri) -> Response {
    let Some(path) = uri.path().strip_prefix("/monaco/vs/") else {
        return error_response(404, "asset not found");
    };
    match monaco_asset(path) {
        Some(asset) => bytes_response(200, asset.mime, asset.bytes, true),
        None => error_response(404, "asset not found"),
    }
}

async fn handle_health(State(state): State<AppState>) -> Response {
    match request_snapshot(&state).await {
        Ok(snapshot) => json_response(
            200,
            &serde_json::json!({
                "ok": true,
                "service": "strudel-core-http",
                "port": current_port(),
                "assetCount": monaco_assets::STATIC_ASSETS.len(),
                "generatedAtS": clock::ntp_current_unix_seconds(),
                "state": PublicState::from(snapshot),
            }),
        ),
        Err(error) => error_response(503, error),
    }
}

async fn handle_state(State(state): State<AppState>) -> Response {
    match request_snapshot(&state).await {
        Ok(snapshot) => json_response(
            200,
            &serde_json::json!({
                "ok": true,
                "state": PublicState::from(snapshot),
            }),
        ),
        Err(error) => error_response(503, error),
    }
}

async fn handle_submit(State(state): State<AppState>, body: Bytes) -> Response {
    if body.len() > STRUDEL_BODY_MAX {
        return error_response(413, "request too large");
    }
    let request = match serde_json::from_slice::<SubmitRequest>(body.as_ref()) {
        Ok(request) => request,
        Err(error) => return error_response(400, format!("bad json: {error}")),
    };
    if request.source.len() > MAX_SOURCE_BYTES {
        return error_response(413, "pattern source is too large");
    }

    match request_submit(&state, request.source).await {
        Ok(SubmitOutcome::Committed(snapshot)) => json_response(
            200,
            &serde_json::json!({
                "ok": true,
                "state": PublicState::from(snapshot),
            }),
        ),
        Ok(SubmitOutcome::Rejected { error, state }) => json_response(
            StatusCode::UNPROCESSABLE_ENTITY.as_u16(),
            &serde_json::json!({
                "ok": false,
                "error": error,
                "state": PublicState::from(state),
            }),
        ),
        Err(error) => error_response(503, error),
    }
}

async fn request_snapshot(state: &AppState) -> Result<CoreSnapshot, String> {
    let (reply, result) = oneshot::channel();
    state
        .engine
        .send(EngineCommand::Snapshot { reply })
        .await
        .map_err(|_| "audio engine is unavailable".to_string())?;
    match tokio::time::timeout(ENGINE_REPLY_TIMEOUT, result).await {
        Ok(Ok(snapshot)) => Ok(snapshot),
        Ok(Err(_)) => Err("audio engine dropped the state reply".to_string()),
        Err(_) => Err("audio engine state request timed out".to_string()),
    }
}

async fn request_submit(state: &AppState, source: String) -> Result<SubmitOutcome, String> {
    let (reply, result) = oneshot::channel();
    state
        .engine
        .send(EngineCommand::Submit { source, reply })
        .await
        .map_err(|_| "audio engine is unavailable".to_string())?;
    match tokio::time::timeout(ENGINE_REPLY_TIMEOUT, result).await {
        Ok(Ok(outcome)) => Ok(outcome),
        Ok(Err(_)) => Err("audio engine dropped the submit reply".to_string()),
        Err(_) => Err("audio engine submit request timed out".to_string()),
    }
}

fn current_port() -> Option<u16> {
    match STRUDEL_HTTP_PORT.load(Ordering::Acquire) {
        0 => None,
        port => Some(port),
    }
}

fn monaco_asset(path: &str) -> Option<&'static monaco_assets::StaticAsset> {
    monaco_assets::STATIC_ASSETS
        .iter()
        .find(|asset| asset.path == path)
}

fn status_code(status: u16) -> StatusCode {
    StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR)
}

fn response(status: u16, content_type: &'static str, body: Vec<u8>, no_store: bool) -> Response {
    Response::builder()
        .status(status_code(status))
        .header(CONTENT_TYPE, content_type)
        .header(CONTENT_LENGTH, body.len().to_string())
        .header(
            CACHE_CONTROL,
            if no_store {
                "no-store"
            } else {
                "no-cache, max-age=0"
            },
        )
        .body(Body::from(body))
        .unwrap_or_else(|_| Response::new(Body::empty()))
}

fn text_response(status: u16, content_type: &'static str, body: &str) -> Response {
    response(status, content_type, body.as_bytes().to_vec(), false)
}

fn bytes_response(
    status: u16,
    content_type: &'static str,
    body: &'static [u8],
    cache: bool,
) -> Response {
    Response::builder()
        .status(status_code(status))
        .header(CONTENT_TYPE, content_type)
        .header(CONTENT_LENGTH, body.len().to_string())
        .header(
            CACHE_CONTROL,
            if cache {
                "public, max-age=3600"
            } else {
                "no-cache, max-age=0"
            },
        )
        .body(Body::from(body))
        .unwrap_or_else(|_| Response::new(Body::empty()))
}

fn json_response<T: Serialize>(status: u16, value: &T) -> Response {
    match serde_json::to_vec(value) {
        Ok(body) => response(status, "application/json; charset=utf-8", body, true),
        Err(_) => text_response(
            500,
            "text/plain; charset=utf-8",
            "json serialization failed\n",
        ),
    }
}

fn error_response(status: u16, error: impl ToString) -> Response {
    json_response(
        status,
        &serde_json::json!({
            "ok": false,
            "error": error.to_string(),
        }),
    )
}
