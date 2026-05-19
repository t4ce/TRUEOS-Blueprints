// trueos-blueprint: features=["tokio-net-probe"]

extern crate alloc;

use alloc::{string::ToString, vec::Vec};
use core::sync::atomic::{AtomicU16, Ordering};

use axum::{
    Router,
    body::Body,
    http::{
        StatusCode,
        header::{CACHE_CONTROL, CONTENT_LENGTH, CONTENT_TYPE},
    },
    response::Response,
    routing::get,
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

const WEBDEVICES_HTTP_TCP_PORT: u16 = 10;
const WEBDEVICES_BIND_RETRY_MS: u64 = 1000;
const WEBDEVICES_INDEX_HTML: &str = include_str!("index.html");
const TRUEOS_TAILWIND_CSS: &str = include_str!("tailwind.css");

static WEBDEVICES_HTTP_PORT: AtomicU16 = AtomicU16::new(0);

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct HardwareSnapshot {
    schema: &'static str,
    generated_at_s: u64,
    service: ServiceSnapshot,
    pci: DeviceGroup,
    usb: DeviceGroup,
    note: &'static str,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ServiceSnapshot {
    name: &'static str,
    port: Option<u16>,
    bind: &'static str,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct DeviceGroup {
    count: usize,
    devices: Vec<serde_json::Value>,
    unavailable_reason: &'static str,
}

fn current_port() -> Option<u16> {
    match WEBDEVICES_HTTP_PORT.load(Ordering::Acquire) {
        0 => None,
        port => Some(port),
    }
}

fn status_code(status: u16) -> StatusCode {
    StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR)
}

fn response(status: u16, content_type: &'static str, body: Vec<u8>, no_store: bool) -> Response {
    let mut builder = Response::builder()
        .status(status_code(status))
        .header(CONTENT_TYPE, content_type)
        .header(CONTENT_LENGTH, body.len().to_string());
    builder = if no_store {
        builder.header(CACHE_CONTROL, "no-store")
    } else {
        builder.header(CACHE_CONTROL, "no-cache")
    };
    builder
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

async fn handle_index() -> Response {
    logl::log(level::INFO, "webdevices-http: GET /");
    text_response(200, "text/html; charset=utf-8", WEBDEVICES_INDEX_HTML)
}

async fn handle_tailwind_css() -> Response {
    text_response(200, "text/css; charset=utf-8", TRUEOS_TAILWIND_CSS)
}

async fn handle_healthz() -> Response {
    json_response(
        200,
        &serde_json::json!({
            "ok": true,
            "service": "webdevices-http",
            "port": current_port(),
        }),
    )
}

async fn handle_snapshot() -> Response {
    json_response(
        200,
        &HardwareSnapshot {
            schema: "trueos.webdevices.v1",
            generated_at_s: clock::ntp_current_unix_seconds(),
            service: ServiceSnapshot {
                name: "webdevices-http",
                port: current_port(),
                bind: "0.0.0.0",
            },
            pci: DeviceGroup {
                count: 0,
                devices: Vec::new(),
                unavailable_reason: "PCI inventory is a kernel service; no guest-safe blueprint facade is exposed yet",
            },
            usb: DeviceGroup {
                count: 0,
                devices: Vec::new(),
                unavailable_reason: "USB topology is a kernel service; no guest-safe blueprint facade is exposed yet",
            },
            note: "webdevices is running as a blueprint axum app with kernel-only inventory intentionally isolated",
        },
    )
}

fn router() -> Router {
    Router::new()
        .route("/", get(handle_index))
        .route("/index.html", get(handle_index))
        .route("/tailwind.css", get(handle_tailwind_css))
        .route("/healthz", get(handle_healthz))
        .route("/api/healthz", get(handle_healthz))
        .route("/api/webdevices/snapshot", get(handle_snapshot))
        .route("/api/devices/snapshot", get(handle_snapshot))
}

async fn webdevices_http_runtime() -> Result<(), io::Error> {
    let app = router();
    let addr = SocketAddr::from(([0, 0, 0, 0], WEBDEVICES_HTTP_TCP_PORT));
    loop {
        let listener = match tokio::net::TcpListener::bind(addr).await {
            Ok(listener) => listener,
            Err(err) => {
                WEBDEVICES_HTTP_PORT.store(0, Ordering::Release);
                logl::log(
                    level::WARN,
                    format_args!("webdevices-http: bind {} failed {}", addr, err),
                );
                time::sleep(Duration::from_millis(WEBDEVICES_BIND_RETRY_MS)).await;
                continue;
            }
        };

        WEBDEVICES_HTTP_PORT.store(addr.port(), Ordering::Release);
        logl::log(
            level::INFO,
            format_args!("webdevices-http: axum listening on http://{}/", addr),
        );
        let listener = listener.tap_io(|_| logl::log(level::INFO, "webdevices-http: tcp accepted"));
        let result = axum::serve(listener, app).await;
        WEBDEVICES_HTTP_PORT.store(0, Ordering::Release);
        return result;
    }
}

fn main() {
    logl::log(level::INFO, "webdevices-http: blueprint start");
    let runtime = match runtime::current_thread_net().build() {
        Ok(runtime) => runtime,
        Err(err) => {
            logl::log(
                level::ERROR,
                format_args!("webdevices-http: runtime build failed {}", err),
            );
            return;
        }
    };
    let local = tokio::task::LocalSet::new();
    local.block_on(&runtime, async {
        if let Err(err) = webdevices_http_runtime().await {
            logl::log(
                level::ERROR,
                format_args!("webdevices-http: runtime failed {:?}", err),
            );
        }
    });
    platform::poll_once();
}
