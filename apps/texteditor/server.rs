// trueos-blueprint: features=["tokio-net-probe"]

extern crate alloc;

use alloc::{
    format,
    string::{String, ToString},
    vec::Vec,
};
use core::sync::atomic::{AtomicU16, Ordering};

#[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
use std::fs as host_fs;

use axum::{
    Router,
    body::{Body, Bytes},
    http::{
        StatusCode, Uri,
        header::{CACHE_CONTROL, CONTENT_DISPOSITION, CONTENT_LENGTH, CONTENT_TYPE},
    },
    response::Response,
    routing::get,
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

const TEXTEDITOR_HTTP_TCP_PORT: u16 = 1010;
const TEXTEDITOR_BIND_RETRY_MS: u64 = 1000;
const TEXTEDITOR_BODY_MAX: usize = 2 * 1024 * 1024;
const TEXTEDITOR_FS_LIST_MAX: usize = 256;
const TEXTEDITOR_STORE_DIR: &str = "texteditor";
const TEXTEDITOR_STORE_PATH: &str = "texteditor/document.json";
const TEXTEDITOR_INDEX_HTML: &str = include_str!("index.html");
const TEXTEDITOR_APP_JS: &str = include_str!("static/app.js");
const TEXTEDITOR_APP_CSS: &str = include_str!("static/app.css");

static TEXTEDITOR_HTTP_PORT: AtomicU16 = AtomicU16::new(0);

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ExportSnapshot {
    #[serde(default)]
    markdown: String,
    #[serde(default)]
    html: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DocumentEnvelope {
    schema: String,
    updated_at_s: u64,
    blocks: Value,
    exports: ExportSnapshot,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SaveDocumentRequest {
    blocks: Value,
    #[serde(default)]
    markdown: String,
    #[serde(default)]
    html: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum FsNodeKind {
    File,
    Folder,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct FsEntry {
    name: String,
    path: String,
    kind: FsNodeKind,
    size: u64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateFolderRequest {
    path: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StoreCopyRequest {
    path: String,
    format: String,
    blocks: Value,
    #[serde(default)]
    markdown: String,
    #[serde(default)]
    html: String,
}

fn current_port() -> Option<u16> {
    match TEXTEDITOR_HTTP_PORT.load(Ordering::Acquire) {
        0 => None,
        port => Some(port),
    }
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
            if no_store { "no-store" } else { "no-cache" },
        )
        .body(Body::from(body))
        .unwrap_or_else(|_| Response::new(Body::empty()))
}

fn download_response(
    status: u16,
    content_type: &'static str,
    filename: &'static str,
    body: Vec<u8>,
) -> Response {
    Response::builder()
        .status(status_code(status))
        .header(CONTENT_TYPE, content_type)
        .header(CONTENT_LENGTH, body.len().to_string())
        .header(
            CONTENT_DISPOSITION,
            format!("attachment; filename=\"{filename}\""),
        )
        .header(CACHE_CONTROL, "no-store")
        .body(Body::from(body))
        .unwrap_or_else(|_| Response::new(Body::empty()))
}

fn text_response(status: u16, content_type: &'static str, body: &str) -> Response {
    response(status, content_type, body.as_bytes().to_vec(), false)
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

fn default_blocks() -> Value {
    serde_json::json!([
        {
            "type": "heading",
            "props": {
                "level": 1
            },
            "content": "TRUEOS Text Editor"
        },
        {
            "type": "paragraph",
            "content": "Start writing here. Use the slash menu for headings, lists, tables, images, and code blocks."
        },
        {
            "type": "bulletListItem",
            "content": "Saved as BlockNote JSON on the TRUEOS filesystem."
        },
        {
            "type": "bulletListItem",
            "content": "Export snapshots are available as JSON, Markdown, or HTML."
        }
    ])
}

fn default_document() -> DocumentEnvelope {
    DocumentEnvelope {
        schema: "trueos.texteditor.document.v1".to_string(),
        updated_at_s: clock::ntp_current_unix_seconds(),
        blocks: default_blocks(),
        exports: ExportSnapshot {
            markdown: String::new(),
            html: String::new(),
        },
    }
}

fn validate_blocks(blocks: &Value) -> Result<(), &'static str> {
    match blocks {
        Value::Array(items) if !items.is_empty() => Ok(()),
        Value::Array(_) => Err("document must contain at least one block"),
        _ => Err("blocks must be a JSON array"),
    }
}

fn normalize_fs_path(path: &str) -> Result<String, String> {
    let path = path.trim().replace('\\', "/");
    if path.is_empty() || path == "." || path == "/" {
        return Ok(".".to_string());
    }
    if path
        .split('/')
        .any(|segment| segment == ".." || segment.contains('\0'))
    {
        return Err("bad path".to_string());
    }
    Ok(path.trim_matches('/').to_string())
}

fn join_path(parent: &str, name: &str) -> String {
    if parent == "." || parent.is_empty() {
        name.to_string()
    } else {
        format!("{}/{}", parent.trim_end_matches('/'), name)
    }
}

fn parent_dir(path: &str) -> Option<String> {
    path.rsplit_once('/').and_then(|(parent, _)| {
        let parent = parent.trim();
        (!parent.is_empty()).then(|| parent.to_string())
    })
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

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
const TRUEOS_FS_KIND_FILE: u32 = 1;
#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
const TRUEOS_FS_KIND_DIR: u32 = 2;

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
unsafe extern "C" {
    fn trueos_cabi_fs_list_dir(
        path_ptr: *const u8,
        path_len: usize,
        out_ptr: *mut u8,
        out_cap: usize,
    ) -> isize;
    fn trueos_cabi_fs_stat(
        path_ptr: *const u8,
        path_len: usize,
        out_kind: *mut u32,
        out_len: *mut u64,
    ) -> i32;
}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
fn list_dir_names(path: &str) -> Result<Vec<String>, String> {
    let bytes = path.as_bytes();
    let len =
        unsafe { trueos_cabi_fs_list_dir(bytes.as_ptr(), bytes.len(), core::ptr::null_mut(), 0) };
    if len < 0 {
        return Err("list failed".to_string());
    }

    let mut out = vec![0u8; len as usize];
    let got = unsafe {
        trueos_cabi_fs_list_dir(bytes.as_ptr(), bytes.len(), out.as_mut_ptr(), out.len())
    };
    if got < 0 {
        return Err("list failed".to_string());
    }

    out.truncate(got as usize);
    let text = String::from_utf8(out).map_err(|_| "bad utf8 in directory listing".to_string())?;
    Ok(text
        .lines()
        .filter(|line| !line.is_empty())
        .map(String::from)
        .collect())
}

#[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
fn list_dir_names(path: &str) -> Result<Vec<String>, String> {
    let mut out = Vec::new();
    for entry in host_fs::read_dir(path).map_err(|err| format!("list failed {err}"))? {
        let entry = entry.map_err(|err| format!("list entry failed {err}"))?;
        out.push(entry.file_name().to_string_lossy().into_owned());
    }
    Ok(out)
}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
fn stat_path(path: &str) -> Option<(FsNodeKind, u64)> {
    let bytes = path.as_bytes();
    let mut kind = 0u32;
    let mut len = 0u64;
    let rc = unsafe {
        trueos_cabi_fs_stat(
            bytes.as_ptr(),
            bytes.len(),
            &mut kind as *mut u32,
            &mut len as *mut u64,
        )
    };
    if rc != 0 {
        return None;
    }
    match kind {
        TRUEOS_FS_KIND_FILE => Some((FsNodeKind::File, len)),
        TRUEOS_FS_KIND_DIR => Some((FsNodeKind::Folder, len)),
        _ => None,
    }
}

#[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
fn stat_path(path: &str) -> Option<(FsNodeKind, u64)> {
    let metadata = host_fs::metadata(path).ok()?;
    let kind = if metadata.is_dir() {
        FsNodeKind::Folder
    } else {
        FsNodeKind::File
    };
    Some((kind, metadata.len()))
}

fn list_entries(path: &str) -> Result<Vec<FsEntry>, String> {
    let mut entries = Vec::new();
    for name in list_dir_names(path)?
        .into_iter()
        .take(TEXTEDITOR_FS_LIST_MAX)
    {
        if name == "." || name == ".." {
            continue;
        }
        let child_path = join_path(path, &name);
        let Some((kind, size)) = stat_path(&child_path) else {
            continue;
        };
        entries.push(FsEntry {
            name,
            path: child_path,
            kind,
            size,
        });
    }
    entries.sort_by(|a, b| {
        let rank_a = if a.kind == FsNodeKind::Folder { 0 } else { 1 };
        let rank_b = if b.kind == FsNodeKind::Folder { 0 } else { 1 };
        rank_a
            .cmp(&rank_b)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    Ok(entries)
}

async fn load_document() -> DocumentEnvelope {
    let bytes = match fs::read(TEXTEDITOR_STORE_PATH).await {
        Ok(bytes) => bytes,
        Err(_) => return default_document(),
    };
    match serde_json::from_slice::<DocumentEnvelope>(&bytes) {
        Ok(document) => document,
        Err(err) => {
            logl::log(
                level::WARN,
                format_args!("texteditor-http: ignored bad store file {}", err),
            );
            default_document()
        }
    }
}

async fn save_document(document: &DocumentEnvelope) -> Result<(), String> {
    if let Err(err) = fs::create_dir_all(TEXTEDITOR_STORE_DIR).await {
        return Err(format!("create store dir failed: {err}"));
    }
    let bytes =
        serde_json::to_vec(document).map_err(|_| "serialize document failed".to_string())?;
    fs::write(TEXTEDITOR_STORE_PATH, bytes.as_slice())
        .await
        .map_err(|err| format!("write document failed: {err}"))
}

async fn handle_index() -> Response {
    text_response(200, "text/html; charset=utf-8", TEXTEDITOR_INDEX_HTML)
}

async fn handle_app_js() -> Response {
    text_response(
        200,
        "application/javascript; charset=utf-8",
        TEXTEDITOR_APP_JS,
    )
}

async fn handle_app_css() -> Response {
    text_response(200, "text/css; charset=utf-8", TEXTEDITOR_APP_CSS)
}

async fn handle_status() -> Response {
    json_response(
        200,
        &serde_json::json!({
            "ok": true,
            "service": "texteditor-http",
            "port": current_port(),
            "storePath": TEXTEDITOR_STORE_PATH,
            "format": "blocknote-json",
            "generatedAtS": clock::ntp_current_unix_seconds(),
        }),
    )
}

async fn handle_document_get() -> Response {
    let document = load_document().await;
    json_response(
        200,
        &serde_json::json!({
            "ok": true,
            "document": document,
        }),
    )
}

async fn handle_document_save(body: Bytes) -> Response {
    if body.len() > TEXTEDITOR_BODY_MAX {
        return error_response(413, "request too large");
    }
    let req = match serde_json::from_slice::<SaveDocumentRequest>(body.as_ref()) {
        Ok(req) => req,
        Err(_) => return error_response(400, "bad json"),
    };
    if let Err(err) = validate_blocks(&req.blocks) {
        return error_response(400, err);
    }

    let document = DocumentEnvelope {
        schema: "trueos.texteditor.document.v1".to_string(),
        updated_at_s: clock::ntp_current_unix_seconds(),
        blocks: req.blocks,
        exports: ExportSnapshot {
            markdown: req.markdown,
            html: req.html,
        },
    };

    match save_document(&document).await {
        Ok(()) => json_response(
            200,
            &serde_json::json!({
                "ok": true,
                "document": document,
            }),
        ),
        Err(err) => error_response(500, err),
    }
}

async fn handle_fs_list(uri: Uri) -> Response {
    let raw_path = uri
        .query()
        .and_then(|query| {
            query.split('&').find_map(|pair| {
                let (key, value) = pair.split_once('=')?;
                (key == "path").then_some(value)
            })
        })
        .unwrap_or(".");
    let decoded = match percent_decode(raw_path) {
        Ok(path) => path,
        Err(err) => return error_response(400, err),
    };
    let path = match normalize_fs_path(&decoded) {
        Ok(path) => path,
        Err(err) => return error_response(400, err),
    };
    match list_entries(&path) {
        Ok(entries) => json_response(
            200,
            &serde_json::json!({
                "ok": true,
                "path": path,
                "entries": entries,
            }),
        ),
        Err(err) => error_response(400, err),
    }
}

async fn handle_fs_mkdir(body: Bytes) -> Response {
    if body.len() > TEXTEDITOR_BODY_MAX {
        return error_response(413, "request too large");
    }
    let req = match serde_json::from_slice::<CreateFolderRequest>(body.as_ref()) {
        Ok(req) => req,
        Err(_) => return error_response(400, "bad json"),
    };
    let path = match normalize_fs_path(&req.path) {
        Ok(path) => path,
        Err(err) => return error_response(400, err),
    };
    match fs::create_dir_all(path.as_str()).await {
        Ok(()) => json_response(
            200,
            &serde_json::json!({
                "ok": true,
                "path": path,
            }),
        ),
        Err(err) => error_response(500, format!("create folder failed: {err}")),
    }
}

async fn handle_fs_store(body: Bytes) -> Response {
    if body.len() > TEXTEDITOR_BODY_MAX {
        return error_response(413, "request too large");
    }
    let req = match serde_json::from_slice::<StoreCopyRequest>(body.as_ref()) {
        Ok(req) => req,
        Err(_) => return error_response(400, "bad json"),
    };
    if let Err(err) = validate_blocks(&req.blocks) {
        return error_response(400, err);
    }
    let path = match normalize_fs_path(&req.path) {
        Ok(path) if path != "." => path,
        Ok(_) => return error_response(400, "choose a file path"),
        Err(err) => return error_response(400, err),
    };
    if let Some(parent) = parent_dir(&path) {
        if let Err(err) = fs::create_dir_all(parent.as_str()).await {
            return error_response(500, format!("create parent folder failed: {err}"));
        }
    }

    let bytes = match req.format.as_str() {
        "md" | "markdown" | "txt" | "text" => req.markdown.into_bytes(),
        "html" => req.html.into_bytes(),
        "json" | "blocknote" => match serde_json::to_vec_pretty(&req.blocks) {
            Ok(bytes) => bytes,
            Err(_) => return error_response(500, "json export failed"),
        },
        _ => return error_response(400, "bad format"),
    };

    match fs::write(path.as_str(), bytes.as_slice()).await {
        Ok(()) => json_response(
            200,
            &serde_json::json!({
                "ok": true,
                "path": path,
                "format": req.format,
                "bytes": bytes.len(),
            }),
        ),
        Err(err) => error_response(500, format!("write failed: {err}")),
    }
}

async fn handle_export(uri: Uri) -> Response {
    let document = load_document().await;
    let format = uri
        .query()
        .and_then(|query| {
            query
                .split('&')
                .find_map(|pair| pair.strip_prefix("format="))
        })
        .unwrap_or("json");

    match format {
        "md" | "markdown" => download_response(
            200,
            "text/markdown; charset=utf-8",
            "trueos-texteditor.md",
            document.exports.markdown.into_bytes(),
        ),
        "html" => download_response(
            200,
            "text/html; charset=utf-8",
            "trueos-texteditor.html",
            document.exports.html.into_bytes(),
        ),
        "txt" | "text" => download_response(
            200,
            "text/plain; charset=utf-8",
            "trueos-texteditor.txt",
            document.exports.markdown.into_bytes(),
        ),
        "json" | _ => match serde_json::to_vec(&document.blocks) {
            Ok(body) => download_response(
                200,
                "application/json; charset=utf-8",
                "trueos-texteditor.blocknote.json",
                body,
            ),
            Err(_) => error_response(500, "json export failed"),
        },
    }
}

fn router() -> Router {
    Router::new()
        .route("/", get(handle_index))
        .route("/index.html", get(handle_index))
        .route("/app.js", get(handle_app_js))
        .route("/app.css", get(handle_app_css))
        .route("/healthz", get(handle_status))
        .route("/api/healthz", get(handle_status))
        .route("/api/texteditor/status", get(handle_status))
        .route(
            "/api/texteditor/document",
            get(handle_document_get).post(handle_document_save),
        )
        .route("/api/texteditor/fs/list", get(handle_fs_list))
        .route(
            "/api/texteditor/fs/mkdir",
            get(handle_status).post(handle_fs_mkdir),
        )
        .route(
            "/api/texteditor/fs/store",
            get(handle_status).post(handle_fs_store),
        )
        .route("/api/texteditor/export", get(handle_export))
}

async fn texteditor_http_runtime() -> Result<(), io::Error> {
    let app = router();
    let addr = SocketAddr::from(([0, 0, 0, 0], TEXTEDITOR_HTTP_TCP_PORT));
    loop {
        let listener = match tokio::net::TcpListener::bind(addr).await {
            Ok(listener) => listener,
            Err(err) => {
                TEXTEDITOR_HTTP_PORT.store(0, Ordering::Release);
                logl::log(
                    level::WARN,
                    format_args!("texteditor-http: bind {} failed {}", addr, err),
                );
                time::sleep(Duration::from_millis(TEXTEDITOR_BIND_RETRY_MS)).await;
                continue;
            }
        };

        TEXTEDITOR_HTTP_PORT.store(addr.port(), Ordering::Release);
        logl::log(
            level::INFO,
            format_args!("texteditor-http: axum listening on http://{}/", addr),
        );
        let listener = listener.tap_io(|_| logl::log(level::INFO, "texteditor-http: tcp accepted"));
        let result = axum::serve(listener, app).await;
        TEXTEDITOR_HTTP_PORT.store(0, Ordering::Release);
        return result;
    }
}

fn main() {
    logl::log(level::INFO, "texteditor-http: blueprint start");
    let runtime = match runtime::current_thread_net().build() {
        Ok(runtime) => runtime,
        Err(err) => {
            logl::log(
                level::ERROR,
                format_args!("texteditor-http: runtime build failed {}", err),
            );
            return;
        }
    };
    let local = tokio::task::LocalSet::new();
    local.block_on(&runtime, async {
        if let Err(err) = texteditor_http_runtime().await {
            logl::log(
                level::ERROR,
                format_args!("texteditor-http: runtime failed {:?}", err),
            );
        }
    });
    platform::poll_once();
}
