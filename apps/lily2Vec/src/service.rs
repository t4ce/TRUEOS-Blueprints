//! A separate Blueprint/Hull owns inference and Spirit submission. HTTP only
//! admits bounded snapshots; the worker drains them without blocking Codex.
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
const CADENCE: Duration = Duration::from_millis(2500);
const REPEAT: Duration = Duration::from_secs(5);
const EXPIRE: Duration = Duration::from_secs(10);
const IDLE: Duration = Duration::from_secs(120);

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    source: String,
    sequence: u64,
    #[serde(default)]
    text: String,
    #[serde(default)]
    clear: bool,
}
struct Snapshot {
    request: u64,
    source: String,
    sequence: u64,
    text: String,
    queued: Instant,
}
struct Source {
    sequence: u64,
    touched: Instant,
    pending: Option<Snapshot>,
}
#[derive(Default)]
struct Inbox {
    received: u64,
    sources: HashMap<String, Source>,
    last_attempt: Option<Instant>,
    last_signal: Option<(usize, Instant)>,
    classified: u64,
    signaled: u64,
    last_error: Option<String>,
}
#[derive(Clone)]
struct Service {
    inbox: Arc<Mutex<Inbox>>,
}

impl Inbox {
    fn admit(&mut self, request: Request, now: Instant) -> Result<bool, StatusCode> {
        self.received += 1;
        if request.source.is_empty()
            || request.source.len() > 128
            || request
                .source
                .chars()
                .any(|c| c.is_control() || c.is_whitespace())
            || request.sequence == 0
            || request.text.len() > 4096
            || request.text.chars().count() > 768
            || (!request.clear && request.text.trim().is_empty())
        {
            return Err(StatusCode::BAD_REQUEST);
        }
        self.sources
            .retain(|_, s| now.saturating_duration_since(s.touched) < IDLE);
        if !self.sources.contains_key(&request.source) && self.sources.len() >= SOURCES {
            return Err(StatusCode::TOO_MANY_REQUESTS);
        }
        let source = self
            .sources
            .entry(request.source.clone())
            .or_insert_with(|| Source {
                sequence: 0,
                touched: now,
                pending: None,
            });
        if request.sequence <= source.sequence {
            return Ok(false);
        }
        source.sequence = request.sequence;
        source.touched = now;
        source.pending = if request.clear {
            None
        } else {
            Some(Snapshot {
                request: self.received,
                source: request.source,
                sequence: request.sequence,
                text: request.text,
                queued: now,
            })
        };
        Ok(true)
    }
    fn take(&mut self, now: Instant) -> Option<Snapshot> {
        self.sources
            .retain(|_, s| now.saturating_duration_since(s.touched) < IDLE);
        for source in self.sources.values_mut() {
            if source
                .pending
                .as_ref()
                .is_some_and(|p| now.saturating_duration_since(p.queued) >= EXPIRE)
            {
                source.pending = None;
            }
        }
        if self
            .last_attempt
            .is_some_and(|last| now.saturating_duration_since(last) < CADENCE)
        {
            return None;
        }
        // Oldest pending source gets its newest snapshot; replacements cannot
        // accumulate an unbounded sequence of obsolete cues.
        let key = self
            .sources
            .iter()
            .filter_map(|(id, s)| s.pending.as_ref().map(|p| (id, p.queued)))
            .min_by_key(|(_, when)| *when)
            .map(|(id, _)| id.clone())?;
        self.last_attempt = Some(now);
        self.sources.get_mut(&key)?.pending.take()
    }
}

async fn health(State(service): State<Service>) -> Json<serde_json::Value> {
    let inbox = service.inbox.lock().await;
    Json(serde_json::json!({
        "protocol":"lily2vec-v1", "backend":"avx-vnni", "ready":true,
        "received":inbox.received,
        "queue_sources":inbox.sources.len(),
        "queue_pending":inbox.sources.values().filter(|s| s.pending.is_some()).count(),
        "classified":inbox.classified, "signaled":inbox.signaled, "last_error":inbox.last_error,
    }))
}
async fn enqueue(
    State(service): State<Service>,
    Json(request): Json<Request>,
) -> (StatusCode, Json<serde_json::Value>) {
    let sequence = request.sequence;
    let chars = request.text.chars().count();
    let clear = request.clear;
    let mut inbox = service.inbox.lock().await;
    let result = inbox.admit(request, Instant::now());
    let number = inbox.received;
    drop(inbox);
    let state = match result {
        Ok(false) => "stale",
        Ok(true) if clear => "clear",
        Ok(true) => "queued",
        Err(_) => "rejected",
    };
    activity(
        number,
        &format!("recv seq={sequence} chars={chars} {state}"),
    );
    match result {
        Ok(accepted) => (
            StatusCode::ACCEPTED,
            Json(serde_json::json!({"accepted":accepted,"stale":!accepted})),
        ),
        Err(status) => (
            status,
            Json(
                serde_json::json!({"accepted":false,"error":"invalid snapshot or source capacity reached"}),
            ),
        ),
    }
}

fn signal(label: &str, dry_run: bool) -> Result<()> {
    if dry_run {
        return Ok(());
    }
    #[cfg(target_os = "trueos")]
    return trueos::spirit::play_expression(label)
        .map_err(|error| anyhow::anyhow!("Spirit admission failed: {}", error.0));
    #[cfg(not(target_os = "trueos"))]
    {
        let _ = label;
        bail!("Spirit signaling requires TRUEOS; use --dry-run for a host test");
    }
}

