// trueos-blueprint: features=["tokio-net-probe"]

extern crate alloc;

use alloc::{
    format,
    string::{String, ToString},
    vec::Vec,
};
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
    logl,
    logl::level,
    platform::{self, io},
    runtime, system_services,
    time::{self, Duration},
    tokio::{self, net::SocketAddr},
};

const HTTP_TCP_PORT: u16 = 11_011;
#[allow(non_upper_case_globals)]
const dummy_dont_take_port_11_because_browsers_block: u16 = 11;
const _: () = assert!(HTTP_TCP_PORT != dummy_dont_take_port_11_because_browsers_block);
const BIND_RETRY_MS: u64 = 1_000;
const INDEX_HTML: &str = include_str!("index.html");
const TAILWIND_CSS: &str = include_str!("tailwind.css");

static HTTP_PORT: AtomicU16 = AtomicU16::new(0);

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ServicesSnapshot {
    schema: &'static str,
    generated_at_ms: u64,
    readiness_mask: String,
    service_count: usize,
    summary: Summary,
    services: Vec<ServiceRow>,
    task_profile_history_capacity: usize,
    task_profile: Option<TaskProfileRow>,
    task_profile_history: Vec<TaskProfileRow>,
}

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
struct Summary {
    started: usize,
    waiting: usize,
    gated: usize,
    disabled: usize,
    pools: usize,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ServiceRow {
    name: String,
    title: String,
    enabled: bool,
    gate_open: bool,
    started: bool,
    required_mask: String,
    missing_mask: String,
    kind: String,
    requires: Vec<String>,
    status: &'static str,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct TaskProfileRow {
    sequence: u64,
    now_ms: u64,
    heartbeat_gap_ms: u64,
    executor: String,
    spawned: u64,
    ready: u64,
    polls: u64,
    busy_us: u64,
    busy_permille: u64,
    top_task: String,
    top_task_id: String,
    top_polls: u64,
    top_total_us: u64,
    longest_task: String,
    longest_task_id: String,
    longest_poll_us: u64,
    slow_polls: u64,
    dropped: u64,
    mismatches: u64,
    readiness: String,
}

fn status_code(status: u16) -> StatusCode {
    StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR)
}

fn response(status: u16, content_type: &'static str, body: Vec<u8>, no_store: bool) -> Response {
    let cache = if no_store { "no-store" } else { "no-cache" };
    Response::builder()
        .status(status_code(status))
        .header(CONTENT_TYPE, content_type)
        .header(CONTENT_LENGTH, body.len().to_string())
        .header(CACHE_CONTROL, cache)
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
    text_response(200, "text/html; charset=utf-8", INDEX_HTML)
}

async fn handle_tailwind_css() -> Response {
    text_response(200, "text/css; charset=utf-8", TAILWIND_CSS)
}

async fn handle_healthz() -> Response {
    json_response(
        200,
        &serde_json::json!({
            "ok": true,
            "app": "system-services",
            "port": HTTP_PORT.load(Ordering::Acquire),
        }),
    )
}

async fn handle_snapshot() -> Response {
    match system_services::snapshot_text() {
        Ok(text) if !text.is_empty() => match parse_snapshot(&text) {
            Ok(snapshot) => json_response(200, &snapshot),
            Err(reason) => json_response(502, &serde_json::json!({ "error": reason })),
        },
        Ok(_) | Err(_) => json_response(
            503,
            &serde_json::json!({
                "error": "system service snapshot is unavailable"
            }),
        ),
    }
}

fn parse_snapshot(text: &str) -> Result<ServicesSnapshot, String> {
    let mut generated_at_ms = 0u64;
    let mut readiness_mask = String::from("0x00000000");
    let mut declared_count = None;
    let mut services = Vec::new();
    let mut task_profile_history_capacity = 0usize;
    let mut task_profile_history = Vec::new();

    for line in text.lines() {
        if let Some(value) = line.strip_prefix("generated_at_ms=") {
            generated_at_ms = value.parse().unwrap_or(0);
        } else if let Some(value) = line.strip_prefix("readiness_mask=") {
            readiness_mask = value.to_string();
        } else if let Some(value) = line.strip_prefix("service_count=") {
            declared_count = value.parse::<usize>().ok();
        } else if let Some(value) = line.strip_prefix("task_profile_history_capacity=") {
            task_profile_history_capacity = value.parse::<usize>().unwrap_or(0);
        } else if line.starts_with("service\t") && !line.starts_with("service\tname\t") {
            services.push(parse_service_row(line)?);
        } else if line.starts_with("task-profile\t")
            && !line.starts_with("task-profile\tsequence\t")
        {
            task_profile_history.push(parse_task_profile_row(line)?);
        }
    }

    if services.is_empty() {
        return Err(String::from("snapshot contained no task rows"));
    }

    let mut summary = Summary::default();
    for service in &services {
        match service.status {
            "Started" => summary.started += 1,
            "Waiting" => summary.waiting += 1,
            "Gated" => summary.gated += 1,
            "Disabled" => summary.disabled += 1,
            _ => {}
        }
        if service.kind == "pool" {
            summary.pools += 1;
        }
    }

    let task_profile = task_profile_history.last().cloned();
    Ok(ServicesSnapshot {
        schema: "trueos.system-services.v2",
        generated_at_ms,
        readiness_mask,
        service_count: declared_count.unwrap_or(services.len()),
        summary,
        services,
        task_profile_history_capacity,
        task_profile,
        task_profile_history,
    })
}

