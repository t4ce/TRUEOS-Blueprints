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
use codex_transport::http::{
    ClientRouteClass, HttpClientFactory, OutboundProxyPolicy, RouteAwareClientPool,
};
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

unsafe extern "C" {
    fn setenv(
        name: *const std::ffi::c_char,
        value: *const std::ffi::c_char,
        overwrite: std::ffi::c_int,
    ) -> std::ffi::c_int;
    fn unsetenv(name: *const std::ffi::c_char) -> std::ffi::c_int;
    fn getenv(name: *const std::ffi::c_char) -> *mut std::ffi::c_char;
}

fn environment_contract() -> bool {
    // Retain an unlogged per-instance sentinel until process teardown. A second
    // VM must not inherit it through a shared host/environment pointer cell.
    let owner = std::env::var("TRUEOS_APP_INSTANCE").unwrap_or_else(|_| "host".into());
    let owner_isolated = std::env::var_os("MCP_PROBE_ENV_OWNER").is_none();
    let owner_value = std::ffi::CString::new(owner.as_bytes()).unwrap();
    let owner_set =
        unsafe { setenv(c"MCP_PROBE_ENV_OWNER".as_ptr(), owner_value.as_ptr(), 1) } == 0;
    if !owner_set {
        log(format_args!(
            "phase=environment_owner ok=0 errno={}",
            std::io::Error::last_os_error().raw_os_error().unwrap_or(0)
        ));
    }
    let owner_isolated = owner_isolated
        && owner_set
        && std::env::vars_os()
            .any(|(name, value)| name == "MCP_PROBE_ENV_OWNER" && value == owner.as_str());
    let key = "MCP_PROBE_ENV_CONTRACT";
    let raw_key = c"MCP_PROBE_ENV_CONTRACT";
    let snapshot =
        |value: &str| std::env::vars_os().any(|(name, entry)| name == key && entry == value);
    let ok = owner_isolated
        && unsafe { setenv(raw_key.as_ptr(), c"first".as_ptr(), 1) } == 0
        && std::env::var(key).as_deref() == Ok("first")
        && snapshot("first")
        && unsafe { setenv(raw_key.as_ptr(), c"ignored".as_ptr(), 0) } == 0
        && std::env::var(key).as_deref() == Ok("first")
        && unsafe { setenv(raw_key.as_ptr(), c"second".as_ptr(), 1) } == 0
        && std::env::var(key).as_deref() == Ok("second")
        && snapshot("second");
    let raw = unsafe { getenv(raw_key.as_ptr()) };
    let ok =
        ok && !raw.is_null() && unsafe { std::ffi::CStr::from_ptr(raw) }.to_bytes() == b"second";
    let ok = ok
        && unsafe { unsetenv(raw_key.as_ptr()) } == 0
        && std::env::var_os(key).is_none()
        && !std::env::vars_os().any(|(name, _)| name == key)
        && unsafe { setenv(c"".as_ptr(), c"value".as_ptr(), 1) } == -1
        && std::io::Error::last_os_error().raw_os_error() == Some(22);
    #[cfg(target_os = "trueos")]
    let ok = ok
        && unsafe { setenv(c"TRUEOS_FS_SCOPE".as_ptr(), c"trueosfs".as_ptr(), 1) } == -1
        && std::io::Error::last_os_error().raw_os_error() == Some(1);
    log(format_args!(
        "phase=environment_contract ok={}",
        u8::from(ok)
    ));
    ok
}

async fn environment_worker_contract() -> bool {
    let key = "MCP_PROBE_ENV_WORKER";
    unsafe {
        std::env::set_var(key, "main");
    }
    let (tx, rx) = tokio::sync::oneshot::channel();
    let thread = std::thread::spawn(move || {
        let ok = std::env::var(key).as_deref() == Ok("main")
            && std::env::vars_os().any(|(name, value)| name == key && value == "main");
        unsafe {
            std::env::set_var(key, "worker");
        }
        let _ = tx.send(ok);
    });
    let ok = matches!(
        tokio::time::timeout(Duration::from_secs(3), rx).await,
        Ok(Ok(true))
    ) && std::env::var(key).as_deref() == Ok("worker");
    drop(thread);
    unsafe {
        std::env::remove_var(key);
    }
    log(format_args!("phase=environment_worker ok={}", u8::from(ok)));
    ok
}

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
    if name == "wrong_root" {
        // Paired fixture evidence must witness an unknown-CA TLS alert and no
        // HTTP request; status zero alone cannot distinguish an unreachable peer.
        let ok = initialized.is_err() && http.last_status.load(Ordering::Relaxed) == 0;
        client.shutdown().await;
        log(format_args!(
            "case=wrong_root phase=untrusted_endpoint_failed ok={}",
            u8::from(ok)
        ));
        return if ok {
            Ok(())
        } else {
            Err("certificate_rejection")
        };
    }
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

