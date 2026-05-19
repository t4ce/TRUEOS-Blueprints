// trueos-blueprint: features=["tokio-net-probe"]

extern crate alloc;

use alloc::{
    collections::BTreeMap,
    format,
    string::{String, ToString},
    sync::Arc,
    vec,
    vec::Vec,
};
use core::sync::atomic::{AtomicU16, AtomicU64, Ordering};

use axum::{
    Router,
    body::{Body, Bytes},
    extract::{DefaultBodyLimit, Path, State},
    http::{
        HeaderMap, StatusCode, Uri,
        header::{CACHE_CONTROL, CONTENT_DISPOSITION, CONTENT_LENGTH, CONTENT_TYPE},
    },
    response::Response,
    routing::{get, patch, post},
    serve::ListenerExt,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use trueos::{
    clock, logl,
    logl::level,
    platform::{self, io},
    runtime,
    time::{self, Duration},
    tokio::{self, net::SocketAddr},
    vfs,
};

const FILEEXPLORER_HTTP_TCP_PORT: u16 = 8;
const FILEEXPLORER_ALT_TCP_PORT: u16 = 6;
const FILEEXPLORER_HTTP_BODY_MAX: usize = 64 * 1024;
const FILEEXPLORER_UPLOAD_BODY_MAX: usize = 16 * 1024 * 1024;
const FILEEXPLORER_BIND_RETRY_MS: u64 = 1000;
const FILEEXPLORER_INDEX_HTML: &str = include_str!("index.html");
const TRUEOS_TAILWIND_CSS: &str = include_str!("tailwind.css");

static FILEEXPLORER_HTTP_PORT: AtomicU16 = AtomicU16::new(0);
static JOB_SEQ: AtomicU64 = AtomicU64::new(1);

type JobMap = Arc<tokio::sync::RwLock<BTreeMap<String, JobRecord>>>;

#[derive(Clone)]
struct AppState {
    jobs: JobMap,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct TreeSnapshot {
    schema: &'static str,
    version: u64,
    root: FileNode,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct FileNode {
    id: String,
    name: String,
    kind: NodeKind,
    size: u64,
    modified: String,
    meta: BTreeMap<String, Value>,
    actions: Vec<&'static str>,
    children: Vec<FileNode>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum NodeKind {
    File,
    Folder,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct JobRecord {
    id: String,
    operation: String,
    status: &'static str,
    progress: u8,
    description: String,
    affected_node_ids: Vec<String>,
    created_at_ms: u64,
    updated_at_ms: u64,
    result: Option<Value>,
    error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct AcceptedJob {
    job_id: String,
    label: String,
    status_url: String,
    events_url: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateNodeRequest {
    parent_id: String,
    name: String,
    kind: NodeKind,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct UpdateNodeRequest {
    #[serde(default)]
    name: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DeleteNodesRequest {
    ids: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MoveNodesRequest {
    #[serde(default)]
    ids: Vec<String>,
}

fn now_ms() -> u64 {
    clock::ntp_current_unix_seconds().saturating_mul(1000)
}

fn now_iso() -> String {
    format!("unix-ms-{}", now_ms())
}

fn status_code(status: u16) -> StatusCode {
    StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR)
}

fn response(status: u16, content_type: &'static str, body: Vec<u8>) -> Response {
    Response::builder()
        .status(status_code(status))
        .header(CONTENT_TYPE, content_type)
        .header(CONTENT_LENGTH, body.len().to_string())
        .header(
            CACHE_CONTROL,
            if status == 200 {
                "no-cache"
            } else {
                "no-store"
            },
        )
        .body(Body::from(body))
        .unwrap_or_else(|_| Response::new(Body::empty()))
}

fn text_response(status: u16, content_type: &'static str, body: &str) -> Response {
    response(status, content_type, body.as_bytes().to_vec())
}

fn json_response<T: Serialize>(status: u16, value: &T) -> Response {
    match serde_json::to_vec(value) {
        Ok(body) => response(status, "application/json; charset=utf-8", body),
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

fn index_html() -> String {
    FILEEXPLORER_INDEX_HTML.replace(
        "<title>Async File Explorer</title>",
        "<title>TRUEOSFS File Explorer</title>\n  <script>window.FILE_EXPLORER_API_BASE = \"/api\";</script>",
    )
}

fn decode_node_id(id: &str) -> Result<String, String> {
    let id = id.trim();
    if id.is_empty() || id == "root" {
        return Ok(String::new());
    }
    percent_decode(id).map(|path| path.trim_matches('/').to_string())
}

fn percent_decode(input: &str) -> Result<String, String> {
    let bytes = input.as_bytes();
    let mut out = Vec::new();
    let mut index = 0usize;
    while index < bytes.len() {
        match bytes[index] {
            b'%' if index + 2 < bytes.len() => {
                let hi = hex(bytes[index + 1]).ok_or_else(|| "bad escape".to_string())?;
                let lo = hex(bytes[index + 2]).ok_or_else(|| "bad escape".to_string())?;
                out.push((hi << 4) | lo);
                index += 3;
            }
            b'%' => return Err("bad escape".to_string()),
            b'+' => {
                out.push(b' ');
                index += 1;
            }
            byte => {
                out.push(byte);
                index += 1;
            }
        }
    }
    String::from_utf8(out).map_err(|_| "bad utf8".to_string())
}

fn hex(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn node_for_path(path: &str) -> FileNode {
    let name = if path.is_empty() {
        "TRUEOSFS".to_string()
    } else {
        path.rsplit('/').next().unwrap_or(path).to_string()
    };
    let stat = if path.is_empty() {
        None
    } else {
        vfs::stat(path.as_bytes()).ok()
    };
    let kind = match stat.map(|stat| stat.kind) {
        Some(vfs::FsNodeKind::File) => NodeKind::File,
        _ => NodeKind::Folder,
    };
    let size = stat.map(|stat| stat.len).unwrap_or(0);
    let mut meta = BTreeMap::new();
    meta.insert("path".to_string(), serde_json::json!(path));
    if path.is_empty() {
        meta.insert(
            "note".to_string(),
            serde_json::json!("directory listing requires a guest-safe VFS list facade"),
        );
    }
    FileNode {
        id: if path.is_empty() {
            "root".to_string()
        } else {
            path.to_string()
        },
        name,
        kind,
        size,
        modified: now_iso(),
        meta,
        actions: match kind {
            NodeKind::File => vec!["open", "download", "delete"],
            NodeKind::Folder => vec!["upload", "create"],
        },
        children: Vec::new(),
    }
}

async fn handle_index() -> Response {
    text_response(200, "text/html; charset=utf-8", &index_html())
}

async fn handle_tailwind_css() -> Response {
    text_response(200, "text/css; charset=utf-8", TRUEOS_TAILWIND_CSS)
}

async fn handle_healthz() -> Response {
    json_response(
        200,
        &serde_json::json!({
            "ok": true,
            "service": "fileexplorer-http",
            "port": FILEEXPLORER_HTTP_PORT.load(Ordering::Acquire),
            "ports": [FILEEXPLORER_HTTP_TCP_PORT, FILEEXPLORER_ALT_TCP_PORT],
        }),
    )
}

async fn handle_tree(uri: Uri) -> Response {
    let root_id = uri.query().and_then(|query| {
        query.split('&').find_map(|pair| {
            let (key, value) = pair.split_once('=')?;
            (key == "rootId").then_some(value)
        })
    });
    let path = match root_id {
        Some(id) => match decode_node_id(id) {
            Ok(path) => path,
            Err(err) => return error_response(400, err),
        },
        None => String::new(),
    };
    json_response(
        200,
        &TreeSnapshot {
            schema: "filetree.v1",
            version: now_ms(),
            root: node_for_path(path.as_str()),
        },
    )
}

async fn record_job(
    state: AppState,
    operation: &str,
    label: String,
    affected_node_ids: Vec<String>,
    result: Result<Option<Value>, String>,
) -> Response {
    let id = format!("job-{}", JOB_SEQ.fetch_add(1, Ordering::Relaxed));
    let now = now_ms();
    let (status, progress, result, error) = match result {
        Ok(result) => ("completed", 100, result, None),
        Err(err) => ("failed", 100, None, Some(err)),
    };
    state.jobs.write().await.insert(
        id.clone(),
        JobRecord {
            id: id.clone(),
            operation: operation.to_string(),
            status,
            progress,
            description: label.clone(),
            affected_node_ids,
            created_at_ms: now,
            updated_at_ms: now,
            result,
            error,
        },
    );
    json_response(
        202,
        &AcceptedJob {
            job_id: id.clone(),
            label,
            status_url: format!("/api/jobs/{id}"),
            events_url: "/api/jobs/events".to_string(),
        },
    )
}

async fn handle_create_node(State(state): State<AppState>, body: Bytes) -> Response {
    if body.len() > FILEEXPLORER_HTTP_BODY_MAX {
        return error_response(413, "request too large");
    }
    let request = match serde_json::from_slice::<CreateNodeRequest>(&body) {
        Ok(request) => request,
        Err(_) => return error_response(400, "bad json"),
    };
    let result = if request.kind == NodeKind::Folder {
        let path = format!(
            "{}/{}",
            decode_node_id(&request.parent_id).unwrap_or_default(),
            request.name
        )
        .trim_matches('/')
        .to_string();
        vfs::create_dir_all(path.as_bytes())
            .map(|_| Some(serde_json::json!({ "path": path })))
            .map_err(|err| format!("create dir failed rc={}", err))
    } else {
        Err("file creation requires upload bytes".to_string())
    };
    record_job(
        state,
        "node_create",
        format!("Create {}", request.name),
        vec![request.parent_id],
        result,
    )
    .await
}

async fn handle_update_node(
    State(state): State<AppState>,
    Path(id): Path<String>,
    body: Bytes,
) -> Response {
    let request = serde_json::from_slice::<UpdateNodeRequest>(&body)
        .unwrap_or(UpdateNodeRequest { name: None });
    record_job(
        state,
        "node_update",
        format!("Update {}", id),
        vec![id],
        Err(format!(
            "rename is not exposed in blueprint VFS yet; requested name={:?}",
            request.name
        )),
    )
    .await
}

async fn handle_delete_node(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    record_job(
        state,
        "node_delete",
        format!("Delete {}", id),
        vec![id],
        Err("remove is not exposed through the blueprint API package yet".to_string()),
    )
    .await
}

async fn handle_delete_nodes(State(state): State<AppState>, body: Bytes) -> Response {
    let request = serde_json::from_slice::<DeleteNodesRequest>(&body)
        .unwrap_or(DeleteNodesRequest { ids: Vec::new() });
    record_job(
        state,
        "node_delete",
        format!("Delete {} item(s)", request.ids.len()),
        request.ids,
        Err("bulk remove is not exposed through the blueprint API package yet".to_string()),
    )
    .await
}

async fn handle_move_nodes(State(state): State<AppState>, body: Bytes) -> Response {
    let request = serde_json::from_slice::<MoveNodesRequest>(&body)
        .unwrap_or(MoveNodesRequest { ids: Vec::new() });
    record_job(
        state,
        "multi_move",
        format!("Move {} item(s)", request.ids.len()),
        request.ids,
        Err("rename/move is not exposed in blueprint VFS yet".to_string()),
    )
    .await
}

async fn handle_upload_file(
    State(state): State<AppState>,
    Path(parent_id): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    if body.len() > FILEEXPLORER_UPLOAD_BODY_MAX {
        return error_response(413, "upload too large");
    }
    let name = headers
        .get("x-file-name")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| percent_decode(value).ok())
        .unwrap_or_else(|| "upload.bin".to_string());
    let parent = decode_node_id(&parent_id).unwrap_or_default();
    let path = format!("{}/{}", parent, name).trim_matches('/').to_string();
    let result = vfs::write_file(path.as_bytes(), body.as_ref())
        .map(|_| Some(serde_json::json!({ "path": path, "bytes": body.len() })))
        .map_err(|err| format!("write failed rc={}", err));
    record_job(
        state,
        "node_upload",
        format!("Upload {}", name),
        vec![parent_id],
        result,
    )
    .await
}

async fn handle_download_file(Path(id): Path<String>) -> Response {
    let path = match decode_node_id(&id) {
        Ok(path) if !path.is_empty() => path,
        _ => return error_response(404, "bad path"),
    };
    match vfs::read_file(path.as_bytes()) {
        Ok(bytes) => Response::builder()
            .status(StatusCode::OK)
            .header(CONTENT_TYPE, "application/octet-stream")
            .header(CONTENT_LENGTH, bytes.len().to_string())
            .header(
                CONTENT_DISPOSITION,
                format!(
                    "attachment; filename=\"{}\"",
                    path.rsplit('/').next().unwrap_or("download.bin")
                ),
            )
            .body(Body::from(bytes))
            .unwrap_or_else(|_| Response::new(Body::empty())),
        Err(err) => error_response(404, format!("read failed rc={}", err)),
    }
}

async fn handle_node_content(Path(id): Path<String>) -> Response {
    let path = match decode_node_id(&id) {
        Ok(path) if !path.is_empty() => path,
        _ => return error_response(404, "bad path"),
    };
    match vfs::read_file(path.as_bytes()) {
        Ok(bytes) => response(200, "text/plain; charset=utf-8", bytes),
        Err(err) => error_response(404, format!("read failed rc={}", err)),
    }
}

async fn handle_job(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    match state.jobs.read().await.get(&id).cloned() {
        Some(record) => json_response(200, &record),
        None => error_response(404, "job not found"),
    }
}

async fn handle_job_events() -> Response {
    text_response(
        200,
        "text/event-stream; charset=utf-8",
        "event: ready\ndata: {\"ok\":true}\n\n",
    )
}

fn router() -> Router {
    let state = AppState {
        jobs: Arc::new(tokio::sync::RwLock::new(BTreeMap::new())),
    };
    Router::new()
        .route("/", get(handle_index))
        .route("/index.html", get(handle_index))
        .route("/tailwind.css", get(handle_tailwind_css))
        .route("/healthz", get(handle_healthz))
        .route("/api/healthz", get(handle_healthz))
        .route("/api/tree", get(handle_tree).put(handle_tree))
        .route("/api/nodes", post(handle_create_node))
        .route(
            "/api/nodes/{id}",
            patch(handle_update_node).delete(handle_delete_node),
        )
        .route("/api/nodes/{id}/content", get(handle_node_content))
        .route("/api/nodes/{id}/download", get(handle_download_file))
        .route("/api/nodes/{id}/upload", post(handle_upload_file))
        .route("/api/nodes/delete", post(handle_delete_nodes))
        .route("/api/nodes/move", post(handle_move_nodes))
        .route("/api/jobs/{id}", get(handle_job))
        .route("/api/jobs/events", get(handle_job_events))
        .layer(DefaultBodyLimit::max(FILEEXPLORER_UPLOAD_BODY_MAX))
        .with_state(state)
}

async fn serve_port(app: Router, port: u16) {
    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    loop {
        let listener = match tokio::net::TcpListener::bind(addr).await {
            Ok(listener) => listener,
            Err(err) => {
                logl::log(
                    level::WARN,
                    format_args!("fileexplorer-http: bind {} failed {}", addr, err),
                );
                time::sleep(Duration::from_millis(FILEEXPLORER_BIND_RETRY_MS)).await;
                continue;
            }
        };
        if port == FILEEXPLORER_HTTP_TCP_PORT {
            FILEEXPLORER_HTTP_PORT.store(port, Ordering::Release);
        }
        logl::log(
            level::INFO,
            format_args!("fileexplorer-http: axum listening on http://{}/", addr),
        );
        let listener = listener.tap_io(move |_| {
            logl::log(
                level::INFO,
                format_args!("fileexplorer-http: tcp accepted port={}", port),
            )
        });
        if let Err(err) = axum::serve(listener, app.clone()).await {
            logl::log(
                level::WARN,
                format_args!("fileexplorer-http: serve failed port={} err={}", port, err),
            );
        }
        if port == FILEEXPLORER_HTTP_TCP_PORT {
            FILEEXPLORER_HTTP_PORT.store(0, Ordering::Release);
        }
        time::sleep(Duration::from_millis(FILEEXPLORER_BIND_RETRY_MS)).await;
    }
}

async fn fileexplorer_http_runtime() {
    let app = router();
    tokio::task::spawn_local(serve_port(app.clone(), FILEEXPLORER_ALT_TCP_PORT));
    serve_port(app, FILEEXPLORER_HTTP_TCP_PORT).await;
}

fn main() {
    logl::log(level::INFO, "fileexplorer-http: blueprint start");
    let runtime = match runtime::current_thread_net().build() {
        Ok(runtime) => runtime,
        Err(err) => {
            logl::log(
                level::ERROR,
                format_args!("fileexplorer-http: runtime build failed {}", err),
            );
            return;
        }
    };
    let local = tokio::task::LocalSet::new();
    local.block_on(&runtime, fileexplorer_http_runtime());
    platform::poll_once();
}
