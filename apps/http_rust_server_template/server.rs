// trueos-blueprint: features=["tokio-net-probe"]

extern crate alloc;

use alloc::string::ToString;
use core::sync::atomic::{AtomicU16, Ordering};

use axum::{
    body::Body,
    http::{
        header::{CACHE_CONTROL, CONTENT_LENGTH, CONTENT_TYPE},
        StatusCode,
    },
    response::Response,
    routing::get,
    serve::ListenerExt,
    Router,
};
use trueos::{
    logl,
    logl::level,
    platform::{self, io},
    runtime,
    time::{self, Duration},
    tokio::{self, net::SocketAddr},
};

const HTTP_TCP_PORT: u16 = 3;
const BIND_RETRY_MS: u64 = 100;

static HTTP_PORT: AtomicU16 = AtomicU16::new(0);

fn text_response(status: StatusCode, content_type: &'static str, body: &'static str) -> Response {
    Response::builder()
        .status(status)
        .header(CONTENT_TYPE, content_type)
        .header(CONTENT_LENGTH, body.len().to_string())
        .header(CACHE_CONTROL, "no-store")
        .body(Body::from(body))
        .unwrap_or_else(|_| Response::new(Body::empty()))
}

async fn handle_index() -> Response {
    text_response(
        StatusCode::OK,
        "text/html; charset=utf-8",
        "<!doctype html><html><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\"><title>TRUEOS HTTP</title></head><body><main><h1>Hello from TRUEOS</h1><p>Axum blueprint server is running.</p></main></body></html>",
    )
}

async fn handle_healthz() -> Response {
    text_response(StatusCode::OK, "text/plain; charset=utf-8", "ok\n")
}

fn router() -> Router {
    Router::new()
        .route("/", get(handle_index))
        .route("/healthz", get(handle_healthz))
}

async fn http_runtime() -> Result<(), io::Error> {
    let app = router();
    let addr = SocketAddr::from(([0, 0, 0, 0], HTTP_TCP_PORT));
    loop {
        logl::log(level::INFO, format_args!("http-template: bind begin addr={}", addr));
        let listener = match tokio::net::TcpListener::bind(addr).await {
            Ok(listener) => listener,
            Err(err) => {
                HTTP_PORT.store(0, Ordering::Release);
                logl::log(level::WARN, format_args!("http-template: bind {} failed {}", addr, err));
                time::sleep(Duration::from_millis(BIND_RETRY_MS)).await;
                continue;
            }
        };

        HTTP_PORT.store(addr.port(), Ordering::Release);
        logl::log(level::INFO, format_args!("http-template: axum listening on http://{}/", addr));
        let listener = listener.tap_io(|_| logl::log(level::INFO, "http-template: tcp accepted"));
        let result = axum::serve(listener, app).await;
        HTTP_PORT.store(0, Ordering::Release);
        return result;
    }
}

fn main() {
    logl::log(level::INFO, "http-template: blueprint start");
    let runtime = match runtime::current_thread_net().build() {
        Ok(runtime) => runtime,
        Err(err) => {
            logl::log(level::ERROR, format_args!("http-template: runtime build failed {}", err));
            return;
        }
    };

    let local = tokio::task::LocalSet::new();
    local.block_on(&runtime, async {
        if let Err(err) = http_runtime().await {
            logl::log(level::ERROR, format_args!("http-template: runtime failed {:?}", err));
        }
    });
    platform::poll_once();
}
