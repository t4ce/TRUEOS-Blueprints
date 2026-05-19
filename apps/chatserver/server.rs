// trueos-blueprint: features=["tokio-net-probe"]

extern crate alloc;

use alloc::{string::ToString, vec::Vec};
use core::sync::atomic::{AtomicBool, AtomicU16, Ordering};

use axum::{
    Router,
    body::{Body, to_bytes},
    extract::Request,
    http::{
        Method, StatusCode,
        header::{CACHE_CONTROL, CONTENT_LENGTH, CONTENT_TYPE},
    },
    response::Response,
    routing::{any, get},
    serve::ListenerExt,
};
use trueos::{
    clock, logl,
    logl::level,
    platform::{self, io},
    runtime,
    time::{self, Duration},
    tokio::{
        self,
        net::SocketAddr,
        sync::{Mutex, MutexGuard},
    },
    vfs,
};
use trueos_chat::{ChatConfig, ChatHub, ChatMethod, ChatRequest, ChatResponse};

const CHAT_HTTP_TCP_PORT: u16 = 3;
const CHAT_HTTP_BODY_MAX: usize = 64 * 1024;
const CHAT_HTTP_BIND_RETRY_MS: u64 = 100;
const CHAT_SAVE_BATCH_MS: u64 = 10_000;
const CHAT_SAVE_IDLE_MS: u64 = 1000;
const CHAT_STORE_DIR: &str = "chat";
const CHAT_STORE_PATH: &str = "chat/rooms.json";
const TRUEOS_TAILWIND_CSS: &str = include_str!("tailwind.css");

static CHAT_HUB: Mutex<Option<ChatHub>> = Mutex::const_new(None);
static CHAT_HUB_LOADED: AtomicBool = AtomicBool::new(false);
static CHAT_SAVE_REQUESTED: AtomicBool = AtomicBool::new(false);
static CHAT_STORE_DIR_READY: AtomicBool = AtomicBool::new(false);
static CHAT_HTTP_PORT: AtomicU16 = AtomicU16::new(0);

fn current_port() -> Option<u16> {
    match CHAT_HTTP_PORT.load(Ordering::Acquire) {
        0 => None,
        port => Some(port),
    }
}

async fn lock_hub() -> MutexGuard<'static, Option<ChatHub>> {
    CHAT_HUB.lock().await
}

fn status_code(status: u16) -> StatusCode {
    StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR)
}

fn now_ms() -> u64 {
    let unix = clock::ntp_current_unix_seconds();
    if unix != 0 {
        return unix.saturating_mul(1000);
    }
    0
}

async fn load_chat_hub_once() {
    if CHAT_HUB_LOADED.load(Ordering::Acquire) {
        return;
    }
    {
        let guard = lock_hub().await;
        if guard.is_some() {
            CHAT_HUB_LOADED.store(true, Ordering::Release);
            return;
        }
    }

    let bytes = match vfs::read_file(CHAT_STORE_PATH.as_bytes()) {
        Ok(bytes) => bytes,
        Err(_) => {
            CHAT_HUB_LOADED.store(true, Ordering::Release);
            return;
        }
    };

    match ChatHub::from_json_bytes(ChatConfig::default(), bytes.as_slice()) {
        Ok(hub) => {
            let room_count = hub.room_count();
            let mut guard = lock_hub().await;
            if guard.is_none() {
                *guard = Some(hub);
                logl::log(
                    level::INFO,
                    format_args!(
                        "chat: loaded {} room(s) from {}",
                        room_count, CHAT_STORE_PATH
                    ),
                );
            }
            CHAT_HUB_LOADED.store(true, Ordering::Release);
        }
        Err(()) => {
            logl::log(
                level::WARN,
                format_args!("chat: ignored invalid {}", CHAT_STORE_PATH),
            );
            CHAT_HUB_LOADED.store(true, Ordering::Release);
        }
    }
}

async fn chat_hub_snapshot_bytes() -> Option<Vec<u8>> {
    let guard = lock_hub().await;
    guard.as_ref().map(ChatHub::to_json_bytes)
}

async fn save_chat_hub_snapshot() {
    let Some(bytes) = chat_hub_snapshot_bytes().await else {
        return;
    };
    if !CHAT_STORE_DIR_READY.load(Ordering::Acquire) {
        match vfs::create_dir_all(CHAT_STORE_DIR.as_bytes()) {
            Ok(()) => CHAT_STORE_DIR_READY.store(true, Ordering::Release),
            Err(err) => {
                logl::log(
                    level::WARN,
                    format_args!("chat: create {} failed rc={}", CHAT_STORE_DIR, err),
                );
                return;
            }
        }
    }
    if let Err(err) = vfs::write_file(CHAT_STORE_PATH.as_bytes(), bytes.as_slice()) {
        logl::log(
            level::WARN,
            format_args!("chat: save {} failed rc={}", CHAT_STORE_PATH, err),
        );
    }
}

fn request_chat_hub_save(reason: &'static str) {
    let was_pending = CHAT_SAVE_REQUESTED.swap(true, Ordering::AcqRel);
    if !was_pending {
        logl::log(
            level::INFO,
            format_args!("chat: save requested reason={} mode=deferred", reason),
        );
    }
}

async fn chat_hub_save_loop() {
    loop {
        if !CHAT_SAVE_REQUESTED.swap(false, Ordering::AcqRel) {
            time::sleep(Duration::from_millis(CHAT_SAVE_IDLE_MS)).await;
            continue;
        }

        time::sleep(Duration::from_millis(CHAT_SAVE_BATCH_MS)).await;
        let coalesced = CHAT_SAVE_REQUESTED.swap(false, Ordering::AcqRel);
        logl::log(level::INFO, "chat: save begin mode=batched");
        save_chat_hub_snapshot().await;
        logl::log(
            level::INFO,
            format_args!(
                "chat: save done mode=batched coalesced_requests={}",
                coalesced
            ),
        );
    }
}

