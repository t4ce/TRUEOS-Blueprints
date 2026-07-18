// trueos-blueprint: features=["tokio-net-probe"]

extern crate alloc;

use alloc::{
    collections::VecDeque,
    format,
    string::{String, ToString},
    sync::Arc,
    vec,
    vec::Vec,
};
use core::sync::atomic::{AtomicU16, Ordering};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};

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
    serve::ListenerExt,
};
use serde::{Deserialize, Serialize};
use trueos::{
    clock, logl,
    logl::level,
    platform::{self, io},
    runtime,
    time::{self, Duration, Instant},
    tokio::{
        self,
        io::{AsyncReadExt, AsyncWriteExt},
        net::{TcpStream, UdpSocket},
        sync::RwLock,
    },
    vnet,
};
use trueos_esp::{gate, swarm};

const SWARM_HTTP_TCP_PORT: u16 = 12;
const SWARM_HTTP_BIND_RETRY_MS: u64 = 1_000;
const SWARM_DISCOVERY_RELAY_UDP_PORT: u16 = 32_345;
const SWARM_DISCOVERY_RELAY_MAGIC: &[u8; 4] = b"SRD1";
const SWARM_REGISTRY_MAX_DEVICES: usize = 64;
const SWARM_DEVICE_STALE_MS: u64 = 60_000;
const SWARM_STATUS_POLL_MS: u64 = 1_000;
const SWARM_CONTROL_TIMEOUT_MS: u64 = 3_000;
const SWARM_CONTROL_MAX_RX: usize = 16 * 1024;
const SWARM_EVENT_LIMIT: usize = 64;
const SWARM_TARGET_FILENAME: &str = "app.py";
const SWARM_INDEX_HTML: &str = include_str!("index.html");
const TRUEOS_TAILWIND_CSS: &str = include_str!("tailwind.css");
const SWARM_CSS: &str = include_str!("swarm.css");

static SWARM_HTTP_PORT: AtomicU16 = AtomicU16::new(0);

struct SketchSpec {
    id: &'static str,
    source_name: &'static str,
    description: &'static str,
    body: &'static [u8],
}

const SWARM_SKETCHES: &[SketchSpec] = &[
    SketchSpec {
        id: "led",
        source_name: "led.py",
        description: "Basic addressable LED test",
        body: include_bytes!("trueos-esp/iot/led.py"),
    },
    SketchSpec {
        id: "led2",
        source_name: "led2.py",
        description: "Second LED animation",
        body: include_bytes!("trueos-esp/iot/led2.py"),
    },
    SketchSpec {
        id: "led4",
        source_name: "led4.py",
        description: "Four-channel LED sketch",
        body: include_bytes!("trueos-esp/iot/led4.py"),
    },
    SketchSpec {
        id: "pink-fade",
        source_name: "led_pinkfade.py",
        description: "Pink fade LED animation",
        body: include_bytes!("trueos-esp/iot/led_pinkfade.py"),
    },
    SketchSpec {
        id: "tmc2226-stepper",
        source_name: "C3_tmc2226_stepper.py",
        description: "ESP32-C3 TMC2226 stepper controller",
        body: include_bytes!("trueos-esp/iot/C3_tmc2226_stepper.py"),
    },
];

#[derive(Clone)]
struct AppState {
    shared: Arc<RwLock<SwarmState>>,
    started_at: Instant,
}

struct SwarmState {
    registry: gate::DeviceRegistry,
    change_seq: u64,
    discovery_packets: u64,
    discovery_online: bool,
    discovery_error: Option<String>,
    events: VecDeque<EventRow>,
}

impl SwarmState {
    fn new() -> Self {
        Self {
            registry: gate::DeviceRegistry::new(SWARM_REGISTRY_MAX_DEVICES),
            change_seq: 1,
            discovery_packets: 0,
            discovery_online: false,
            discovery_error: None,
            events: VecDeque::new(),
        }
    }

    fn note_change(&mut self) {
        self.change_seq = self.change_seq.wrapping_add(1).max(1);
    }