fn parse_task_profile_row(line: &str) -> Result<TaskProfileRow, String> {
    let fields = line.split('\t').collect::<Vec<_>>();
    if fields.len() != 21 {
        return Err(format!("bad task profile row with {} fields", fields.len()));
    }
    let number = |index: usize| -> Result<u64, String> {
        fields[index]
            .parse::<u64>()
            .map_err(|_| format!("bad task profile number at field {}", index))
    };
    Ok(TaskProfileRow {
        sequence: number(1)?,
        now_ms: number(2)?,
        heartbeat_gap_ms: number(3)?,
        executor: fields[4].to_string(),
        spawned: number(5)?,
        ready: number(6)?,
        polls: number(7)?,
        busy_us: number(8)?,
        busy_permille: number(9)?,
        top_task: fields[10].to_string(),
        top_task_id: fields[11].to_string(),
        top_polls: number(12)?,
        top_total_us: number(13)?,
        longest_task: fields[14].to_string(),
        longest_task_id: fields[15].to_string(),
        longest_poll_us: number(16)?,
        slow_polls: number(17)?,
        dropped: number(18)?,
        mismatches: number(19)?,
        readiness: fields[20].to_string(),
    })
}

fn parse_service_row(line: &str) -> Result<ServiceRow, String> {
    let fields = line.split('\t').collect::<Vec<_>>();
    if fields.len() != 9 {
        return Err(format!("bad task row with {} fields", fields.len()));
    }
    let enabled = fields[2] == "1";
    let gate_open = fields[3] == "1";
    let started = fields[4] == "1";
    let missing = fields[6] != "0x00000000";
    let status = if !enabled {
        "Disabled"
    } else if started {
        "Started"
    } else if missing {
        "Waiting"
    } else if !gate_open {
        "Gated"
    } else {
        "Starting"
    };

    Ok(ServiceRow {
        name: fields[1].to_string(),
        title: friendly_title(fields[1]),
        enabled,
        gate_open,
        started,
        required_mask: fields[5].to_string(),
        missing_mask: fields[6].to_string(),
        kind: fields[7].to_string(),
        requires: friendly_requirements(fields[8]),
        status,
    })
}

fn friendly_requirements(raw: &str) -> Vec<String> {
    let mut result = Vec::new();
    if raw == "-" {
        return result;
    }
    for requirement in raw.split('|') {
        let label = match requirement {
            "NET_GATEWAY_REACHABLE"
            | "NET_V4_GATEWAY_REACHABLE"
            | "NET_V6_GATEWAY_REACHABLE"
            | "NET_ANY_CONFIGURED"
            | "NET_V4_CONFIGURED"
            | "NET_V6_CONFIGURED" => "Network",
            "NET_SOCKET_READY" | "TLS_SOCKET_SERVICE_READY" => "Network sockets",
            "TRUEOSFS_ROOT_MOUNTED" => "File system",
            "TRUEOSFS_INDEX_READY" => "File index",
            "QJS_ASYNC_FS_READY" => "Async files",
            "INTEL_HDA_READY" => "Audio",
            "BACKGROUND_AP_WORKER_READY" => "Worker cores",
            "VTHREAD_HW_TAG_READY" => "Hardware threads",
            "UI3_INTEL_PRESENT_READY" => "Display",
            "UI3_ASSET_SERVICE_READY" => "UI assets",
            "GFX_VIRGL_READY" | "GFX_BACKEND_READY" => "Graphics",
            "UI_FRAME_READY" => "UI frame",
            "APP_VM_READY" => "App runtime",
            "TOKIO_RUNTIME_READY" => "Async runtime",
            other => other,
        };
        if !result.iter().any(|existing| existing == label) {
            result.push(label.to_string());
        }
    }
    result
}