fn chat_response(response: ChatResponse) -> Response {
    let no_cache = response.content_type.starts_with("application/json");
    let mut builder = Response::builder()
        .status(status_code(response.status))
        .header(CONTENT_TYPE, response.content_type)
        .header(CONTENT_LENGTH, response.body.len().to_string());
    if no_cache {
        builder = builder.header(CACHE_CONTROL, "no-store");
    }
    builder
        .body(Body::from(response.body))
        .unwrap_or_else(|_| Response::new(Body::empty()))
}

fn request_too_large_response() -> Response {
    let body = b"{\"ok\":false,\"error\":\"request too large\"}".to_vec();
    Response::builder()
        .status(StatusCode::PAYLOAD_TOO_LARGE)
        .header(CONTENT_TYPE, "application/json; charset=utf-8")
        .header(CONTENT_LENGTH, body.len().to_string())
        .header(CACHE_CONTROL, "no-store")
        .body(Body::from(body))
        .unwrap_or_else(|_| Response::new(Body::empty()))
}

async fn handle_tailwind_css() -> Response {
    let body = TRUEOS_TAILWIND_CSS.as_bytes().to_vec();
    Response::builder()
        .status(StatusCode::OK)
        .header(CONTENT_TYPE, "text/css; charset=utf-8")
        .header(CONTENT_LENGTH, body.len().to_string())
        .header(CACHE_CONTROL, "no-cache")
        .body(Body::from(body))
        .unwrap_or_else(|_| Response::new(Body::empty()))
}

async fn handle_chat_request(request: Request) -> Response {
    load_chat_hub_once().await;
    let (parts, body) = request.into_parts();
    let method = match parts.method {
        Method::GET => ChatMethod::Get,
        Method::POST => ChatMethod::Post,
        _ => ChatMethod::Other,
    };
    let path = parts.uri.path().to_string();
    let query = parts.uri.query().map(|query| query.to_string());
    let body = match to_bytes(body, CHAT_HTTP_BODY_MAX).await {
        Ok(body) => body.to_vec(),
        Err(_) => return request_too_large_response(),
    };

    let response = {
        let mut guard = lock_hub().await;
        let hub = guard.get_or_insert_with(|| ChatHub::new(ChatConfig::default()));
        hub.handle(ChatRequest {
            method,
            path,
            query,
            body,
            now_ms: now_ms(),
        })
    };
    if method == ChatMethod::Post && response.status == 200 {
        request_chat_hub_save("http-post");
    }
    chat_response(response)
}

fn chat_router() -> Router {
    Router::new()
        .route("/", any(handle_chat_request))
        .route("/tailwind.css", get(handle_tailwind_css))
        .route("/api", any(handle_chat_request))
        .route("/api/rooms", any(handle_chat_request))
        .route("/api/rooms/{room}/messages", any(handle_chat_request))
        .fallback(handle_chat_request)
}

async fn chat_http_runtime() -> Result<(), io::Error> {
    logl::log(level::INFO, "chat-http: runtime async enter");
    load_chat_hub_once().await;

    let app = chat_router();
    let addr = SocketAddr::from(([0, 0, 0, 0], CHAT_HTTP_TCP_PORT));
    loop {
        logl::log(
            level::INFO,
            format_args!("chat-http: bind begin addr={}", addr),
        );
        let listener = match tokio::net::TcpListener::bind(addr).await {
            Ok(listener) => listener,
            Err(err) => {
                CHAT_HTTP_PORT.store(0, Ordering::Release);
                logl::log(
                    level::WARN,
                    format_args!(
                        "chat-http: bind {} failed kind={:?} err={}",
                        addr,
                        err.kind(),
                        err
                    ),
                );
                time::sleep(Duration::from_millis(CHAT_HTTP_BIND_RETRY_MS)).await;
                continue;
            }
        };

        CHAT_HTTP_PORT.store(addr.port(), Ordering::Release);
        logl::log(
            level::INFO,
            format_args!("chat-http: axum listening on http://{}/", addr),
        );
        let listener = listener.tap_io(|_| logl::log(level::INFO, "chat-http: tcp accepted"));
        if let Err(err) = axum::serve(listener, app.clone()).await {
            CHAT_HTTP_PORT.store(0, Ordering::Release);
            logl::log(
                level::WARN,
                format_args!(
                    "chat-http: serve failed port={} kind={:?} err={}",
                    addr.port(),
                    err.kind(),
                    err
                ),
            );
            time::sleep(Duration::from_millis(1000)).await;
        }
    }
}

fn main() {
    logl::log(level::INFO, "chat-http: blueprint start");
    let runtime = match runtime::current_thread_net().build() {
        Ok(runtime) => runtime,
        Err(err) => {
            logl::log(
                level::ERROR,
                format_args!("chat-http: runtime build failed {}", err),
            );
            return;
        }
    };
    let local = tokio::task::LocalSet::new();
    local.block_on(&runtime, async {
        tokio::task::spawn_local(chat_hub_save_loop());
        if let Err(err) = chat_http_runtime().await {
            logl::log(
                level::ERROR,
                format_args!("chat-http: runtime failed {:?}", err),
            );
        }
    });
    CHAT_HTTP_PORT.store(0, Ordering::Release);
    logl::log(
        level::INFO,
        format_args!("chat-http: blueprint stop port={:?}", current_port()),
    );
    platform::poll_once();
}
