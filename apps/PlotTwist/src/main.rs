// trueos-blueprint: features=["lifecycle-net"]

#[cfg(target_os = "trueos")]
mod server {
    extern crate alloc;

    use alloc::{string::ToString, sync::Arc, vec::Vec};
    use core::{
        pin::Pin,
        sync::atomic::{AtomicU16, AtomicUsize, Ordering},
        task::{Context, Poll},
    };
    use std::net::SocketAddr;

    use axum::{
        Json, Router,
        body::Body,
        extract::{Path, State},
        http::{
            StatusCode,
            header::{CACHE_CONTROL, CONTENT_LENGTH, CONTENT_TYPE},
        },
        response::Response,
        routing::{get, post},
    };
    use hyper::service::service_fn;
    use plot_twist::{Action, ConnectRequest, GameError, PlotTwist};
    use serde::Deserialize;
    use serde_json::{Value, json};
    use tower::Service;
    use trueos::{
        logl,
        logl::level,
        platform::{self, io},
        runtime,
        time::Instant,
        tokio::{
            self,
            io::{AsyncRead, AsyncWrite, ReadBuf},
            sync::Mutex,
        },
    };

    const HTTP_PORT: u16 = 8338;
    const INDEX_HTML: &str = include_str!("../web/index.html");
    const APP_CSS: &str = include_str!("../web/app.css");
    const APP_JS: &str = include_str!("../web/app.js");
    const EMOJI: [(&str, &str); 5] = [
        ("1f600", include_str!("../web/emoji/1f600.svg")),
        ("1f60e", include_str!("../web/emoji/1f60e.svg")),
        ("1f929", include_str!("../web/emoji/1f929.svg")),
        ("1f914", include_str!("../web/emoji/1f914.svg")),
        ("1f920", include_str!("../web/emoji/1f920.svg")),
    ];
    static BOUND_PORT: AtomicU16 = AtomicU16::new(0);
    static CONNECTION_COUNT: AtomicUsize = AtomicUsize::new(0);

    struct HyperIo<T>(T);

    fn hyper_io_error(_: io::Error) -> hyper::io::Error {
        hyper::io::Error::new(hyper::io::ErrorKind::Other, "tokio io error")
    }

    impl<T: AsyncRead + Unpin> hyper::rt::Read for HyperIo<T> {
        fn poll_read(
            mut self: Pin<&mut Self>,
            cx: &mut Context<'_>,
            mut output: hyper::rt::ReadBufCursor<'_>,
        ) -> Poll<Result<(), hyper::io::Error>> {
            let read = unsafe {
                let mut input = ReadBuf::uninit(output.as_mut());
                match Pin::new(&mut self.0).poll_read(cx, &mut input) {
                    Poll::Ready(Ok(())) => input.filled().len(),
                    Poll::Ready(Err(err)) => return Poll::Ready(Err(hyper_io_error(err))),
                    Poll::Pending => return Poll::Pending,
                }
            };
            unsafe { output.advance(read) };
            Poll::Ready(Ok(()))
        }
    }

    impl<T: AsyncWrite + Unpin> hyper::rt::Write for HyperIo<T> {
        fn poll_write(
            mut self: Pin<&mut Self>,
            cx: &mut Context<'_>,
            bytes: &[u8],
        ) -> Poll<Result<usize, hyper::io::Error>> {
            Pin::new(&mut self.0)
                .poll_write(cx, bytes)
                .map_err(hyper_io_error)
        }

        fn poll_flush(
            mut self: Pin<&mut Self>,
            cx: &mut Context<'_>,
        ) -> Poll<Result<(), hyper::io::Error>> {
            Pin::new(&mut self.0).poll_flush(cx).map_err(hyper_io_error)
        }

        fn poll_shutdown(
            mut self: Pin<&mut Self>,
            cx: &mut Context<'_>,
        ) -> Poll<Result<(), hyper::io::Error>> {
            Pin::new(&mut self.0)
                .poll_shutdown(cx)
                .map_err(hyper_io_error)
        }
    }

    #[derive(Clone)]
    struct AppState {
        game: Arc<Mutex<PlotTwist>>,
        started: Instant,
    }

    impl AppState {
        fn now_ms(&self) -> u64 {
            self.started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64
        }
    }

    #[derive(Deserialize)]
    struct TokenRequest {
        token: String,
    }

    #[derive(Deserialize)]
    struct ActionRequest {
        token: String,
        action: Action,
    }

    fn response(status: StatusCode, content_type: &'static str, body: Vec<u8>) -> Response {
        Response::builder()
            .status(status)
            .header(CONTENT_TYPE, content_type)
            .header(CONTENT_LENGTH, body.len().to_string())
            .header(CACHE_CONTROL, "no-store")
            .body(Body::from(body))
            .unwrap_or_else(|_| Response::new(Body::empty()))
    }

    fn text(content_type: &'static str, body: &'static str) -> Response {
        response(StatusCode::OK, content_type, body.as_bytes().to_vec())
    }

    fn api(value: Value) -> Response {
        match serde_json::to_vec(&value) {
            Ok(body) => response(StatusCode::OK, "application/json; charset=utf-8", body),
            Err(_) => error(GameError("response serialization failed".to_string())),
        }
    }

    fn error(error: GameError) -> Response {
        let body = serde_json::to_vec(&json!({ "ok": false, "error": error.0 }))
            .unwrap_or_else(|_| b"{\"ok\":false}".to_vec());
        response(
            StatusCode::BAD_REQUEST,
            "application/json; charset=utf-8",
            body,
        )
    }

