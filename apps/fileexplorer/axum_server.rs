// trueos-blueprint: features=["tokio-net-probe"]

extern crate alloc;

use alloc::{
    boxed::Box,
    collections::BTreeMap,
    format,
    string::{String, ToString},
    sync::Arc,
    vec,
    vec::Vec,
};
use core::future::Future;
use core::pin::Pin;
use core::sync::atomic::{AtomicU16, AtomicU64, Ordering};
use std::env;

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
    clock, fs, logl,
    logl::level,
    platform::{self, io},
    runtime,
    time::{self, Duration},
    tokio::{self, net::SocketAddr},
};

const FILEEXPLORER_HTTP_TCP_PORT: u16 = 8;
const FILEEXPLORER_ALT_TCP_PORT: u16 = 6;
const FILEEXPLORER_HTTP_BODY_MAX: usize = 64 * 1024;
const FILEEXPLORER_UPLOAD_BODY_MAX: usize = 16 * 1024 * 1024;
const FILEEXPLORER_BIND_RETRY_MS: u64 = 1000;
const FILEEXPLORER_INDEX_HTML: &str = include_str!("index.html");
const TRUEOS_TAILWIND_CSS: &str = include_str!("tailwind.css");
const FILEEXPLORER_TREE_MAX_DEPTH: usize = 8;
const FILEEXPLORER_TREE_MAX_NODES: usize = 2048;

static FILEEXPLORER_HTTP_PORT: AtomicU16 = AtomicU16::new(0);
static JOB_SEQ: AtomicU64 = AtomicU64::new(1);

type JobMap = Arc<tokio::sync::RwLock<BTreeMap<String, JobRecord>>>;

#[derive(Clone)]
struct AppState {
    jobs: JobMap,
    app_root: String,
    common_root: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RootScope {
    App,
    Common,
}

#[derive(Debug, Clone)]
struct ResolvedNode {
    scope: RootScope,
    rel: String,
    physical_path: String,
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

fn default_app_root() -> String {
    env::var("TRUEOS_APP_FS_ROOT")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| ".".to_string())
}

fn default_common_root() -> Option<String> {
    env::var("TRUEOS_APP_COMMON")
        .or_else(|_| env::var("TRUEOS_APP_FS_COMMON"))
        .ok()
        .filter(|value| !value.trim().is_empty())
}

fn normalize_root(root: String) -> String {
    let trimmed = root.trim();
    if trimmed.is_empty() {
        ".".to_string()
    } else if trimmed == "/" {
        "/".to_string()
    } else {
        trimmed.trim_end_matches('/').to_string()
    }
}

fn root_id(scope: RootScope) -> &'static str {
    match scope {
        RootScope::App => "app",
        RootScope::Common => "common",
    }
}

fn root_label(scope: RootScope) -> &'static str {
    match scope {
        RootScope::App => "app://",
        RootScope::Common => "common://",
    }
}

fn logical_id(scope: RootScope, rel: &str) -> String {
    let root = root_id(scope);
    if rel.is_empty() {
        root.to_string()
    } else {
        format!("{root}/{rel}")
    }
}

fn logical_path(scope: RootScope, rel: &str) -> String {
    let root = root_label(scope);
    if rel.is_empty() {
        root.to_string()
    } else {
        format!("{root}{rel}")
    }
}

fn display_name(scope: RootScope, rel: &str) -> String {
    if rel.is_empty() {
        root_label(scope).to_string()
    } else {
        rel.rsplit('/').next().unwrap_or(rel).to_string()
    }
}

fn join_rel(parent: &str, name: &str) -> String {
    if parent.is_empty() {
        name.to_string()
    } else {
        format!("{parent}/{name}")
    }
}

fn join_physical(root: &str, rel: &str) -> String {
    if rel.is_empty() {
        root.to_string()
    } else if root == "/" {
        format!("/{rel}")
    } else {
        format!("{}/{}", root.trim_end_matches('/'), rel)
    }
}

fn validate_rel_path(rel: &str) -> Result<String, String> {
    let rel = rel.trim().trim_matches('/');
    if rel.is_empty() {
        return Ok(String::new());
    }
    if rel
        .split('/')
        .any(|segment| segment.is_empty() || segment == "." || segment == "..")
    {
        return Err("bad path".to_string());
    }
    Ok(rel.to_string())
}

