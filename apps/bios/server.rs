// trueos-blueprint: features=["lifecycle-net"]

extern crate alloc;

use alloc::{string::ToString, vec::Vec};
use core::{
    net::SocketAddr,
    sync::atomic::{AtomicU16, Ordering},
};
use axum::{
    Router,
    body::Body,
    http::header::{CACHE_CONTROL, CONTENT_LENGTH, CONTENT_TYPE},
    response::Response,
    routing::get,
};
use trueos::{logl, logl::level, platform, runtime, tokio};

const PORT: u16 = 1012;
const INDEX: &str = include_str!("index.html");
const JS: &str = include_str!("app.js");
const CSS: &str = include_str!("app.css");
static PUBLISHED_PORT: AtomicU16 = AtomicU16::new(0);

fn response(content_type: &'static str, body: Vec<u8>, no_store: bool) -> Response {
    Response::builder()
        .header(CONTENT_TYPE, content_type)
        .header(CONTENT_LENGTH, body.len().to_string())
        .header(CACHE_CONTROL, if no_store { "no-store" } else { "no-cache, max-age=0" })
        .header(
            "content-security-policy",
            "default-src 'self'; script-src 'self'; style-src 'self'; connect-src 'self'; object-src 'none'; base-uri 'none'; frame-ancestors 'none'",
        )
        .header("x-content-type-options", "nosniff")
        .body(Body::from(body))
        .unwrap_or_else(|_| Response::new(Body::empty()))
}

fn static_text(content_type: &'static str, text: &'static str) -> Response {
    response(content_type, text.as_bytes().to_vec(), false)
}

async fn index() -> Response {
    static_text("text/html; charset=utf-8", INDEX)
}

async fn app_js() -> Response {
    static_text("application/javascript; charset=utf-8", JS)
}

async fn app_css() -> Response {
    static_text("text/css; charset=utf-8", CSS)
}

async fn schema() -> Response {
    match v::vbios::snapshot_bytes() {
        Ok(bytes) => response("application/json; charset=utf-8", bytes, true),
        Err(code) => response(
            "application/json; charset=utf-8",
            alloc::format!(
                "{{\"ok\":false,\"readOnly\":true,\"activeWritePath\":\"none\",\"errorCode\":{code}}}"
            )
            .into_bytes(),
            true,
        ),
    }
}

async fn health() -> Response {
    let port = PUBLISHED_PORT.load(Ordering::Acquire);
    response(
        "application/json; charset=utf-8",
        alloc::format!(
            "{{\"ok\":true,\"service\":\"bios\",\"listen\":\"0.0.0.0\",\"port\":{port},\"readOnly\":true,\"activeWritePath\":\"none\"}}"
        )
        .into_bytes(),
        true,
    )
}

fn router() -> Router {
    Router::new()
        .route("/", get(index))
        .route("/index.html", get(index))
        .route("/app.js", get(app_js))
        .route("/app.css", get(app_css))
        .route("/healthz", get(health))
        .route("/api/healthz", get(health))
        .route("/api/bios/schema", get(schema))
}

async fn serve() -> Result<(), trueos::platform::io::Error> {
    let addr = SocketAddr::from(([0, 0, 0, 0], PORT));
    let listener = trueos::lifecycle_axum_listener!("bios", addr, &PUBLISHED_PORT).await;
    logl::log(
        level::INFO,
        format_args!(
            "bios: http://0.0.0.0:{}/ (read-only)",
            PUBLISHED_PORT.load(Ordering::Acquire)
        ),
    );
    axum::serve(listener, router()).await
}

fn main() {
    let runtime = match runtime::current_thread_net().build() {
        Ok(runtime) => runtime,
        Err(err) => {
            logl::log(level::ERROR, format_args!("bios: runtime build failed {err}"));
            return;
        }
    };
    let local = tokio::task::LocalSet::new();
    local.block_on(&runtime, async {
        if let Err(err) = serve().await {
            logl::log(level::ERROR, format_args!("bios: server failed {err:?}"));
        }
    });
    platform::poll_once();
}
