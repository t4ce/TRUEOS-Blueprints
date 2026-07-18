// trueos-blueprint: features=["lifecycle-net"]

extern crate alloc;

use alloc::{
    format,
    string::{String, ToString},
    vec::Vec,
};
use core::sync::atomic::{AtomicU16, Ordering};

use axum::{
    Router,
    body::{Body, Bytes},
    http::{
        StatusCode, Uri,
        header::{CACHE_CONTROL, CONTENT_LENGTH, CONTENT_TYPE},
    },
    response::Response,
    routing::{get, post},
    serve::ListenerExt,
};
use serde::{Deserialize, Serialize};
use trueos::{
    clock, fs, logl,
    logl::level,
    platform::{self, io},
    runtime,
    tokio::{self, net::SocketAddr},
};

mod monaco_assets {
    include!(concat!(env!("OUT_DIR"), "/monaco_assets.rs"));
}

const MONACO_HTTP_TCP_PORT: u16 = 1011;
const MONACO_BODY_MAX: usize = 4 * 1024 * 1024;
const MONACO_DEFAULT_PATH: &str = "monaco/main.rs";
const MONACO_INDEX_HTML: &str = include_str!("index.html");
const MONACO_APP_JS: &str = include_str!("app.js");
const MONACO_APP_CSS: &str = include_str!("app.css");
const MONACO_STARTER: &str = "fn main() {\n    println!(\"hello from TRUEOS Monaco\");\n}\n";