fn sanitize_child_name(name: &str) -> Result<String, String> {
    let name = name.trim();
    if name.is_empty() || name == "." || name == ".." || name.contains('/') || name.contains('\\') {
        return Err("bad file name".to_string());
    }
    Ok(name.to_string())
}

fn resolve_node_id(state: &AppState, id: &str) -> Result<Option<ResolvedNode>, String> {
    let id = decode_node_id(id)?;
    if id.is_empty() {
        return Ok(None);
    }

    let (scope, rel) = if id == root_id(RootScope::App) {
        (RootScope::App, String::new())
    } else if let Some(rel) = id.strip_prefix("app/") {
        (RootScope::App, validate_rel_path(rel)?)
    } else if id == root_id(RootScope::Common) {
        (RootScope::Common, String::new())
    } else if let Some(rel) = id.strip_prefix("common/") {
        (RootScope::Common, validate_rel_path(rel)?)
    } else {
        return Err("node is outside app/common roots".to_string());
    };

    let root = match scope {
        RootScope::App => state.app_root.as_str(),
        RootScope::Common => state
            .common_root
            .as_deref()
            .ok_or_else(|| "common root unavailable".to_string())?,
    };

    Ok(Some(ResolvedNode {
        scope,
        physical_path: join_physical(root, &rel),
        rel,
    }))
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

fn file_actions(kind: NodeKind) -> Vec<&'static str> {
    match kind {
        NodeKind::File => vec!["open", "download", "delete"],
        NodeKind::Folder => vec!["open", "upload", "create"],
    }
}

async fn root_overview_node(state: &AppState) -> FileNode {
    let mut meta = BTreeMap::new();
    meta.insert("path".to_string(), serde_json::json!("trueos://"));

    let mut budget = FILEEXPLORER_TREE_MAX_NODES;
    let mut children = Vec::new();
    children.push(scan_scope_root(RootScope::App, state.app_root.as_str(), &mut budget).await);
    if let Some(common_root) = state.common_root.as_deref() {
        children.push(scan_scope_root(RootScope::Common, common_root, &mut budget).await);
    }

    FileNode {
        id: "root".to_string(),
        name: "TRUEOSFS".to_string(),
        kind: NodeKind::Folder,
        size: 0,
        modified: now_iso(),
        meta,
        actions: vec!["open"],
        children,
    }
}

async fn scan_scope_root(scope: RootScope, root: &str, budget: &mut usize) -> FileNode {
    scan_node(
        scope,
        "",
        root,
        Some(root),
        FILEEXPLORER_TREE_MAX_DEPTH,
        budget,
    )
    .await
    .unwrap_or_else(|| {
        let mut meta = BTreeMap::new();
        meta.insert("path".to_string(), serde_json::json!(root_label(scope)));
        meta.insert("physicalPath".to_string(), serde_json::json!(root));
        meta.insert(
            "error".to_string(),
            serde_json::json!("root could not be listed"),
        );
        FileNode {
            id: logical_id(scope, ""),
            name: display_name(scope, ""),
            kind: NodeKind::Folder,
            size: 0,
            modified: now_iso(),
            meta,
            actions: file_actions(NodeKind::Folder),
            children: Vec::new(),
        }
    })
}

fn scan_node<'a>(
    scope: RootScope,
    rel: &'a str,
    path: &'a str,
    forced_root: Option<&'a str>,
    depth: usize,
    budget: &'a mut usize,
) -> Pin<Box<dyn Future<Output = Option<FileNode>> + Send + 'a>> {
    Box::pin(async move {
        if *budget == 0 {
            return None;
        }
        *budget = budget.saturating_sub(1);

        let (kind, size) = if forced_root.is_some() {
            (NodeKind::Folder, 0)
        } else {
            stat_path(path).await?
        };
        let mut meta = BTreeMap::new();
        meta.insert(
            "path".to_string(),
            serde_json::json!(logical_path(scope, rel)),
        );
        meta.insert("physicalPath".to_string(), serde_json::json!(path));

        let mut children = Vec::new();
        if kind == NodeKind::Folder && depth > 0 && *budget > 0 {
            match list_dir(path).await {
                Ok(mut names) => {
                    names.sort();
                    for name in names.into_iter().filter(|name| !name.starts_with('.')) {
                        let child_rel = join_rel(rel, &name);
                        let child_path = join_physical(path, &name);
                        if let Some(child) =
                            scan_node(scope, &child_rel, &child_path, None, depth - 1, budget).await
                        {
                            children.push(child);
                        }
                    }
                }
                Err(err) => {
                    meta.insert("listError".to_string(), serde_json::json!(err));
                }
            }
        }

        Some(FileNode {
            id: logical_id(scope, rel),
            name: display_name(scope, rel),
            kind,
            size,
            modified: now_iso(),
            meta,
            actions: file_actions(kind),
            children,
        })
    })
}

