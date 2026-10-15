//! Synthetic, unauthenticated MCP HTTP contract probe using Codex's real client.
use codex_config::types::{AuthKeyringBackendKind, OAuthCredentialsStoreMode};
use codex_exec_server::{
    ExecServerError, HttpClient, HttpRequestParams, HttpRequestResponse, HttpResponseBodyStream,
    RouteAwareHttpClient,
};
use codex_rmcp_client::{
    ElicitationAction, ElicitationResponse, McpOAuthRefreshMode, McpProtocolMode, RmcpClient,
    StreamableHttpBearerToken, StreamableHttpRedirectMode,
};
use codex_transport::http::{HttpClientFactory, OutboundProxyPolicy};
use futures::{FutureExt, future::BoxFuture};
use rmcp::model::{ClientCapabilities, Implementation, InitializeRequestParams, ProtocolVersion};
use std::{
    io::Write,
    sync::{
        Arc,
        atomic::{AtomicU16, AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};
use tracing_subscriber::prelude::*;

fn log(message: impl std::fmt::Display) {
    #[cfg(target_os = "trueos")]
    trueos::logl::log(
        trueos::logl::level::INFO,
        format_args!("mcp_http_probe: {message}"),
    );
    #[cfg(not(target_os = "trueos"))]
    println!("mcp_http_probe: {message}");
}

struct PhaseWriter(Vec<u8>);
impl Write for PhaseWriter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
impl Drop for PhaseWriter {
    fn drop(&mut self) {
        log(String::from_utf8_lossy(&self.0).trim_end());
    }
}

fn install_phase_logging() {
    // The production event owns its numeric/static field allowlist. Reject
    // every other tracing target, including request/error/auth diagnostics.
    let layer = tracing_subscriber::fmt::layer()
        .with_ansi(false)
        .without_time()
        .with_target(false)
        .with_writer(|| PhaseWriter(Vec::new()))
        .with_filter(tracing_subscriber::filter::filter_fn(|metadata| {
            metadata.target() == "codex_rmcp_client::phase"
        }));
    let _ = tracing::subscriber::set_global_default(tracing_subscriber::registry().with(layer));
}

struct ObservedHttp {
    inner: RouteAwareHttpClient,
    case: &'static str,
    requests: AtomicUsize,
    last_status: AtomicU16,
}
impl HttpClient for ObservedHttp {
    fn http_request(
        &self,
        params: HttpRequestParams,
    ) -> BoxFuture<'_, Result<HttpRequestResponse, ExecServerError>> {
        async move {
            let start = Instant::now();
            let sequence = self.requests.fetch_add(1, Ordering::Relaxed);
            let result = self.inner.http_request(params).await;
            self.last_status.store(
                result.as_ref().map(|response| response.status).unwrap_or(0),
                Ordering::Relaxed,
            );
            log(format_args!(
                "case={} phase=headers sequence={} status_code={} duration_ms={}",
                self.case,
                sequence,
                result.as_ref().map(|response| response.status).unwrap_or(0),
                start.elapsed().as_millis()
            ));
            result
        }
        .boxed()
    }
    fn http_request_stream(
        &self,
        params: HttpRequestParams,
    ) -> BoxFuture<'_, Result<(HttpRequestResponse, HttpResponseBodyStream), ExecServerError>> {
        async move {
            let start = Instant::now();
            let sequence = self.requests.fetch_add(1, Ordering::Relaxed);
            let result = self.inner.http_request_stream(params).await;
            self.last_status.store(
                result
                    .as_ref()
                    .map(|(response, _)| response.status)
                    .unwrap_or(0),
                Ordering::Relaxed,
            );
            log(format_args!(
                "case={} phase=headers sequence={} status_code={} duration_ms={}",
                self.case,
                sequence,
                result
                    .as_ref()
                    .map(|(response, _)| response.status)
                    .unwrap_or(0),
                start.elapsed().as_millis()
            ));
            result
        }
        .boxed()
    }
}

