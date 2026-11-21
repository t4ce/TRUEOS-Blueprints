//! Lily2Vec classifies completed turns and submits one contextual action.
use anyhow::{Context, Result, bail};
use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, State},
    http::StatusCode,
    routing::{get, post},
};
use lily2vec::{Model, vnni_available};
use serde::Deserialize;
use std::{
    collections::HashMap,
    net::{Ipv4Addr, SocketAddr},
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::Mutex;

#[cfg(any(target_os = "trueos", feature = "native-transport"))]
#[path = "native_transport.rs"]
mod native_transport;

const PORT: u16 = 9771;
const SOURCES: usize = 32;
const SOURCE_IDLE: Duration = Duration::from_secs(120);
const CLASSIFY_TEXT_BYTES: usize = 16 * 1024;
const CLASSIFY_BODY_BYTES: usize = CLASSIFY_TEXT_BYTES * 6 + 1024;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Turn {
    source: String,
    sequence: u64,
    text: String,
}

impl Turn {
    fn valid(&self) -> bool {
        !self.source.is_empty()
            && self.source.len() <= 128
            && !self
                .source
                .chars()
                .any(|c| c.is_control() || c.is_whitespace())
            && self.sequence != 0
            && self.text.len() <= 4096
            && self.text.chars().count() <= 768
            && !self.text.trim().is_empty()
    }
}

struct Source {
    sequence: u64,
    touched: Instant,
}

#[derive(Default)]
struct Inbox {
    received: u64,
    sources: HashMap<String, Source>,
    classified: u64,
    signaled: u64,
    last_error: Option<String>,
}

impl Inbox {
    fn admit(&mut self, turn: &Turn, now: Instant) -> Result<bool, StatusCode> {
        if !turn.valid() {
            return Err(StatusCode::BAD_REQUEST);
        }
        self.sources
            .retain(|_, source| now.saturating_duration_since(source.touched) < SOURCE_IDLE);
        if let Some(source) = self.sources.get_mut(&turn.source) {
            source.touched = now;
            return Ok(turn.sequence > source.sequence);
        }
        if self.sources.len() >= SOURCES {
            return Err(StatusCode::TOO_MANY_REQUESTS);
        }
        Ok(true)
    }

    fn commit(&mut self, turn: Turn, now: Instant) {
        self.sources.insert(
            turn.source,
            Source {
                sequence: turn.sequence,
                touched: now,
            },
        );
    }
}

#[derive(Clone)]
struct Service {
    inbox: Arc<Mutex<Inbox>>,
    contextual: Arc<Model>,
    lookup: Arc<Model>,
    dry_run: bool,
}

fn classify_text<'a>(model: &'a Model, text: &str) -> Result<&'a str> {
    let query = model.encode(text)?;
    let scores = model.score_vnni(&query)?;
    Ok(&model.labels[model.winner(&scores)])
}

async fn health(State(service): State<Service>) -> Json<serde_json::Value> {
    let inbox = service.inbox.lock().await;
    Json(serde_json::json!({
        "protocol":"lily2vec-v2", "backend":"avx-vnni", "ready":true,
        "received":inbox.received,
        "queue_sources":inbox.sources.len(), "queue_pending":0,
        "classified":inbox.classified, "signaled":inbox.signaled, "last_error":inbox.last_error,
    }))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ClassifyRequest {
    text: String,
}

async fn classify(
    State(service): State<Service>,
    Json(request): Json<ClassifyRequest>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    if request.text.len() > CLASSIFY_TEXT_BYTES || request.text.trim().is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }
    let label = classify_text(&service.lookup, &request.text)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(serde_json::json!({"label":label})))
}

async fn legacy_snapshot(
    Json(_): Json<serde_json::Value>,
) -> (StatusCode, Json<serde_json::Value>) {
    let text = "lily2vec: deprecated snapshot signal ignored";
    #[cfg(target_os = "trueos")]
    trueos::logl::log(trueos::logl::level::WARN, text);
    #[cfg(not(target_os = "trueos"))]
    eprintln!("WARN {text}");
    (
        StatusCode::ACCEPTED,
        Json(serde_json::json!({"accepted":false,"deprecated":true})),
    )
}

fn signal(label: &str, dry_run: bool) -> Result<()> {
    if dry_run {
        return Ok(());
    }
    #[cfg(target_os = "trueos")]
    return trueos::spirit::play_contextual(label)
        .map_err(|error| anyhow::anyhow!("Spirit admission failed: {}", error.0));
    #[cfg(not(target_os = "trueos"))]
    {
        let _ = label;
        bail!("Spirit signaling requires TRUEOS; use --dry-run for a host test");
    }
}