    fn push_event(&mut self, at_ms: u64, level: &'static str, message: String) {
        while self.events.len() >= SWARM_EVENT_LIMIT {
            let _ = self.events.pop_front();
        }
        self.events.push_back(EventRow {
            at_ms,
            level,
            message,
        });
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct EventRow {
    at_ms: u64,
    level: &'static str,
    message: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SnapshotResponse {
    schema: &'static str,
    generated_at_ms: u64,
    change_seq: u64,
    service: ServiceRow,
    discovery: DiscoveryRow,
    devices: Vec<DeviceRow>,
    sketches: Vec<SketchRow>,
    events: Vec<EventRow>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ServiceRow {
    name: &'static str,
    http_port: Option<u16>,
    bind: &'static str,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct DiscoveryRow {
    online: bool,
    public_udp_port: u16,
    relay_udp_port: u16,
    esp_http_port: u16,
    trueos_peer_tcp_port: u16,
    packet_count: u64,
    error: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct DeviceRow {
    handle: u32,
    class: &'static str,
    tag: String,
    ip: Option<String>,
    endpoint: String,
    service_port: u16,
    node_id: String,
    caps: String,
    connected_at_ms: u64,
    last_activity_ms: u64,
    age_ms: u64,
    status: Option<DeviceStatusRow>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct DeviceStatusRow {
    threading_available: bool,
    app_exists: bool,
    running: bool,
    last_status: String,
    last_error: String,
    last_started_ms: Option<u64>,
    last_finished_ms: Option<u64>,
    last_heartbeat_ms: Option<u64>,
    heartbeat_count: u64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SketchRow {
    id: &'static str,
    source_name: &'static str,
    target_name: &'static str,
    description: &'static str,
    bytes: usize,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct UploadRequest {
    sketch_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ManualDeviceRequest {
    address: String,
    #[serde(default)]
    port: Option<u16>,
}

fn monotonic_ms(state: &AppState) -> u64 {
    state
        .started_at
        .elapsed()
        .as_millis()
        .min(u128::from(u64::MAX)) as u64
}

fn generated_at_ms(state: &AppState) -> u64 {
    let unix_ms = clock::ntp_current_unix_seconds().saturating_mul(1_000);
    if unix_ms == 0 {
        monotonic_ms(state)
    } else {
        unix_ms
    }
}

fn current_port() -> Option<u16> {
    match SWARM_HTTP_PORT.load(Ordering::Acquire) {
        0 => None,
        port => Some(port),
    }
}

fn status_code(status: u16) -> StatusCode {
    StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR)
}

fn response(status: u16, content_type: &'static str, body: Vec<u8>, no_store: bool) -> Response {
    Response::builder()
        .status(status_code(status))
        .header(CONTENT_TYPE, content_type)
        .header(CONTENT_LENGTH, body.len().to_string())
        .header(
            CACHE_CONTROL,
            if no_store { "no-store" } else { "no-cache" },
        )
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

fn error_response(status: u16, message: impl Into<String>) -> Response {
    json_response(
        status,
        &serde_json::json!({ "ok": false, "error": message.into() }),
    )
}

async fn handle_index() -> Response {
    text_response(200, "text/html; charset=utf-8", SWARM_INDEX_HTML)
}

async fn handle_tailwind_css() -> Response {
    text_response(200, "text/css; charset=utf-8", TRUEOS_TAILWIND_CSS)
}

async fn handle_swarm_css() -> Response {
    text_response(200, "text/css; charset=utf-8", SWARM_CSS)
}

async fn handle_healthz(State(state): State<AppState>) -> Response {
    let shared = state.shared.read().await;
    json_response(
        200,
        &serde_json::json!({
            "ok": true,
            "service": "swarm-http",
            "port": current_port(),
            "discoveryOnline": shared.discovery_online,
            "devices": shared.registry.len(),
        }),
    )
}

async fn handle_snapshot(State(state): State<AppState>) -> Response {
    let now_ms = monotonic_ms(&state);
    let shared = state.shared.read().await;
    let devices = shared
        .registry
        .snapshot()
        .iter()
        .map(|snapshot| device_row(snapshot, now_ms))
        .collect();
    let sketches = SWARM_SKETCHES
        .iter()
        .map(|sketch| SketchRow {
            id: sketch.id,
            source_name: sketch.source_name,
            target_name: SWARM_TARGET_FILENAME,
            description: sketch.description,
            bytes: sketch.body.len(),
        })
        .collect();

    json_response(
        200,
        &SnapshotResponse {
            schema: "trueos.swarm.v1",
            generated_at_ms: generated_at_ms(&state),
            change_seq: shared.change_seq,
            service: ServiceRow {
                name: "swarm-http",
                http_port: current_port(),
                bind: "0.0.0.0",
            },
            discovery: DiscoveryRow {
                online: shared.discovery_online,
                public_udp_port: gate::ESP_UDP_BROADCAST_PORT,
                relay_udp_port: SWARM_DISCOVERY_RELAY_UDP_PORT,
                esp_http_port: gate::ESP_HTTP_UPLOAD_PORT,
                trueos_peer_tcp_port: gate::TRUEOS_PEER_TCP_PORT,
                packet_count: shared.discovery_packets,
                error: shared.discovery_error.clone(),
            },
            devices,
            sketches,
            events: shared.events.iter().cloned().rev().collect(),
        },
    )
}

fn device_row(snapshot: &gate::DeviceSnapshot, now_ms: u64) -> DeviceRow {
    let (ip, endpoint) = match snapshot.ip {
        Some(gate::DeviceIp::V4(addr)) => {
            let ip = format!("{}.{}.{}.{}", addr[0], addr[1], addr[2], addr[3]);
            let endpoint = format!("{}:{}", ip, snapshot.service_port);
            (Some(ip), endpoint)
        }
        Some(gate::DeviceIp::V6(addr)) => {
            let ip = Ipv6Display(addr).to_string();
            let endpoint = format!("[{}]:{}", ip, snapshot.service_port);
            (Some(ip), endpoint)
        }
        None => (None, format!("pending:{}", snapshot.service_port)),
    };
    let status = snapshot.status.as_ref().map(|status| DeviceStatusRow {
        threading_available: status.threading_available,
        app_exists: status.app_exists,
        running: status.running,
        last_status: status.last_status.as_str().to_string(),
        last_error: status.last_error.as_str().to_string(),
        last_started_ms: status.last_started_ms,
        last_finished_ms: status.last_finished_ms,
        last_heartbeat_ms: status.last_heartbeat_ms,
        heartbeat_count: status.heartbeat_count,
    });

    DeviceRow {
        handle: snapshot.handle.0,
        class: match snapshot.class {
            gate::DeviceClass::EspUploader => "esp",
            gate::DeviceClass::TrueOsHost => "trueos",
        },
        tag: snapshot.tag.as_str().to_string(),
        ip,
        endpoint,
        service_port: snapshot.service_port,
        node_id: format!("0x{:016X}", snapshot.node_id),
        caps: format!("0x{:08X}", snapshot.caps),
        connected_at_ms: snapshot.connected_at_ms,
        last_activity_ms: snapshot.last_activity_ms,
        age_ms: now_ms.saturating_sub(snapshot.last_activity_ms),
        status,
    }
}

struct Ipv6Display([u8; 16]);

impl core::fmt::Display for Ipv6Display {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let a = self.0;
        write!(
            f,
            "{:x}:{:x}:{:x}:{:x}:{:x}:{:x}:{:x}:{:x}",
            u16::from_be_bytes([a[0], a[1]]),
            u16::from_be_bytes([a[2], a[3]]),
            u16::from_be_bytes([a[4], a[5]]),
            u16::from_be_bytes([a[6], a[7]]),
            u16::from_be_bytes([a[8], a[9]]),
            u16::from_be_bytes([a[10], a[11]]),
            u16::from_be_bytes([a[12], a[13]]),
            u16::from_be_bytes([a[14], a[15]])
        )
    }
}

async fn handle_manual_device(
    State(state): State<AppState>,
    Json(request): Json<ManualDeviceRequest>,
) -> Response {
    let address = match request.address.trim().parse::<Ipv4Addr>() {
        Ok(address) => address,
        Err(_) => return error_response(400, "address must be an IPv4 address"),
    };
    let port = request.port.unwrap_or(gate::ESP_HTTP_UPLOAD_PORT);
    let now_ms = monotonic_ms(&state);
    let mut shared = state.shared.write().await;
    let is_new = shared
        .registry
        .upsert_heartbeat_v4(address.octets(), port, now_ms);
    shared.note_change();
    shared.push_event(
        now_ms,
        "info",
        format!(
            "{} ESP endpoint {}:{}",
            if is_new { "added" } else { "refreshed" },
            address,
            port
        ),
    );
    let handle = gate::device_handle_v4(address.octets()).0;
    json_response(200, &serde_json::json!({ "ok": true, "handle": handle }))
}

async fn handle_remove_device(State(state): State<AppState>, Path(handle): Path<u32>) -> Response {
    let now_ms = monotonic_ms(&state);
    let mut shared = state.shared.write().await;
    if !shared.registry.remove_device(vnet::NetHandle(handle)) {
        return error_response(404, "device is not in the swarm registry");
    }
    shared.note_change();
    shared.push_event(now_ms, "info", format!("removed device {}", handle));
    json_response(200, &serde_json::json!({ "ok": true }))
}

async fn handle_upload(
    State(state): State<AppState>,
    Path(handle): Path<u32>,
    Json(request): Json<UploadRequest>,
) -> Response {
    let Some(sketch) = SWARM_SKETCHES
        .iter()
        .find(|entry| entry.id == request.sketch_id)
    else {
        return error_response(404, "unknown sketch id");
    };
    let Some(snapshot) = snapshot_for_handle(&state, handle).await else {
        return error_response(404, "device is not in the swarm registry");
    };
    if snapshot.class != gate::DeviceClass::EspUploader {
        return error_response(409, "uploads are only available for ESP nodes");
    }

    let result = async {
        device_http_request(
            &snapshot,
            "POST",
            swarm::ESP_UPLOAD_PATH,
            &[
                ("Content-Type", "application/octet-stream"),
                ("X-Filename", SWARM_TARGET_FILENAME),
            ],
            sketch.body,
        )
        .await?;
        device_http_request(&snapshot, "POST", swarm::ESP_RUN_PATH, &[], &[]).await?;
        Ok::<(), String>(())
    }
    .await;

    let now_ms = monotonic_ms(&state);
    let mut shared = state.shared.write().await;
    match result {
        Ok(()) => {
            shared.push_event(
                now_ms,
                "ok",
                format!(
                    "uploaded {} as {} to device {} and started it",
                    sketch.source_name, SWARM_TARGET_FILENAME, handle
                ),
            );
            json_response(
                200,
                &serde_json::json!({
                    "ok": true,
                    "handle": handle,
                    "sourceName": sketch.source_name,
                    "targetName": SWARM_TARGET_FILENAME,
                    "bytes": sketch.body.len(),
                    "runRequested": true,
                }),
            )
        }
        Err(error) => {
            shared.push_event(
                now_ms,
                "error",
                format!("upload to device {} failed: {}", handle, error),
            );
            error_response(502, error)
        }
    }
}

async fn handle_restart(State(state): State<AppState>, Path(handle): Path<u32>) -> Response {
    let Some(snapshot) = snapshot_for_handle(&state, handle).await else {
        return error_response(404, "device is not in the swarm registry");
    };
    if snapshot.class != gate::DeviceClass::EspUploader {
        return error_response(409, "restart is only available for ESP nodes");
    }

    let result = device_http_request(&snapshot, "POST", swarm::ESP_RESTART_PATH, &[], &[]).await;
    let now_ms = monotonic_ms(&state);
    let mut shared = state.shared.write().await;
    let removed = shared.registry.remove_device(snapshot.handle);
    if removed {
        shared.note_change();
    }
    match result {
        Ok(_) => {
            shared.push_event(
                now_ms,
                "ok",
                format!("restart requested for device {}", handle),
            );
            json_response(200, &serde_json::json!({ "ok": true, "removed": removed }))
        }
        Err(error) => {
            shared.push_event(
                now_ms,
                "error",
                format!("restart for device {} failed: {}", handle, error),
            );
            error_response(502, error)
        }
    }
}

async fn snapshot_for_handle(state: &AppState, handle: u32) -> Option<gate::DeviceSnapshot> {
    state
        .shared
        .read()
        .await
        .registry
        .snapshot_for(vnet::NetHandle(handle))
}

async fn device_http_request(
    snapshot: &gate::DeviceSnapshot,
    method: &str,
    path: &str,
    headers: &[(&str, &str)],
    body: &[u8],
) -> Result<Vec<u8>, String> {
    let addr = match snapshot.ip {
        Some(gate::DeviceIp::V4(addr)) => SocketAddr::new(
            IpAddr::V4(Ipv4Addr::new(addr[0], addr[1], addr[2], addr[3])),
            snapshot.service_port,
        ),
        Some(gate::DeviceIp::V6(_)) => {
            return Err("IPv6 ESP control is not implemented".to_string());
        }
        None => return Err("device has no network address".to_string()),
    };
    let host = addr.to_string();
    let operation = async {
        let mut stream = TcpStream::connect(addr)
            .await
            .map_err(|error| format!("connect {} failed: {}", addr, error))?;
        let mut request = format!(
            "{} {} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\nContent-Length: {}\r\n",
            method,
            path,
            host,
            body.len()
        );
        for (name, value) in headers {
            request.push_str(name);
            request.push_str(": ");
            request.push_str(value);
            request.push_str("\r\n");
        }
        request.push_str("\r\n");
        stream
            .write_all(request.as_bytes())
            .await
            .map_err(|error| format!("write request failed: {}", error))?;
        if !body.is_empty() {
            stream
                .write_all(body)
                .await
                .map_err(|error| format!("write body failed: {}", error))?;
        }

        let mut bytes = Vec::new();
        let mut scratch = [0u8; 1_024];
        loop {
            let count = stream
                .read(&mut scratch)
                .await
                .map_err(|error| format!("read response failed: {}", error))?;
            if count == 0 {
                break;
            }
            if bytes.len().saturating_add(count) > SWARM_CONTROL_MAX_RX {
                return Err("ESP response exceeded receive limit".to_string());
            }
            bytes.extend_from_slice(&scratch[..count]);
        }
        parse_http_response(bytes)
    };

    match time::timeout(Duration::from_millis(SWARM_CONTROL_TIMEOUT_MS), operation).await {
        Ok(result) => result,
        Err(_) => Err(format!(
            "{} {} timed out after {} ms",
            method, path, SWARM_CONTROL_TIMEOUT_MS
        )),
    }
}

fn parse_http_response(bytes: Vec<u8>) -> Result<Vec<u8>, String> {
    let Some(split) = bytes.windows(4).position(|window| window == b"\r\n\r\n") else {
        return Err("ESP returned a malformed HTTP response".to_string());
    };
    let header = core::str::from_utf8(&bytes[..split])
        .map_err(|_| "ESP returned a non-UTF8 HTTP header".to_string())?;
    let status = header
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|value| value.parse::<u16>().ok())
        .ok_or_else(|| "ESP response did not contain an HTTP status".to_string())?;
    let body = bytes[split + 4..].to_vec();
    if !(200..300).contains(&status) {
        let detail = core::str::from_utf8(&body)
            .unwrap_or("non-text response")
            .trim();
        return Err(format!("ESP returned HTTP {}: {}", status, detail));
    }
    Ok(body)
}

async fn discovery_loop(state: AppState) {
    let addr = SocketAddr::from(([0, 0, 0, 0], SWARM_DISCOVERY_RELAY_UDP_PORT));
    loop {
        let socket = match UdpSocket::bind(addr).await {
            Ok(socket) => socket,
            Err(error) => {
                let message = format!("bind {} failed: {}", addr, error);
                logl::log(level::WARN, format_args!("swarm-discovery: {}", message));
                let mut shared = state.shared.write().await;
                shared.discovery_online = false;
                shared.discovery_error = Some(message);
                drop(shared);
                time::sleep(Duration::from_millis(SWARM_HTTP_BIND_RETRY_MS)).await;
                continue;
            }
        };
        {
            let mut shared = state.shared.write().await;
            shared.discovery_online = true;
            shared.discovery_error = None;
            shared.push_event(
                monotonic_ms(&state),
                "info",
                format!(
                    "discovery relay listening on UDP {}",
                    SWARM_DISCOVERY_RELAY_UDP_PORT
                ),
            );
        }
        logl::log(
            level::INFO,
            format_args!("swarm-discovery: relay listening on {}", addr),
        );

        let mut buffer = [0u8; 2_048];
        loop {
            match socket.recv_from(&mut buffer).await {
                Ok((len, _)) => handle_discovery_packet(&state, &buffer[..len]).await,
                Err(error) => {
                    let message = format!("receive failed: {}", error);
                    let mut shared = state.shared.write().await;
                    shared.discovery_online = false;
                    shared.discovery_error = Some(message.clone());
                    shared.push_event(monotonic_ms(&state), "error", message);
                    break;
                }
            }
        }
    }
}

async fn handle_discovery_packet(state: &AppState, frame: &[u8]) {
    let Some((from, payload)) = decode_discovery_relay(frame) else {
        return;
    };
    let now_ms = monotonic_ms(state);
    let mut shared = state.shared.write().await;
    shared.discovery_packets = shared.discovery_packets.saturating_add(1);

    if payload == gate::ESP_SWARM_HEARTBEAT {
        let is_new =
            shared
                .registry
                .upsert_heartbeat_v4(from.addr, gate::ESP_HTTP_UPLOAD_PORT, now_ms);
        if is_new {
            shared.note_change();
            shared.push_event(
                now_ms,
                "ok",
                format!(
                    "discovered ESP at {}.{}.{}.{}:{}",
                    from.addr[0],
                    from.addr[1],
                    from.addr[2],
                    from.addr[3],
                    gate::ESP_HTTP_UPLOAD_PORT
                ),
            );
        }
        return;
    }

    if let Some(advertisement) = gate::parse_trueos_host_advertisement(from, payload) {
        let is_new = shared.registry.upsert_trueos_host_v4(
            advertisement.from.addr,
            advertisement.peer_tcp_port,
            advertisement.node_id,
            advertisement.caps,
            now_ms,
        );
        if is_new {
            shared.note_change();
            shared.push_event(
                now_ms,
                "info",
                format!(
                    "discovered TRUEOS peer node=0x{:016X}",
                    advertisement.node_id
                ),
            );
        }
    }
}

fn decode_discovery_relay(frame: &[u8]) -> Option<(vnet::EndpointV4, &[u8])> {
    if frame.len() < 10 || &frame[..4] != SWARM_DISCOVERY_RELAY_MAGIC {
        return None;
    }
    let from = vnet::EndpointV4::new(
        [frame[4], frame[5], frame[6], frame[7]],
        u16::from_be_bytes([frame[8], frame[9]]),
    );
    Some((from, &frame[10..]))
}

async fn status_loop(state: AppState) {
    let mut poll_index = 0usize;
    loop {
        let now_ms = monotonic_ms(&state);
        let candidate = {
            let mut shared = state.shared.write().await;
            let snapshots = shared.registry.snapshot();
            let stale = snapshots
                .iter()
                .filter(|snapshot| {
                    now_ms.saturating_sub(snapshot.last_activity_ms) > SWARM_DEVICE_STALE_MS
                })
                .map(|snapshot| snapshot.handle)
                .collect::<Vec<_>>();
            for handle in stale {
                if shared.registry.remove_device(handle) {
                    shared.note_change();
                    shared.push_event(
                        now_ms,
                        "warn",
                        format!("expired inactive device {}", handle.0),
                    );
                }
            }
            let esp = shared
                .registry
                .snapshot()
                .into_iter()
                .filter(|snapshot| snapshot.class == gate::DeviceClass::EspUploader)
                .collect::<Vec<_>>();
            if esp.is_empty() {
                None
            } else {
                let snapshot = esp[poll_index % esp.len()].clone();
                poll_index = poll_index.wrapping_add(1);
                Some(snapshot)
            }
        };

        if let Some(snapshot) = candidate
            && let Ok(body) =
                device_http_request(&snapshot, "GET", swarm::ESP_STATUS_PATH, &[], &[]).await
            && let Some(status) = swarm::parse_status_snapshot(&body)
        {
            let now_ms = monotonic_ms(&state);
            let mut shared = state.shared.write().await;
            if let Some(event) = shared
                .registry
                .update_status(snapshot.handle, status, now_ms)
            {
                shared.note_change();
                shared.push_event(
                    now_ms,
                    "info",
                    format!(
                        "device {} status={} running={}",
                        event.handle.0,
                        event.current.last_status.as_str(),
                        event.current.running
                    ),
                );
            }
        }

        time::sleep(Duration::from_millis(SWARM_STATUS_POLL_MS)).await;
    }
}

fn router(state: AppState) -> Router {
    Router::new()
        .route("/", get(handle_index))
        .route("/index.html", get(handle_index))
        .route("/tailwind.css", get(handle_tailwind_css))
        .route("/swarm.css", get(handle_swarm_css))
        .route("/healthz", get(handle_healthz))
        .route("/api/healthz", get(handle_healthz))
        .route("/api/swarm/snapshot", get(handle_snapshot))
        .route("/api/swarm/devices", post(handle_manual_device))
        .route("/api/swarm/devices/{handle}/upload", post(handle_upload))
        .route("/api/swarm/devices/{handle}/restart", post(handle_restart))
        .route(
            "/api/swarm/devices/{handle}/remove",
            post(handle_remove_device),
        )
        .with_state(state)
}

async fn swarm_http_runtime() -> Result<(), io::Error> {
    let state = AppState {
        shared: Arc::new(RwLock::new(SwarmState::new())),
        started_at: Instant::now(),
    };
    tokio::task::spawn_local(discovery_loop(state.clone()));
    tokio::task::spawn_local(status_loop(state.clone()));

    let app = router(state);
    let addr = SocketAddr::from(([0, 0, 0, 0], SWARM_HTTP_TCP_PORT));
    loop {
        let listener = match tokio::net::TcpListener::bind(addr).await {
            Ok(listener) => listener,
            Err(error) => {
                SWARM_HTTP_PORT.store(0, Ordering::Release);
                logl::log(
                    level::WARN,
                    format_args!("swarm-http: bind {} failed {}", addr, error),
                );
                time::sleep(Duration::from_millis(SWARM_HTTP_BIND_RETRY_MS)).await;
                continue;
            }
        };
        SWARM_HTTP_PORT.store(addr.port(), Ordering::Release);
        logl::log(
            level::INFO,
            format_args!("swarm-http: axum listening on http://{}/", addr),
        );
        let listener = listener.tap_io(|_| logl::log(level::INFO, "swarm-http: tcp accepted"));
        let result = axum::serve(listener, app).await;
        SWARM_HTTP_PORT.store(0, Ordering::Release);
        return result;
    }
}

fn main() {
    logl::log(level::INFO, "swarm-http: blueprint start");
    let runtime = match runtime::current_thread_net().build() {
        Ok(runtime) => runtime,
        Err(error) => {
            logl::log(
                level::ERROR,
                format_args!("swarm-http: runtime build failed {}", error),
            );
            return;
        }
    };
    let local = tokio::task::LocalSet::new();
    local.block_on(&runtime, async {
        if let Err(error) = swarm_http_runtime().await {
            logl::log(
                level::ERROR,
                format_args!("swarm-http: runtime failed {:?}", error),
            );
        }
    });
    platform::poll_once();
}