async fn list_dir(path: &str) -> Result<Vec<String>, String> {
    let mut out = Vec::new();
    let mut entries = fs::read_dir(path)
        .await
        .map_err(|err| format!("list failed {}", err))?;
    while let Some(entry) = entries
        .next_entry()
        .await
        .map_err(|err| format!("list entry failed {}", err))?
    {
        #[cfg(any(target_os = "trueos", target_os = "zkvm"))]
        let name = entry.file_name();
        #[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
        let name = entry.file_name().to_string_lossy().into_owned();
        out.push(name);
    }
    Ok(out)
}

async fn stat_path(path: &str) -> Option<(NodeKind, u64)> {
    let metadata = fs::metadata(path).await.ok()?;
    let kind = if metadata.is_file() {
        NodeKind::File
    } else if metadata.is_dir() {
        NodeKind::Folder
    } else {
        return None;
    };
    Some((kind, metadata.len()))
}

async fn node_for_resolved(node: &ResolvedNode, state: &AppState) -> FileNode {
    let mut budget = FILEEXPLORER_TREE_MAX_NODES;
    scan_node(
        node.scope,
        &node.rel,
        &node.physical_path,
        if node.rel.is_empty() {
            match node.scope {
                RootScope::App => Some(state.app_root.as_str()),
                RootScope::Common => state.common_root.as_deref(),
            }
        } else {
            None
        },
        FILEEXPLORER_TREE_MAX_DEPTH,
        &mut budget,
    )
    .await
    .unwrap_or_else(|| {
        let mut meta = BTreeMap::new();
        meta.insert(
            "path".to_string(),
            serde_json::json!(logical_path(node.scope, &node.rel)),
        );
        meta.insert(
            "physicalPath".to_string(),
            serde_json::json!(node.physical_path),
        );
        FileNode {
            id: logical_id(node.scope, &node.rel),
            name: display_name(node.scope, &node.rel),
            kind: NodeKind::Folder,
            size: 0,
            modified: now_iso(),
            meta,
            actions: file_actions(NodeKind::Folder),
            children: Vec::new(),
        }
    })
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

async fn handle_tree(State(state): State<AppState>, uri: Uri) -> Response {
    let root_id = uri.query().and_then(|query| {
        query.split('&').find_map(|pair| {
            let (key, value) = pair.split_once('=')?;
            (key == "rootId").then_some(value)
        })
    });
    let root = match root_id {
        Some(id) => match resolve_node_id(&state, id) {
            Ok(Some(node)) => node_for_resolved(&node, &state).await,
            Ok(None) => root_overview_node(&state).await,
            Err(err) => return error_response(400, err),
        },
        None => root_overview_node(&state).await,
    };
    json_response(
        200,
        &TreeSnapshot {
            schema: "filetree.v1",
            version: now_ms(),
            root,
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
        Ok(result) => ("succeeded", 100, result, None),
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
        let parent = match resolve_node_id(&state, &request.parent_id) {
            Ok(Some(parent)) => parent,
            Ok(None) => {
                return record_job(
                    state,
                    "node_create",
                    format!("Create {}", request.name),
                    vec![request.parent_id],
                    Err("choose app:// or common:// first".to_string()),
                )
                .await;
            }
            Err(err) => {
                return record_job(
                    state,
                    "node_create",
                    format!("Create {}", request.name),
                    vec![request.parent_id],
                    Err(err),
                )
                .await;
            }
        };
        let name = match sanitize_child_name(&request.name) {
            Ok(name) => name,
            Err(err) => {
                return record_job(
                    state,
                    "node_create",
                    format!("Create {}", request.name),
                    vec![request.parent_id],
                    Err(err),
                )
                .await;
            }
        };
        let path = join_physical(&parent.physical_path, &name);
        let id = logical_id(parent.scope, &join_rel(&parent.rel, &name));
        fs::create_dir_all(path.as_str())
            .await
            .map(|_| Some(serde_json::json!({ "id": id, "path": path })))
            .map_err(|err| format!("create dir failed {}", err))
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
            "rename is not exposed through the filesystem facade yet; requested name={:?}",
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
        Err("rename/move is not exposed through the filesystem facade yet".to_string()),
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
    let parent = match resolve_node_id(&state, &parent_id) {
        Ok(Some(parent)) => parent,
        Ok(None) => {
            return record_job(
                state,
                "node_upload",
                format!("Upload {}", name),
                vec![parent_id],
                Err("choose app:// or common:// first".to_string()),
            )
            .await;
        }
        Err(err) => {
            return record_job(
                state,
                "node_upload",
                format!("Upload {}", name),
                vec![parent_id],
                Err(err),
            )
            .await;
        }
    };
    let name = match sanitize_child_name(&name) {
        Ok(name) => name,
        Err(err) => {
            return record_job(
                state,
                "node_upload",
                format!("Upload {}", name),
                vec![parent_id],
                Err(err),
            )
            .await;
        }
    };
    let path = join_physical(&parent.physical_path, &name);
    let id = logical_id(parent.scope, &join_rel(&parent.rel, &name));
    let result = fs::write(path.as_str(), body.as_ref())
        .await
        .map(|_| Some(serde_json::json!({ "id": id, "path": path, "bytes": body.len() })))
        .map_err(|err| format!("write failed {}", err));
    record_job(
        state,
        "node_upload",
        format!("Upload {}", name),
        vec![parent_id],
        result,
    )
    .await
}

async fn handle_download_file(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    let node = match resolve_node_id(&state, &id) {
        Ok(Some(node)) => node,
        Ok(None) => return error_response(404, "bad path"),
        Err(err) => return error_response(404, err),
    };
    match fs::read(node.physical_path.as_str()).await {
        Ok(bytes) => Response::builder()
            .status(StatusCode::OK)
            .header(CONTENT_TYPE, "application/octet-stream")
            .header(CONTENT_LENGTH, bytes.len().to_string())
            .header(
                CONTENT_DISPOSITION,
                format!(
                    "attachment; filename=\"{}\"",
                    node.rel.rsplit('/').next().unwrap_or("download.bin")
                ),
            )
            .body(Body::from(bytes))
            .unwrap_or_else(|_| Response::new(Body::empty())),
        Err(err) => error_response(404, format!("read failed {}", err)),
    }
}

async fn handle_node_content(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    let node = match resolve_node_id(&state, &id) {
        Ok(Some(node)) => node,
        Ok(None) => return error_response(404, "bad path"),
        Err(err) => return error_response(404, err),
    };
    match fs::read(node.physical_path.as_str()).await {
        Ok(bytes) => response(200, "text/plain; charset=utf-8", bytes),
        Err(err) => error_response(404, format!("read failed {}", err)),
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

fn new_state() -> AppState {
    AppState {
        jobs: Arc::new(tokio::sync::RwLock::new(BTreeMap::new())),
        app_root: normalize_root(default_app_root()),
        common_root: default_common_root().map(normalize_root),
    }
}

async fn setup_roots(state: &AppState) {
    if let Err(err) = fs::create_dir_all(state.app_root.as_str()).await {
        logl::log(
            level::WARN,
            format_args!(
                "fileexplorer-http: app root setup failed path={} err={}",
                state.app_root, err
            ),
        );
    }
    if let Some(common_root) = state.common_root.as_deref() {
        if let Err(err) = fs::create_dir_all(common_root).await {
            logl::log(
                level::WARN,
                format_args!(
                    "fileexplorer-http: common root setup failed path={} err={}",
                    common_root, err
                ),
            );
        }
    }
}

fn router(state: AppState) -> Router {
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
    let state = new_state();
    logl::log(
        level::INFO,
        format_args!(
            "fileexplorer-http: roots app={} common={}",
            state.app_root,
            state.common_root.as_deref().unwrap_or("<none>")
        ),
    );
    setup_roots(&state).await;
    let app = router(state);
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