async fn worker(service: Service, model: Model, dry_run: bool) {
    let mut tick = tokio::time::interval(Duration::from_millis(50));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        tick.tick().await;
        let Some(job) = service.inbox.lock().await.take(Instant::now()) else {
            continue;
        };
        let result = model.encode(&job.text).and_then(|q| model.score_vnni(&q));
        let mut inbox = service.inbox.lock().await;
        // Clear/new sequence admission invalidates a previously selected job.
        if !inbox
            .sources
            .get(&job.source)
            .is_some_and(|s| s.sequence == job.sequence)
        {
            continue;
        }
        let scores = match result {
            Ok(scores) => scores,
            Err(error) => {
                activity(job.request, &format!("inference=failed {error}"));
                inbox.last_error = Some(error.to_string());
                continue;
            }
        };
        inbox.classified += 1;
        let index = model.winner(&scores);
        let label = &model.labels[index];
        let now = Instant::now();
        if inbox.last_signal.is_some_and(|(last, when)| {
            last == index && now.saturating_duration_since(when) < REPEAT
        }) {
            activity(job.request, &format!("result={label} spirit=held-repeat"));
            continue;
        }
        match signal(label, dry_run) {
            Ok(()) => {
                inbox.signaled += 1;
                inbox.last_signal = Some((index, now));
                inbox.last_error = None;
                let state = if dry_run { "dry-run" } else { "queued" };
                activity(job.request, &format!("result={label} spirit={state}"));
                if dry_run {
                    println!(
                        "{}",
                        serde_json::json!({"source":job.source,"sequence":job.sequence,"label":label,"accepted":true})
                    );
                }
            }
            Err(error) => {
                activity(
                    job.request,
                    &format!("result={label} spirit=failed {error}"),
                );
                inbox.last_error = Some(error.to_string());
            }
        }
    }
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
    let model = Model::load(lily2vec::DEFAULT_GUIDE)?;
    // Execute the target-feature kernel before advertising readiness.
    let query = model.encode("hello")?;
    // Keep warmup arithmetic observable even under whole-program optimization.
    std::hint::black_box(model.score_vnni(&query)?);
    let service = Service {
        inbox: Arc::new(Mutex::new(Inbox::default())),
    };
    let router = Router::new()
        .route("/health", get(health))
        .route("/v1/snapshots", post(enqueue))
        .layer(DefaultBodyLimit::max(8192))
        .with_state(service.clone());
    announce(&format!(
        "lily2vec-service: READY http://127.0.0.1:{} protocol=lily2vec-v1 backend=avx-vnni queue=32 scheduling=default-hull dry_run={dry_run}",
        listener.local_addr()?.port()
    ));
    let job = tokio::task::spawn_local(worker(service, model, dry_run));
    #[cfg(any(target_os = "trueos", feature = "native-transport"))]
    let result = native_transport::serve(listener, router)
        .await
        .map_err(anyhow::Error::msg);
    #[cfg(not(any(target_os = "trueos", feature = "native-transport")))]
    let result = axum::serve(listener, router)
        .await
        .map_err(anyhow::Error::from);
    job.abort();
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

#[cfg(test)]
mod tests {
    use super::*;
    fn request(source: &str, sequence: u64, text: &str) -> Request {
        Request {
            source: source.into(),
            sequence,
            text: text.into(),
            clear: false,
        }
    }
    #[test]
    fn latest_sequence_wins_clear_cancels_and_stale_work_does_not_return() {
        let mut inbox = Inbox::default();
        let now = Instant::now();
        inbox.admit(request("one", 1, "hi"), now).unwrap();
        inbox.admit(request("one", 2, "no"), now).unwrap();
        assert!(!inbox.admit(request("one", 1, "old"), now).unwrap());
        assert_eq!(inbox.take(now).unwrap().text, "no");
        inbox.admit(request("one", 3, "sad"), now).unwrap();
        let mut clear = request("one", 4, "");
        clear.clear = true;
        inbox.admit(clear, now).unwrap();
        assert!(inbox.take(now + CADENCE).is_none());
        assert!(!inbox.admit(request("one", 3, "late"), now).unwrap());
    }
    #[test]
    fn bounded_sources_expiry_cadence_and_unicode_limits() {
        let mut inbox = Inbox::default();
        let now = Instant::now();
        for i in 0..SOURCES {
            inbox
                .admit(request(&format!("source-{i}"), 1, "hello"), now)
                .unwrap();
        }
        assert_eq!(
            inbox.admit(request("overflow", 1, "hi"), now),
            Err(StatusCode::TOO_MANY_REQUESTS)
        );
        assert!(inbox.take(now).is_some());
        assert!(inbox.take(now).is_none());
        assert!(inbox.take(now + EXPIRE).is_none());
        inbox.admit(request("new", 1, "hi"), now + IDLE).unwrap();
        assert_eq!(inbox.sources.len(), 1);
        assert_eq!(
            inbox.admit(request("new", 2, &"🙂".repeat(769)), now + IDLE),
            Err(StatusCode::BAD_REQUEST)
        );
    }
}