async fn certificate_rejection_witness(base: &str) -> bool {
    // The exec-server adapter intentionally flattens transport error chains.
    // Inspect the same normal pool directly for this synthetic negative case;
    // never print the chain or weaken the verifier to make the request pass.
    let client = RouteAwareClientPool::with_chatgpt_cloudflare_cookies_without_request_logging(
        HttpClientFactory::new(OutboundProxyPolicy::ReqwestDefault),
        ClientRouteClass::Other,
    );
    let result = tokio::time::timeout(
        Duration::from_secs(4),
        client
            .get(format!("{base}/wrong_root"))
            .timeout(Duration::from_secs(3))
            .send(),
    )
    .await;
    let unknown_issuer = match result {
        Ok(Err(error)) => {
            let mut current: Option<&(dyn std::error::Error + 'static)> = Some(&error);
            let mut found = false;
            while let Some(error) = current {
                found |= format!("{error:?}").contains("UnknownIssuer");
                current = error.source();
            }
            found
        }
        _ => false,
    };
    log(format_args!(
        "case=wrong_root phase=certificate_validation unknown_issuer={} ok={}",
        u8::from(unknown_issuer),
        u8::from(unknown_issuer)
    ));
    unknown_issuer
}

async fn run(base: &str, wrong_root_base: Option<&str>) -> bool {
    if !environment_worker_contract().await {
        return false;
    }
    let mut passed = 0;
    let mut trusted_endpoint_succeeded = false;
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
        if name == "json" {
            trusted_endpoint_succeeded = ok;
        }
        passed += usize::from(ok);
        log(format_args!(
            "case={name} phase=result ok={} duration_ms={}",
            u8::from(ok),
            start.elapsed().as_millis()
        ));
    }
    let mut total = 9;
    if let Some(wrong_root_base) = wrong_root_base {
        total += 1;
        let result =
            tokio::time::timeout(Duration::from_secs(12), case(wrong_root_base, "wrong_root"))
                .await;
        let certificate_rejected = certificate_rejection_witness(wrong_root_base).await;
        let ok = trusted_endpoint_succeeded && certificate_rejected && matches!(result, Ok(Ok(())));
        passed += usize::from(ok);
        log(format_args!(
            "case=wrong_root phase=result ok={}",
            u8::from(ok)
        ));
    }
    log(format_args!("phase=summary passed={passed} total={total}"));
    passed == total
}

fn main() {
    install_phase_logging();
    if !environment_contract() {
        log("FAIL environment");
        return;
    }
    // LAN fixture only; no auth files, bearer headers or account state are read.
    let mut args = std::env::args().skip(1);
    let mut base = None;
    let mut wrong_root_base = None;
    // Shell3 exact-name launch has no argv forwarding. The native probe now
    // defaults to the next contract under test; host CLI defaults stay HTTP1.
    let mut https = cfg!(target_os = "trueos");
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--https" => https = true,
            "--http1" => https = false,
            "--wrong-root" => wrong_root_base = args.next(),
            _ if !arg.starts_with('-') && base.is_none() => base = Some(arg),
            _ => {
                log("phase=arguments ok=0");
                return;
            }
        }
    }
    let base = base.unwrap_or_else(|| {
        if https {
            "https://192.168.178.111:18768"
        } else {
            "http://192.168.178.111:18766"
        }
        .into()
    });
    let ca_file = if https {
        let path = std::env::temp_dir().join("mcp_http_probe-synthetic-ca.pem");
        if std::fs::create_dir_all(std::env::temp_dir())
            .and_then(|_| std::fs::write(&path, include_bytes!("tls/ca.pem")))
            .is_err()
        {
            log("phase=fixture_ca ok=0");
            return;
        }
        // This process owns the environment before starting any Tokio threads.
        // The unmodified route-aware transport consumes its normal CA setting.
        unsafe {
            std::env::set_var("CODEX_CA_CERTIFICATE", &path);
        }
        let env_ok = std::env::var_os("CODEX_CA_CERTIFICATE").as_deref() == Some(path.as_os_str())
            && std::env::vars_os()
                .any(|(key, value)| key == "CODEX_CA_CERTIFICATE" && value == path.as_os_str());
        log(format_args!(
            "phase=fixture_ca_readback ok={}",
            u8::from(env_ok)
        ));
        if !env_ok {
            log("FAIL environment");
            return;
        }
        wrong_root_base.get_or_insert_with(|| "https://192.168.178.111:18769".into());
        log("phase=fixture_ca ok=1");
        Some(path)
    } else {
        None
    };
    #[cfg(target_os = "trueos")]
    let runtime = trueos::t::runtime::current_thread().enable_io().build();
    #[cfg(not(target_os = "trueos"))]
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build();
    let Ok(runtime) = runtime else {
        log("phase=runtime ok=0");
        return;
    };
    let ok = runtime.block_on(run(&base, wrong_root_base.as_deref()));
    drop(runtime);
    if let Some(path) = ca_file {
        let _ = std::fs::remove_file(path);
    }
    if ok {
        log("ALL PASS");
    } else {
        log("FAIL");
        #[cfg(not(target_os = "trueos"))]
        std::process::exit(1);
    }
}