    async fn connect(
        State(state): State<AppState>,
        Json(request): Json<ConnectRequest>,
    ) -> Response {
        match state.game.lock().await.connect(request) {
            Ok(session) => api(json!({ "ok": true, "session": session })),
            Err(err) => error(err),
        }
    }

    async fn list_lobbies(State(state): State<AppState>) -> Response {
        let now = state.now_ms();
        let lobbies = state.game.lock().await.lobbies(now);
        api(json!({ "ok": true, "lobbies": lobbies, "serverNowMs": now }))
    }

    async fn create_lobby(
        State(state): State<AppState>,
        Json(request): Json<TokenRequest>,
    ) -> Response {
        let now = state.now_ms();
        match state.game.lock().await.create_lobby(&request.token, now) {
            Ok(snapshot) => api(json!({ "ok": true, "snapshot": snapshot })),
            Err(err) => error(err),
        }
    }

    async fn join_lobby(
        State(state): State<AppState>,
        Path(id): Path<String>,
        Json(request): Json<TokenRequest>,
    ) -> Response {
        let now = state.now_ms();
        match state.game.lock().await.join_lobby(&id, &request.token, now) {
            Ok(snapshot) => api(json!({ "ok": true, "snapshot": snapshot })),
            Err(err) => error(err),
        }
    }

    async fn state(
        State(state): State<AppState>,
        Path(id): Path<String>,
        Json(request): Json<TokenRequest>,
    ) -> Response {
        let now = state.now_ms();
        match state.game.lock().await.snapshot(&id, &request.token, now) {
            Ok(snapshot) => api(json!({ "ok": true, "snapshot": snapshot })),
            Err(err) => error(err),
        }
    }

    async fn act(
        State(state): State<AppState>,
        Path(id): Path<String>,
        Json(request): Json<ActionRequest>,
    ) -> Response {
        let now = state.now_ms();
        match state
            .game
            .lock()
            .await
            .act(&id, &request.token, request.action, now)
        {
            Ok(result) => api(json!({ "ok": true, "result": result })),
            Err(err) => error(err),
        }
    }

    async fn emoji(Path(code): Path<String>) -> Response {
        match EMOJI.iter().find(|(name, _)| *name == code) {
            Some((_, svg)) => text("image/svg+xml", svg),
            None => response(StatusCode::NOT_FOUND, "text/plain", b"not found".to_vec()),
        }
    }

    fn router(state: AppState) -> Router {
        Router::new()
            .route(
                "/",
                get(|| async { text("text/html; charset=utf-8", INDEX_HTML) }),
            )
            .route(
                "/app.css",
                get(|| async { text("text/css; charset=utf-8", APP_CSS) }),
            )
            .route(
                "/app.js",
                get(|| async { text("application/javascript; charset=utf-8", APP_JS) }),
            )
            .route("/emoji/{code}", get(emoji))
            .route("/api/connect", post(connect))
            .route("/api/lobbies", get(list_lobbies).post(create_lobby))
            .route("/api/lobbies/{id}/join", post(join_lobby))
            .route("/api/lobbies/{id}/state", post(self::state))
            .route("/api/lobbies/{id}/action", post(act))
            .with_state(state)
    }

    async fn serve() -> Result<(), io::Error> {
        let state = AppState {
            game: Arc::new(Mutex::new(PlotTwist::default())),
            started: Instant::now(),
        };
        let address = SocketAddr::from(([0, 0, 0, 0], HTTP_PORT));
        let mut listener = trueos::lifecycle::RebindableTcpListener::bind(
            trueos::lifecycle::ServerConfig::new("plot-twist-http", address),
            &BOUND_PORT,
        )
        .await;
        let app = router(state);
        loop {
            let (stream, _) = listener.accept().await;
            let connection = CONNECTION_COUNT.fetch_add(1, Ordering::Relaxed);
            logl::log(
                level::INFO,
                format_args!("PlotTwist: accepted connection={connection}"),
            );
            let app = app.clone();
            tokio::task::spawn_local(async move {
                logl::log(
                    level::INFO,
                    format_args!("PlotTwist: HTTP task start connection={connection}"),
                );
                let service = service_fn(move |request| {
                    logl::log(
                        level::INFO,
                        format_args!(
                            "PlotTwist: HTTP request connection={} method={} uri={}",
                            connection,
                            request.method(),
                            request.uri()
                        ),
                    );
                    let mut app = app.clone();
                    async move { app.call(request).await }
                });
                if let Err(err) = hyper::server::conn::http1::Builder::new()
                    .serve_connection(HyperIo(stream), service)
                    .await
                {
                    logl::log(
                        level::WARN,
                        format_args!(
                            "PlotTwist: connection={} failed {err:?}",
                            connection
                        ),
                    );
                }
            });
        }
    }

    pub fn main() {
        logl::log(level::INFO, "PlotTwist: blueprint start");
        let runtime = match runtime::current_thread_net().build() {
            Ok(runtime) => runtime,
            Err(err) => {
                logl::log(
                    level::ERROR,
                    format_args!("PlotTwist: runtime build failed {err}"),
                );
                return;
            }
        };
        let local = tokio::task::LocalSet::new();
        local.block_on(&runtime, async {
            if let Err(err) = serve().await {
                logl::log(
                    level::ERROR,
                    format_args!("PlotTwist: server failed {err:?}"),
                );
            }
        });
        BOUND_PORT.store(0, Ordering::Release);
        platform::poll_once();
    }
}

#[cfg(target_os = "trueos")]
fn main() {
    server::main();
}

#[cfg(not(target_os = "trueos"))]
fn main() {
    eprintln!("PlotTwist is a TRUEOS network blueprint; build it with `cargo bp PlotTwist`.");
}