async fn present_turn(
    State(service): State<Service>,
    Json(turn): Json<Turn>,
) -> Result<(StatusCode, Json<serde_json::Value>), StatusCode> {
    let now = Instant::now();
    let mut inbox = service.inbox.lock().await;
    inbox.received += 1;
    let request = inbox.received;
    if !inbox.admit(&turn, now)? {
        return Ok((
            StatusCode::ACCEPTED,
            Json(serde_json::json!({"accepted":false,"stale":true})),
        ));
    }
    let label = classify_text(&service.contextual, &turn.text).map_err(|error| {
        inbox.last_error = Some(error.to_string());
        StatusCode::INTERNAL_SERVER_ERROR
    })?;
    let has_action = label != "Static";
    if has_action {
        signal(label, service.dry_run).map_err(|error| {
            inbox.last_error = Some(error.to_string());
            StatusCode::SERVICE_UNAVAILABLE
        })?;
        inbox.signaled += 1;
    }
    inbox.classified += 1;
    inbox.last_error = None;
    let state = if !has_action {
        "idle"
    } else if service.dry_run {
        "dry-run"
    } else {
        "queued"
    };
    activity(
        request,
        &format!("turn seq={} result={label} spirit={state}", turn.sequence),
    );
    if service.dry_run {
        println!(
            "{}",
            serde_json::json!({"source":turn.source,"sequence":turn.sequence,"label":label,"accepted":true})
        );
    }
    inbox.commit(turn, now);
    Ok((
        StatusCode::ACCEPTED,
        Json(serde_json::json!({"accepted":true,"label":label})),
    ))
}

fn activity(request: u64, event: &str) {
    // Plain append-only log records route to this Hull's ShellIO/Matrix slot.
    // UTC needs no timezone database and remains explicit in remote sessions.
    #[cfg(target_os = "trueos")]
    let stamp = trueos::clock::unix_nanos().map(Duration::from_nanos);
    #[cfg(not(target_os = "trueos"))]
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok();
    let time = match stamp {
        Some(stamp) => {
            let seconds = stamp.as_secs();
            format!(
                "{:02}:{:02}:{:02}.{:03}Z",
                seconds / 3600 % 24,
                seconds / 60 % 60,
                seconds % 60,
                stamp.subsec_millis(),
            )
        }
        None => "time=unavailable".into(),
    };
    announce(&format!("lily2vec #{request} {time} {event}"));
}

pub(super) fn announce(text: &str) {
    #[cfg(target_os = "trueos")]
    trueos::logl::log(trueos::logl::level::INFO, text);
    #[cfg(not(target_os = "trueos"))]
    eprintln!("{text}");
}
async fn run(port: u16, dry_run: bool) -> Result<()> {
    // Bind before warming. Concurrent launches cannot own two models/services
    // for the endpoint. VNNI is executed in this separate Hull's entry runtime.
    let listener =
        tokio::net::TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, port))).await?;
    if !vnni_available() {
        bail!("AVX-VNNI + AVX2 required; no substitute inference backend");
    }
    if !cfg!(target_os = "trueos") && !dry_run {
        bail!("host service requires --dry-run; Spirit signaling runs on TRUEOS");
    }
    let contextual = Model::contextual()?;
    let lookup = Model::load(lily2vec::DEFAULT_GUIDE)?;
    // Execute the target-feature kernel before advertising readiness.
    let query = contextual.encode("hello")?;
    // Keep warmup arithmetic observable even under whole-program optimization.
    std::hint::black_box(contextual.score_vnni(&query)?);
    let service = Service {
        inbox: Arc::new(Mutex::new(Inbox::default())),
        contextual: Arc::new(contextual),
        lookup: Arc::new(lookup),
        dry_run,
    };
    let router = Router::new()
        .route("/health", get(health))
        .route(
            "/v1/snapshots",
            post(legacy_snapshot).layer(DefaultBodyLimit::max(8192)),
        )
        .route(
            "/v1/turns",
            post(present_turn).layer(DefaultBodyLimit::max(8192)),
        )
        .route(
            "/v1/classify",
            post(classify).layer(DefaultBodyLimit::max(CLASSIFY_BODY_BYTES)),
        )
        .with_state(service.clone());
    announce(&format!(
        "lily2vec-service: READY http://127.0.0.1:{} protocol=lily2vec-v2 backend=avx-vnni sources=32 scheduling=default-hull dry_run={dry_run}",
        listener.local_addr()?.port()
    ));
    #[cfg(any(target_os = "trueos", feature = "native-transport"))]
    let result = native_transport::serve(listener, router)
        .await
        .map_err(anyhow::Error::msg);
    #[cfg(not(any(target_os = "trueos", feature = "native-transport")))]
    let result = axum::serve(listener, router)
        .await
        .map_err(anyhow::Error::from);
    result
}

pub fn entry(args: &[String]) -> Result<()> {
    let mut port = PORT;
    let mut dry_run = false;
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--serve" => (),
            "--dry-run" => dry_run = true,
            "--port" => port = args.next().context("--port requires a number")?.parse()?,
            _ => bail!("unknown service option: {arg}"),
        }
    }
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let result = tokio::task::LocalSet::new().block_on(&runtime, run(port, dry_run));
    if let Err(error) = &result {
        announce(&format!("lily2vec-service: FAIL {error:#}"));
    }
    result
}