fn friendly_title(name: &str) -> String {
    let known = match name {
        "job-runner" => "Jobs",
        "blocking-service-lanes" => "Blocking lanes",
        "smp-hlt-history" => "CPU idle",
        "executor-realm-migration-smoke" => "Realm probe",
        "codec-service" => "Codecs",
        "factory-ram-probe" => "Memory probe",
        "font-tessel-boot-probe" => "Font probe",
        "qjs-async-fs-service" => "QuickJS files",
        "trueosfs-mount-service" => "File system",
        "trueosfs-index-service" => "File index",
        "hv-vm-store" => "VM store",
        "hv-vm-store-net" => "VM network",
        "net-poll-tasks" => "Network polling",
        "net-service" => "Network",
        "net-cache-service" => "Network cache",
        "tls-socket-service" => "TLS sockets",
        "ntp-sync" => "NTP sync",
        "sntp-service" => "SNTP",
        "net-shell" => "Network shell",
        "tactics-srv" => "Tactics",
        "hid-udp-srv" => "HID UDP",
        "resource-monitor" => "Resources",
        "logtotcp" => "TCP logs",
        "ai-task" => "AI",
        "http-trueosfs" => "File HTTP",
        "trueosfs-rw-probe" => "File probe",
        "unix-fd-probe" => "Unix FD probe",
        "silk-service" => "Silk",
        "app-vm-run-queue" => "App queue",
        "bp-autostart" => "App autostart",
        "ws-time" => "WebSocket time",
        "usb-controller-tasks" => "USB controllers",
        "lan-discovery" => "LAN discovery",
        "esp-gate-registry" => "ESP registry",
        "esp-piano-audio" => "ESP piano",
        "esp-piano-udp" => "Piano UDP",
        "ftp-server" => "FTP",
        "tga" => "TGA",
        "intel-cursor-service" => "Hardware cursor",
        "hw_pic_service" => "Picture engine",
        "hw_vid_probe_task" => "Video probe",
        "hw_logo_present_task" => "Boot logo",
        "i226-diagnostic-display" => "I226 display",
        "virtio-gpu-ui" => "VirtIO GPU",
        "intel-hda-audio-demo" => "HDA demo",
        "raple-service" => "RAPL power",
        "thermal-service" => "Thermals",
        "html_fetch_service" => "HTML fetch",
        "asset_shack_service" => "Asset fetch",
        "truesurfer-parse-pool" => "Parser pool",
        "ui3-service" => "Desktop UI",
        "ui3-orbits" => "UI orbits",
        "tinyaudio_service" => "TinyAudio",
        "tinyaudio-live-http" => "Audio HTTP",
        "trueosfs-ready-hook" => "File ready hook",
        "net-tcp-shell" => "TCP shell",
        "atomic_bomb" => "Atomic probe",
        _ => return fallback_title(name),
    };
    known.to_string()
}

fn fallback_title(name: &str) -> String {
    let mut words = name
        .split(['-', '_'])
        .filter(|word| !matches!(*word, "service" | "task" | "srv"))
        .take(2);
    let first = words.next().unwrap_or(name);
    let second = words.next();
    let mut title = first.to_ascii_uppercase();
    if first.len() > 4 {
        title = format!("{}{}", &first[..1].to_ascii_uppercase(), &first[1..]);
    }
    if let Some(second) = second {
        title.push(' ');
        title.push_str(second);
    }
    title
}

fn router() -> Router {
    Router::new()
        .route("/", get(handle_index))
        .route("/index.html", get(handle_index))
        .route("/tailwind.css", get(handle_tailwind_css))
        .route("/healthz", get(handle_healthz))
        .route("/api/healthz", get(handle_healthz))
        .route("/api/system-services/snapshot", get(handle_snapshot))
        .route("/api/services/snapshot", get(handle_snapshot))
}

async fn http_runtime() -> Result<(), io::Error> {
    let app = router();
    let addr = SocketAddr::from(([0, 0, 0, 0], HTTP_TCP_PORT));
    loop {
        let listener = match tokio::net::TcpListener::bind(addr).await {
            Ok(listener) => listener,
            Err(err) => {
                HTTP_PORT.store(0, Ordering::Release);
                logl::log(
                    level::WARN,
                    format_args!("system-services: bind {} failed {}", addr, err),
                );
                time::sleep(Duration::from_millis(BIND_RETRY_MS)).await;
                continue;
            }
        };

        HTTP_PORT.store(addr.port(), Ordering::Release);
        logl::log(
            level::INFO,
            format_args!("system-services: listening on http://{}/", addr),
        );
        let listener = listener.tap_io(|_| logl::log(level::INFO, "system-services: tcp accepted"));
        let result = axum::serve(listener, app).await;
        HTTP_PORT.store(0, Ordering::Release);
        return result;
    }
}

fn main() {
    logl::log(level::INFO, "system-services: blueprint start");
    let runtime = match runtime::current_thread_net().build() {
        Ok(runtime) => runtime,
        Err(err) => {
            logl::log(
                level::ERROR,
                format_args!("system-services: runtime build failed {}", err),
            );
            return;
        }
    };
    let local = tokio::task::LocalSet::new();
    local.block_on(&runtime, async {
        if let Err(err) = http_runtime().await {
            logl::log(
                level::ERROR,
                format_args!("system-services: runtime failed {:?}", err),
            );
        }
    });
    platform::poll_once();
}
