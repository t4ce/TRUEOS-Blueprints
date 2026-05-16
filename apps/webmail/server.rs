// trueos-blueprint: features=["tokio-net-probe"]

extern crate alloc;

use alloc::{string::ToString, vec::Vec};
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
use serde::Serialize;
use trueos::{
    clock, logl,
    logl::level,
    platform::{self, io},
    runtime,
    time::{self, Duration},
    tokio::{self, net::SocketAddr},
};

const WEBMAIL_HTTP_TCP_PORT: u16 = 4;
const WEBMAIL_BIND_RETRY_MS: u64 = 1000;
const WEBMAIL_INDEX_HTML: &str = include_str!("index.html");
const WEBMAIL_APP_JS: &str = include_str!("app.js");
const TRUEOS_TAILWIND_CSS: &str = include_str!("tailwind.css");

static WEBMAIL_HTTP_PORT: AtomicU16 = AtomicU16::new(0);

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct WebmailStatus {
    ok: bool,
    service: &'static str,
    port: Option<u16>,
    generated_at_s: u64,
    inbox_state: &'static str,
    send_state: &'static str,
    note: &'static str,
}

fn current_port() -> Option<u16> {
    match WEBMAIL_HTTP_PORT.load(Ordering::Acquire) {
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
        .header(CACHE_CONTROL, if no_store { "no-store" } else { "no-cache" })
        .body(Body::from(body))
        .unwrap_or_else(|_| Response::new(Body::empty()))
}

fn text_response(status: u16, content_type: &'static str, body: &str) -> Response {
    response(status, content_type, body.as_bytes().to_vec(), false)
}

fn json_response<T: Serialize>(status: u16, value: &T) -> Response {
    match serde_json::to_vec(value) {
        Ok(body) => response(status, "application/json; charset=utf-8", body, true),
        Err(_) => text_response(500, "text/plain; charset=utf-8", "json serialization failed\n"),
    }
}

async fn handle_index() -> Response {
    text_response(200, "text/html; charset=utf-8", WEBMAIL_INDEX_HTML)
}

async fn handle_app_js() -> Response {
    text_response(200, "application/javascript; charset=utf-8", WEBMAIL_APP_JS)
}

async fn handle_tailwind_css() -> Response {
    text_response(200, "text/css; charset=utf-8", TRUEOS_TAILWIND_CSS)
}

async fn handle_status() -> Response {
    json_response(
        200,
        &WebmailStatus {
            ok: true,
            service: "webmail-http",
            port: current_port(),
            generated_at_s: clock::ntp_current_unix_seconds(),
            inbox_state: "unavailable",
            send_state: "unavailable",
            note: "POP3/SMTP remain kernel services; this blueprint serves the webmail UI and stable API shape",
        },
    )
}

async fn handle_list() -> Response {
    json_response(
        200,
        &serde_json::json!({
            "ok": true,
            "messages": [],
            "note": "mailbox backend unavailable in blueprint mode"
        }),
    )
}

async fn handle_read(uri: Uri) -> Response {
    let id = uri
        .query()
        .and_then(|query| query.split('&').find_map(|pair| pair.strip_prefix("id=")))
        .unwrap_or("");
    json_response(
        404,
        &serde_json::json!({
            "ok": false,
            "id": id,
            "error": "message not available in blueprint mode"
        }),
    )
}

async fn handle_refresh() -> Response {
    json_response(
        202,
        &serde_json::json!({
            "ok": false,
            "queued": false,
            "error": "POP3 refresh is not exposed to blueprints yet"
        }),
    )
}

async fn handle_send(body: Bytes) -> Response {
    json_response(
        202,
        &serde_json::json!({
            "ok": false,
            "queued": false,
            "bytes": body.len(),
            "error": "SMTP send is not exposed to blueprints yet"
        }),
    )
}

fn router() -> Router {
    Router::new()
        .route("/", get(handle_index))
        .route("/index.html", get(handle_index))
        .route("/app.js", get(handle_app_js))
        .route("/tailwind.css", get(handle_tailwind_css))
        .route("/healthz", get(handle_status))
        .route("/api/healthz", get(handle_status))
        .route("/api/webmail/status", get(handle_status))
        .route("/api/webmail/refresh", get(handle_refresh).post(handle_refresh))
        .route("/api/webmail/list", get(handle_list))
        .route("/api/webmail/read", get(handle_read))
        .route("/api/webmail/send", post(handle_send))
}

async fn webmail_http_runtime() -> Result<(), io::Error> {
    let app = router();
    let addr = SocketAddr::from(([0, 0, 0, 0], WEBMAIL_HTTP_TCP_PORT));
    loop {
        let listener = match tokio::net::TcpListener::bind(addr).await {
            Ok(listener) => listener,
            Err(err) => {
                WEBMAIL_HTTP_PORT.store(0, Ordering::Release);
                logl::log(level::WARN, format_args!("webmail-http: bind {} failed {}", addr, err));
                time::sleep(Duration::from_millis(WEBMAIL_BIND_RETRY_MS)).await;
                continue;
            }
        };

        WEBMAIL_HTTP_PORT.store(addr.port(), Ordering::Release);
        logl::log(level::INFO, format_args!("webmail-http: axum listening on http://{}/", addr));
        let listener = listener.tap_io(|_| logl::log(level::INFO, "webmail-http: tcp accepted"));
        let result = axum::serve(listener, app).await;
        WEBMAIL_HTTP_PORT.store(0, Ordering::Release);
        return result;
    }
}

fn main() {
    logl::log(level::INFO, "webmail-http: blueprint start");
    let runtime = match runtime::current_thread_net().build() {
        Ok(runtime) => runtime,
        Err(err) => {
            logl::log(level::ERROR, format_args!("webmail-http: runtime build failed {}", err));
            return;
        }
    };
    let local = tokio::task::LocalSet::new();
    local.block_on(&runtime, async {
        if let Err(err) = webmail_http_runtime().await {
            logl::log(level::ERROR, format_args!("webmail-http: runtime failed {:?}", err));
        }
    });
    platform::poll_once();
}
