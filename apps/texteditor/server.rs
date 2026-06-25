// trueos-blueprint: features=["tokio-net-probe"]

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