async fn case(base: &str, name: &'static str) -> Result<(), &'static str> {
    log(format_args!("case={name} phase=construct state=started"));
    let factory = HttpClientFactory::new(OutboundProxyPolicy::ReqwestDefault);
    let http = Arc::new(ObservedHttp {
        inner: RouteAwareHttpClient::new(factory),
        case: name,
        requests: AtomicUsize::new(0),
        last_status: AtomicU16::new(0),
    });
    let start = Instant::now();
    let client = RmcpClient::new_streamable_http_client_with_protocol_mode_and_redirect_mode(
        "synthetic_mcp_probe",
        &format!("{base}/{name}"),
        // This adapter intentionally sends no auth. Mark credentials as owned
        // by the HTTP client to bypass every local OAuth-store lookup.
        Some(StreamableHttpBearerToken::ProvidedByHttpClient),
        None,
        None,
        None,
        OAuthCredentialsStoreMode::File,
        AuthKeyringBackendKind::default(),
        http.clone(),
        None,
        McpProtocolMode::Legacy,
        StreamableHttpRedirectMode::Legacy,
        McpOAuthRefreshMode::Legacy,
    )
    .await
    .map_err(|_| "construct")?;
    log(format_args!(
        "case={name} phase=initialize state=started deadline_ms=5000"
    ));
    let params = InitializeRequestParams::new(
        ClientCapabilities::default(),
        Implementation::new("mcp-http-probe", "1"),
    )
    .with_protocol_version(ProtocolVersion::V_2025_06_18);
    let initialized = client
        .initialize(
            params,
            Some(Duration::from_secs(5)),
            Box::new(|_, _| {
                Box::pin(async {
                    Ok(ElicitationResponse {
                        action: ElicitationAction::Decline,
                        content: None,
                        meta: None,
                    })
                })
            }),
        )
        .await;
    log(format_args!(
        "case={name} phase=initialize ok={} duration_ms={}",
        u8::from(initialized.is_ok()),
        start.elapsed().as_millis()
    ));
    if initialized.is_err() {
        client.shutdown().await;
        return Err("initialize");
    }
    http.last_status.store(0, Ordering::Relaxed);
    let deadline_ms = match name {
        "headers_timeout" | "body_timeout" => 300,
        "cancel" => 150,
        _ => 5000,
    };
    log(format_args!(
        "case={name} phase=tools_list state=started deadline_ms={deadline_ms}"
    ));
    let start = Instant::now();
    let mut count = 0;
    let ok = if name == "cancel" {
        let cancelled = tokio::time::timeout(
            Duration::from_millis(150),
            client.list_tools(None, Some(Duration::from_secs(10))),
        )
        .await;
        cancelled.is_err()
    } else {
        let deadline = if name == "headers_timeout" || name == "body_timeout" {
            Duration::from_millis(300)
        } else {
            Duration::from_secs(5)
        };
        let result = client.list_tools(None, Some(deadline)).await;
        if name == "json" || name == "sse" || name == "recovery" {
            if let Ok(result) = result {
                count = result.tools.len();
                count == 106
            } else {
                false
            }
        } else {
            result.is_err()
        }
    };
    let elapsed = start.elapsed().as_millis();
    log(format_args!(
        "case={name} phase=tools_list ok={} tools={count} duration_ms={elapsed}",
        u8::from(ok)
    ));
    // A successful SSE reply must be delivered while its HTTP stream is open.
    let expected_status = match name {
        "401" => 401,
        "404" => 404,
        "reset" | "headers_timeout" | "cancel" => 0,
        _ => 200,
    };
    let ok = ok
        && (name != "sse" || elapsed < 4_000)
        && (name != "body_timeout" && name != "headers_timeout" || (250..2_000).contains(&elapsed))
        && (name == "cancel" || http.last_status.load(Ordering::Relaxed) == expected_status);
    let start = Instant::now();
    client.shutdown().await;
    drop(client);
    tokio::task::yield_now().await;
    log(format_args!(
        "case={name} phase=shutdown duration_ms={}",
        start.elapsed().as_millis()
    ));
    if ok { Ok(()) } else { Err("tools_list") }
}

async fn run(base: &str) -> bool {
    let mut passed = 0;
    for name in [
        "json",
        "sse",
        "404",
        "401",
        "reset",
        "headers_timeout",
        "body_timeout",
        "cancel",
        "recovery",
    ] {
        let start = Instant::now();
        let result = tokio::time::timeout(Duration::from_secs(12), case(base, name)).await;
        let ok = matches!(result, Ok(Ok(())));
        passed += usize::from(ok);
        log(format_args!(
            "case={name} phase=result ok={} duration_ms={}",
            u8::from(ok),
            start.elapsed().as_millis()
        ));
    }
    log(format_args!("phase=summary passed={passed} total=9"));
    passed == 9
}

fn main() {
    install_phase_logging();
    // LAN fixture only; no auth files, bearer headers or account state are read.
    let base = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "http://192.168.178.111:18766".into());
    #[cfg(target_os = "trueos")]
    let runtime = trueos::t::runtime::current_thread().build();
    #[cfg(not(target_os = "trueos"))]
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build();
    let Ok(runtime) = runtime else {
        log("phase=runtime ok=0");
        return;
    };
    if runtime.block_on(run(&base)) {
        log("ALL PASS");
    } else {
        log("FAIL");
        #[cfg(not(target_os = "trueos"))]
        std::process::exit(1);
    }
}