static MONACO_HTTP_PORT: AtomicU16 = AtomicU16::new(0);

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct MonacoDocument {
    path: String,
    language: String,
    value: String,
    updated_at_s: u64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SaveDocumentRequest {
    path: String,
    language: String,
    value: String,
}

fn current_port() -> Option<u16> {
    match MONACO_HTTP_PORT.load(Ordering::Acquire) {
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

fn normalize_fs_path(path: &str) -> Result<String, String> {
    let path = path.trim().replace('\\', "/");
    if path.is_empty() {
        return Ok(MONACO_DEFAULT_PATH.to_string());
    }
    if path.starts_with('/')
        || path
            .split('/')
            .any(|part| part == ".." || part.contains('\0'))
    {
        return Err("bad path".to_string());
    }
    Ok(path.trim_matches('/').to_string())
}

fn parent_dir(path: &str) -> Option<String> {
    path.rsplit_once('/').and_then(|(parent, _)| {
        let parent = parent.trim();
        (!parent.is_empty()).then(|| parent.to_string())
    })
}

fn language_for_path(path: &str, requested: &str) -> String {
    let lower = path.to_ascii_lowercase();
    let language = if lower.ends_with(".rs") {
        "rust"
    } else if lower.ends_with(".ts") {
        "typescript"
    } else if lower.ends_with(".js") || lower.ends_with(".mjs") {
        "javascript"
    } else if lower.ends_with(".json") {
        "json"
    } else if lower.ends_with(".md") {
        "markdown"
    } else if lower.ends_with(".html") || lower.ends_with(".htm") {
        "html"
    } else if lower.ends_with(".css") {
        "css"
    } else if requested.trim().is_empty() {
        "plaintext"
    } else {
        requested.trim()
    };
    language.to_string()
}

fn query_value(uri: &Uri, name: &str) -> Option<String> {
    uri.query().and_then(|query| {
        query.split('&').find_map(|pair| {
            let (key, value) = pair.split_once('=')?;
            (key == name).then(|| percent_decode(value).ok()).flatten()
        })
    })
}

fn monaco_asset(path: &str) -> Option<&'static monaco_assets::StaticAsset> {
    monaco_assets::STATIC_ASSETS
        .iter()
        .find(|asset| asset.path == path)
}

async fn handle_index() -> Response {
    text_response(200, "text/html; charset=utf-8", MONACO_INDEX_HTML)
}

async fn handle_app_js() -> Response {
    text_response(200, "application/javascript; charset=utf-8", MONACO_APP_JS)
}

async fn handle_app_css() -> Response {
    text_response(200, "text/css; charset=utf-8", MONACO_APP_CSS)
}

async fn handle_monaco_asset(uri: Uri) -> Response {
    let Some(path) = uri.path().strip_prefix("/monaco/vs/") else {
        return error_response(404, "asset not found");
    };
    match monaco_asset(path) {
        Some(asset) => bytes_response(200, asset.mime, asset.bytes, false),
        None => error_response(404, "asset not found"),
    }
}

async fn handle_status() -> Response {
    json_response(
        200,
        &serde_json::json!({
            "ok": true,
            "service": "monaco-http",
            "port": current_port(),
            "defaultPath": MONACO_DEFAULT_PATH,
            "assetCount": monaco_assets::STATIC_ASSETS.len(),
            "generatedAtS": clock::ntp_current_unix_seconds(),
        }),
    )
}

async fn handle_document_get(uri: Uri) -> Response {
    let raw_path = query_value(&uri, "path").unwrap_or_else(|| MONACO_DEFAULT_PATH.to_string());
    let path = match normalize_fs_path(&raw_path) {
        Ok(path) => path,
        Err(err) => return error_response(400, err),
    };
    let bytes = fs::read(path.as_str())
        .await
        .unwrap_or_else(|_| MONACO_STARTER.as_bytes().to_vec());
    let value = match String::from_utf8(bytes) {
        Ok(value) => value,
        Err(_) => return error_response(415, "file is not utf8 text"),
    };
    let language = language_for_path(&path, "");
    json_response(
        200,
        &serde_json::json!({
            "ok": true,
            "document": MonacoDocument {
                path,
                language,
                value,
                updated_at_s: 0,
            },
        }),
    )
}

async fn handle_document_save(body: Bytes) -> Response {
    if body.len() > MONACO_BODY_MAX {
        return error_response(413, "request too large");
    }
    let req = match serde_json::from_slice::<SaveDocumentRequest>(body.as_ref()) {
        Ok(req) => req,
        Err(_) => return error_response(400, "bad json"),
    };
    let path = match normalize_fs_path(&req.path) {
        Ok(path) => path,
        Err(err) => return error_response(400, err),
    };
    if let Some(parent) = parent_dir(&path) {
        if let Err(err) = fs::create_dir_all(parent.as_str()).await {
            return error_response(500, format!("create parent folder failed: {err}"));
        }
    }
    let bytes = req.value.as_bytes();
    if let Err(err) = fs::write(path.as_str(), bytes).await {
        return error_response(500, format!("write failed: {err}"));
    }
    let language = language_for_path(&path, &req.language);
    let updated_at_s = clock::ntp_current_unix_seconds();
    json_response(
        200,
        &serde_json::json!({
            "ok": true,
            "bytes": bytes.len(),
            "document": MonacoDocument {
                path,
                language,
                value: req.value,
                updated_at_s,
            },
        }),
    )
}

fn router() -> Router {
    Router::new()
        .route("/", get(handle_index))
        .route("/index.html", get(handle_index))
        .route("/app.js", get(handle_app_js))
        .route("/app.css", get(handle_app_css))
        .route("/monaco/vs/{*asset}", get(handle_monaco_asset))
        .route("/healthz", get(handle_status))
        .route("/api/healthz", get(handle_status))
        .route("/api/monaco/status", get(handle_status))
        .route(
            "/api/monaco/document",
            get(handle_document_get).post(handle_document_save),
        )
        .route("/api/monaco/save", post(handle_document_save))
}

async fn monaco_http_runtime() -> Result<(), io::Error> {
    let app = router();
    let addr = SocketAddr::from(([0, 0, 0, 0], MONACO_HTTP_TCP_PORT));
    let listener = trueos::lifecycle_axum_listener!("monaco-http", addr, &MONACO_HTTP_PORT).await;
    let listener = listener.tap_io(|_| logl::log(level::INFO, "monaco-http: tcp accepted"));
    axum::serve(listener, app).await
}

fn main() {
    logl::log(level::INFO, "monaco-http: blueprint start");
    let runtime = match runtime::current_thread_net().build() {
        Ok(runtime) => runtime,
        Err(err) => {
            logl::log(
                level::ERROR,
                format_args!("monaco-http: runtime build failed {}", err),
            );
            return;
        }
    };
    let local = tokio::task::LocalSet::new();
    local.block_on(&runtime, async {
        if let Err(err) = monaco_http_runtime().await {
            logl::log(
                level::ERROR,
                format_args!("monaco-http: runtime failed {:?}", err),
            );
        }
    });
    platform::poll_once();
}
