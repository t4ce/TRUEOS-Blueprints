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
    clock, logl,
    logl::level,
    pci,
    platform::{self, io},
    rapl, runtime, thermal,
    time::{self, Duration},
    tokio::{self, net::SocketAddr},
};

const WEBDEVICES_HTTP_TCP_PORT: u16 = 10;
const WEBDEVICES_BIND_RETRY_MS: u64 = 1000;
const RAPL_UI_HISTORY_BYTES: usize = 256 * 1024;
const RAPL_UI_HISTORY_POINTS: usize = 240;
const WEBDEVICES_INDEX_HTML: &str = include_str!("index.html");
const TRUEOS_TAILWIND_CSS: &str = include_str!("tailwind.css");

static WEBDEVICES_HTTP_PORT: AtomicU16 = AtomicU16::new(0);

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct HardwareSnapshot {
    schema: &'static str,
    generated_at_s: u64,
    service: ServiceSnapshot,
    rapl: RaplSnapshot,
    thermal: ThermalSnapshot,
    pci: DeviceGroup,
    usb: UsbSnapshot,
    note: &'static str,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ServiceSnapshot {
    name: &'static str,
    port: Option<u16>,
    bind: &'static str,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct RaplSnapshot {
    vlayer_available: bool,
    sample_valid: bool,
    snapshot_bytes: usize,
    history_bytes: usize,
    max_history_bytes: usize,
    domains: Vec<RaplDomainRow>,
    history: Vec<RaplHistoryPoint>,
    latest_text: String,
    unavailable_reason: Option<&'static str>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct RaplDomainRow {
    domain: String,
    description: String,
    msr: String,
    raw: String,
    joules: Option<f64>,
    delta_joules: Option<f64>,
    watts: Option<f64>,
    state: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct RaplHistoryPoint {
    ms: u64,
    dt_ms: u64,
    update: u64,
    valid: bool,
    package_joules: Option<f64>,
    core_joules: Option<f64>,
    graphics_joules: Option<f64>,
    dram_joules: Option<f64>,
    platform_joules: Option<f64>,
    package_watts: Option<f64>,
    core_watts: Option<f64>,
    graphics_watts: Option<f64>,
    dram_watts: Option<f64>,
    platform_watts: Option<f64>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ThermalSnapshot {
    vlayer_available: bool,
    sample_valid: bool,
    snapshot_bytes: usize,
    update_count: Option<u64>,
    last_update_ms: Option<u64>,
    tj_max_celsius: Option<u64>,
    total_cpus: Option<u64>,
    online_cpus: Option<u64>,
    completed_cpus: Option<u64>,
    package: Option<ThermalPackageRow>,
    cores: Vec<ThermalCoreRow>,
    latest_text: String,
    unavailable_reason: Option<&'static str>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ThermalPackageRow {
    domain: String,
    raw: String,
    temp_celsius: Option<i64>,
    delta_to_tjmax: Option<u64>,
    valid: bool,
    state: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ThermalCoreRow {
    slot: u64,
    online: bool,
    source: String,
    age_ms: Option<u64>,
    kind: String,
    spawned: Option<u64>,
    ready: Option<u64>,
    hlt_now: Option<bool>,
    hlt_active80: Option<u64>,
    perf_ratio: Option<u64>,
    effective_permille: Option<u64>,
    raw: String,
    temp_celsius: Option<i64>,
    delta_to_tjmax: Option<u64>,
    valid: bool,
    thermal: Option<bool>,
    prochot: Option<bool>,
    critical: Option<bool>,
    power_limit: Option<bool>,
    current_limit: Option<bool>,
    cross_domain: Option<bool>,
    state: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct DeviceGroup {
    count: usize,
    devices: Vec<serde_json::Value>,
    unavailable_reason: &'static str,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct UsbSnapshot {
    count: usize,
    device_count: usize,
    controller_count: usize,
    topology_count: usize,
    devices: Vec<serde_json::Value>,
    controllers: Vec<serde_json::Value>,
    topology: Vec<serde_json::Value>,
    unavailable_reason: &'static str,
}

struct PciDeviceDraft {
    id: String,
    bdf: String,
    vendor_id: String,
    device_id: String,
    class_code: String,
    subclass: String,
    prog_if: String,
    class_name: String,
    role: String,
    command: String,
    status: String,
    name: String,
    bars: Vec<serde_json::Value>,
}

fn current_port() -> Option<u16> {
    match WEBDEVICES_HTTP_PORT.load(Ordering::Acquire) {
        0 => None,
        port => Some(port),
    }
}

fn status_code(status: u16) -> StatusCode {
    StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR)
}

fn response(status: u16, content_type: &'static str, body: Vec<u8>, no_store: bool) -> Response {
    let mut builder = Response::builder()
        .status(status_code(status))
        .header(CONTENT_TYPE, content_type)
        .header(CONTENT_LENGTH, body.len().to_string());
    builder = if no_store {
        builder.header(CACHE_CONTROL, "no-store")
    } else {
        builder.header(CACHE_CONTROL, "no-cache")
    };
    builder
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
    logl::log(level::INFO, "webdevices-http: GET /");
    text_response(200, "text/html; charset=utf-8", WEBDEVICES_INDEX_HTML)
}

async fn handle_tailwind_css() -> Response {
    text_response(200, "text/css; charset=utf-8", TRUEOS_TAILWIND_CSS)
}

async fn handle_healthz() -> Response {
    json_response(
        200,
        &serde_json::json!({
            "ok": true,
            "service": "webdevices-http",
            "port": current_port(),
        }),
    )
}

fn rapl_payload() -> RaplSnapshot {
    match rapl::snapshot_text() {
        Ok(latest_text) if !latest_text.is_empty() => {
            let sample_valid = latest_text
                .lines()
                .any(|line| line.trim() == "sample_valid=true");
            let domains = parse_rapl_domains(&latest_text);
            let history_text = rapl::history_tail_text(RAPL_UI_HISTORY_BYTES).unwrap_or_default();
            let history = parse_rapl_history(&history_text, RAPL_UI_HISTORY_POINTS);
            RaplSnapshot {
                vlayer_available: true,
                sample_valid,
                snapshot_bytes: latest_text.len(),
                history_bytes: rapl::history_len().unwrap_or(0),
                max_history_bytes: rapl::MAX_HISTORY_BYTES,
                domains,
                history,
                latest_text,
                unavailable_reason: None,
            }
        }
        Ok(_) | Err(_) => RaplSnapshot {
            vlayer_available: false,
            sample_valid: false,
            snapshot_bytes: 0,
            history_bytes: 0,
            max_history_bytes: rapl::MAX_HISTORY_BYTES,
            domains: Vec::new(),
            history: Vec::new(),
            latest_text: String::new(),
            unavailable_reason: Some("RAPL vlayer surface is unavailable"),
        },
    }
}

fn thermal_payload() -> ThermalSnapshot {
    match thermal::snapshot_text() {
        Ok(latest_text) if !latest_text.is_empty() => {
            let sample_valid = latest_text
                .lines()
                .any(|line| line.trim() == "sample_valid=true");
            let (package, cores) = parse_thermal_rows(&latest_text);
            ThermalSnapshot {
                vlayer_available: true,
                sample_valid,
                snapshot_bytes: latest_text.len(),
                update_count: parse_thermal_scalar(&latest_text, "update_count"),
                last_update_ms: parse_thermal_scalar(&latest_text, "last_update_ms"),
                tj_max_celsius: parse_thermal_scalar(&latest_text, "tj_max_celsius"),
                total_cpus: parse_thermal_cpu_scalar(&latest_text, "total"),
                online_cpus: parse_thermal_cpu_scalar(&latest_text, "online"),
                completed_cpus: parse_thermal_cpu_scalar(&latest_text, "completed"),
                package,
                cores,
                latest_text,
                unavailable_reason: None,
            }
        }
        Ok(_) | Err(_) => ThermalSnapshot {
            vlayer_available: false,
            sample_valid: false,
            snapshot_bytes: 0,
            update_count: None,
            last_update_ms: None,
            tj_max_celsius: None,
            total_cpus: None,
            online_cpus: None,
            completed_cpus: None,
            package: None,
            cores: Vec::new(),
            latest_text: String::new(),
            unavailable_reason: Some("thermal vlayer surface is unavailable"),
        },
    }
}

fn parse_thermal_rows(text: &str) -> (Option<ThermalPackageRow>, Vec<ThermalCoreRow>) {
    let mut package = None;
    let mut cores = Vec::new();
    let mut table = "";

    for line in text.lines() {
        let line = line.trim();
        if line
            == "domain,description,msr,raw,temp_c,delta_to_tjmax,valid,thermal,prochot,critical,power_limit,current_limit,cross_domain,state"
        {
            table = "package";
            continue;
        }
        if line
            == "slot,online,source,age_ms,kind,spawned,ready,hlt_now,hlt_active80,hlt_history,perf_ratio,perf_status,aperf_delta,mperf_delta,eff_permille,raw,temp_c,delta_to_tjmax,valid,thermal,prochot,critical,power_limit,current_limit,cross_domain,state"
        {
            table = "cores";
            continue;
        }
        if line.is_empty() || line.contains('=') {
            continue;
        }

        let fields: Vec<&str> = line.split(',').map(str::trim).collect();
        match table {
            "package" if fields.len() >= 14 => {
                package = Some(ThermalPackageRow {
                    domain: fields[0].to_string(),
                    raw: fields[3].to_string(),
                    temp_celsius: parse_optional_i64(fields[4]),
                    delta_to_tjmax: parse_optional_u64(fields[5]),
                    valid: parse_bool(fields[6]).unwrap_or(false),
                    state: fields[13].to_string(),
                });
            }
            "cores" if fields.len() >= 26 => {
                let Some(slot) = parse_u64(fields[0]) else {
                    continue;
                };
                cores.push(ThermalCoreRow {
                    slot,
                    online: parse_bool(fields[1]).unwrap_or(false),
                    source: fields[2].to_string(),
                    age_ms: parse_optional_u64(fields[3]),
                    kind: fields[4].to_string(),
                    spawned: parse_optional_u64(fields[5]),
                    ready: parse_optional_u64(fields[6]),
                    hlt_now: parse_bool(fields[7]),
                    hlt_active80: parse_optional_u64(fields[8]),
                    perf_ratio: parse_optional_u64(fields[10]),
                    effective_permille: parse_optional_u64(fields[14]),
                    raw: fields[15].to_string(),
                    temp_celsius: parse_optional_i64(fields[16]),
                    delta_to_tjmax: parse_optional_u64(fields[17]),
                    valid: parse_bool(fields[18]).unwrap_or(false),
                    thermal: parse_bool(fields[19]),
                    prochot: parse_bool(fields[20]),
                    critical: parse_bool(fields[21]),
                    power_limit: parse_bool(fields[22]),
                    current_limit: parse_bool(fields[23]),
                    cross_domain: parse_bool(fields[24]),
                    state: fields[25].to_string(),
                });
            }
            _ => {}
        }
    }

    (package, cores)
}

fn parse_thermal_scalar(text: &str, key: &str) -> Option<u64> {
    let prefix = format!("{}=", key);
    text.lines()
        .map(str::trim)
        .find_map(|line| line.strip_prefix(&prefix).and_then(parse_optional_u64))
}

fn parse_thermal_cpu_scalar(text: &str, key: &str) -> Option<u64> {
    text.lines()
        .map(str::trim)
        .find(|line| line.starts_with("cpus "))
        .and_then(|line| {
            line.split_whitespace().find_map(|part| {
                let (name, value) = part.split_once('=')?;
                if name == key {
                    parse_optional_u64(value)
                } else {
                    None
                }
            })
        })
}

fn parse_rapl_domains(text: &str) -> Vec<RaplDomainRow> {
    let mut rows = Vec::new();
    let mut in_table = false;
    for line in text.lines() {
        let line = line.trim();
        if line == "domain,description,msr,raw,joules,delta_joules,watts,state" {
            in_table = true;
            continue;
        }
        if !in_table || line.is_empty() {
            continue;
        }

        let fields: Vec<&str> = line.split(',').collect();
        if fields.len() < 8 {
            continue;
        }
        rows.push(RaplDomainRow {
            domain: fields[0].to_string(),
            description: fields[1].to_string(),
            msr: fields[2].to_string(),
            raw: fields[3].to_string(),
            joules: parse_optional_f64(fields[4]),
            delta_joules: parse_optional_f64(fields[5]),
            watts: parse_optional_f64(fields[6]),
            state: fields[7].to_string(),
        });
    }
    rows
}

fn parse_rapl_history(text: &str, max_points: usize) -> Vec<RaplHistoryPoint> {
    let mut points = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        let fields: Vec<&str> = line.split(',').collect();
        if fields.len() < 14 {
            continue;
        }
        let Some(ms) = parse_u64(fields[0]) else {
            continue;
        };

        if max_points != 0 && points.len() == max_points {
            points.remove(0);
        }
        points.push(RaplHistoryPoint {
            ms,
            dt_ms: parse_u64(fields[1]).unwrap_or(0),
            update: parse_u64(fields[2]).unwrap_or(0),
            valid: fields[3].trim() == "1",
            package_joules: parse_optional_f64(fields[4]),
            core_joules: parse_optional_f64(fields[5]),
            graphics_joules: parse_optional_f64(fields[6]),
            dram_joules: parse_optional_f64(fields[7]),
            platform_joules: parse_optional_f64(fields[8]),
            package_watts: parse_optional_f64(fields[9]),
            core_watts: parse_optional_f64(fields[10]),
            graphics_watts: parse_optional_f64(fields[11]),
            dram_watts: parse_optional_f64(fields[12]),
            platform_watts: parse_optional_f64(fields[13]),
        });
    }
    points
}

fn parse_u64(value: &str) -> Option<u64> {
    value.trim().parse::<u64>().ok()
}

fn parse_optional_u64(value: &str) -> Option<u64> {
    let value = value.trim();
    if value.is_empty() || value == "-" {
        return None;
    }
    value.parse::<u64>().ok()
}

fn parse_optional_i64(value: &str) -> Option<i64> {
    let value = value.trim();
    if value.is_empty() || value == "-" {
        return None;
    }
    value.parse::<i64>().ok()
}

fn parse_bool(value: &str) -> Option<bool> {
    match value.trim() {
        "true" | "1" | "yes" => Some(true),
        "false" | "0" | "no" => Some(false),
        _ => None,
    }
}

fn parse_optional_f64(value: &str) -> Option<f64> {
    let value = value.trim();
    if value.is_empty() || value == "-" {
        return None;
    }
    value.parse::<f64>().ok()
}

fn pci_payload() -> DeviceGroup {
    match pci::snapshot_text() {
        Ok(text) if !text.is_empty() => {
            let devices = parse_pci_devices(&text);
            DeviceGroup {
                count: devices.len(),
                devices,
                unavailable_reason: "",
            }
        }
        Ok(_) | Err(_) => DeviceGroup {
            count: 0,
            devices: Vec::new(),
            unavailable_reason: "PCI vlayer snapshot is unavailable",
        },
    }
}

fn usb_payload_from_pci(pci_devices: &[serde_json::Value]) -> UsbSnapshot {
    let mut controllers = Vec::new();
    for device in pci_devices {
        let role = json_str(device, "role");
        let class_name = json_str(device, "className");
        let class_code = json_str(device, "classCode");
        let subclass = json_str(device, "subclass");
        if role != "usb" && !(class_code == "0C" && subclass == "03") && class_name != "usb" {
            continue;
        }

        let bdf = json_str(device, "bdf");
        let prog_if = json_str(device, "progIf");
        let phase = if prog_if == "30" {
            "xHCI"
        } else {
            "USB controller"
        };
        controllers.push(serde_json::json!({
            "id": bdf,
            "bdf": bdf,
            "vendorId": json_str(device, "vendorId"),
            "deviceId": json_str(device, "deviceId"),
            "phase": phase,
            "lifecycle": "pci-vlayer",
            "eventReady": false,
            "rootPortChangeSeen": false,
            "emptyProbeStreak": 0,
            "ports": [],
        }));
    }

    UsbSnapshot {
        count: 0,
        device_count: 0,
        controller_count: controllers.len(),
        topology_count: 0,
        devices: Vec::new(),
        controllers,
        topology: Vec::new(),
        unavailable_reason: "USB controllers are projected from PCI; USB devices and input topology are not exported yet",
    }
}

fn json_str<'a>(value: &'a serde_json::Value, key: &str) -> &'a str {
    value.get(key).and_then(|v| v.as_str()).unwrap_or("")
}

fn parse_pci_devices(text: &str) -> Vec<serde_json::Value> {
    let mut devices = Vec::new();
    let mut current: Option<PciDeviceDraft> = None;

    for line in text.lines() {
        let line = line.trim();
        if line.is_empty()
            || line.starts_with('#')
            || line.starts_with("trueos ")
            || line.starts_with("device_count=")
            || line.starts_with("dev,bdf,")
        {
            continue;
        }

        let fields: Vec<&str> = line.split(',').collect();
        match fields.first().copied() {
            Some("dev") if fields.len() >= 12 => {
                if let Some(device) = current.take() {
                    devices.push(pci_device_to_json(device));
                }
                let bdf = fields[1].trim().to_string();
                let vendor_id = fields[2].trim().to_string();
                let device_id = fields[3].trim().to_string();
                let class_name = fields[7].trim().to_string();
                let name = fields[11].trim();
                current = Some(PciDeviceDraft {
                    id: format!("pci-{}", bdf),
                    bdf,
                    vendor_id: vendor_id.clone(),
                    device_id: device_id.clone(),
                    class_code: fields[4].trim().to_string(),
                    subclass: fields[5].trim().to_string(),
                    prog_if: fields[6].trim().to_string(),
                    class_name: class_name.clone(),
                    role: fields[8].trim().to_string(),
                    command: fields[9].trim().to_string(),
                    status: fields[10].trim().to_string(),
                    name: if name.is_empty() || name == "-" {
                        format!("{} {}:{}", class_name, vendor_id, device_id)
                    } else {
                        name.to_string()
                    },
                    bars: Vec::new(),
                });
            }
            Some("bar") if fields.len() >= 9 => {
                let Some(device) = current.as_mut() else {
                    continue;
                };
                if fields[1].trim() != device.bdf {
                    continue;
                }
                device.bars.push(serde_json::json!({
                    "index": parse_u64(fields[2]).unwrap_or(0),
                    "kind": fields[3].trim(),
                    "width": fields[4].trim(),
                    "prefetchable": fields[5].trim() == "1",
                    "base": fields[6].trim(),
                    "size": fields[7].trim(),
                    "raw": fields[8].trim(),
                }));
            }
            _ => {}
        }
    }

    if let Some(device) = current.take() {
        devices.push(pci_device_to_json(device));
    }

    devices
}

fn pci_device_to_json(device: PciDeviceDraft) -> serde_json::Value {
    serde_json::json!({
        "id": device.id,
        "bdf": device.bdf,
        "vendorId": device.vendor_id,
        "deviceId": device.device_id,
        "classCode": device.class_code,
        "subclass": device.subclass,
        "progIf": device.prog_if,
        "className": device.class_name,
        "role": device.role,
        "command": device.command,
        "status": device.status,
        "name": device.name,
        "bars": device.bars,
    })
}

async fn handle_snapshot() -> Response {
    let pci = pci_payload();
    let usb = usb_payload_from_pci(&pci.devices);
    json_response(
        200,
        &HardwareSnapshot {
            schema: "trueos.webdevices.v1",
            generated_at_s: clock::ntp_current_unix_seconds(),
            service: ServiceSnapshot {
                name: "webdevices-http",
                port: current_port(),
                bind: "0.0.0.0",
            },
            rapl: rapl_payload(),
            thermal: thermal_payload(),
            pci,
            usb,
            note: "webdevices is running as a blueprint axum app with vlayer-backed PCI, RAPL, and thermal snapshots",
        },
    )
}

async fn handle_rapl_snapshot() -> Response {
    match rapl::snapshot_text() {
        Ok(text) if !text.is_empty() => {
            response(200, "text/plain; charset=utf-8", text.into_bytes(), true)
        }
        Ok(_) | Err(_) => text_response(
            503,
            "text/plain; charset=utf-8",
            "RAPL vlayer surface is unavailable\n",
        ),
    }
}

async fn handle_rapl_history() -> Response {
    match rapl::history_bytes(rapl::MAX_HISTORY_BYTES) {
        Ok(bytes) if !bytes.is_empty() => response(200, "text/plain; charset=utf-8", bytes, true),
        Ok(_) => text_response(
            503,
            "text/plain; charset=utf-8",
            "RAPL history is empty; waiting for service samples or vlayer resolver\n",
        ),
        Err(_) => text_response(
            503,
            "text/plain; charset=utf-8",
            "RAPL history is unavailable\n",
        ),
    }
}

async fn handle_thermal_snapshot() -> Response {
    match thermal::snapshot_text() {
        Ok(text) if !text.is_empty() => {
            response(200, "text/plain; charset=utf-8", text.into_bytes(), true)
        }
        Ok(_) | Err(_) => text_response(
            503,
            "text/plain; charset=utf-8",
            "thermal vlayer surface is unavailable\n",
        ),
    }
}

fn router() -> Router {
    Router::new()
        .route("/", get(handle_index))
        .route("/index.html", get(handle_index))
        .route("/tailwind.css", get(handle_tailwind_css))
        .route("/healthz", get(handle_healthz))
        .route("/api/healthz", get(handle_healthz))
        .route("/api/webdevices/snapshot", get(handle_snapshot))
        .route("/api/devices/snapshot", get(handle_snapshot))
        .route("/api/rapl/snapshot", get(handle_rapl_snapshot))
        .route("/api/rapl/history", get(handle_rapl_history))
        .route("/api/thermal/snapshot", get(handle_thermal_snapshot))
}

async fn webdevices_http_runtime() -> Result<(), io::Error> {
    let app = router();
    let addr = SocketAddr::from(([0, 0, 0, 0], WEBDEVICES_HTTP_TCP_PORT));
    loop {
        let listener = match tokio::net::TcpListener::bind(addr).await {
            Ok(listener) => listener,
            Err(err) => {
                WEBDEVICES_HTTP_PORT.store(0, Ordering::Release);
                logl::log(
                    level::WARN,
                    format_args!("webdevices-http: bind {} failed {}", addr, err),
                );
                time::sleep(Duration::from_millis(WEBDEVICES_BIND_RETRY_MS)).await;
                continue;
            }
        };

        WEBDEVICES_HTTP_PORT.store(addr.port(), Ordering::Release);
        logl::log(
            level::INFO,
            format_args!("webdevices-http: axum listening on http://{}/", addr),
        );
        let listener = listener.tap_io(|_| logl::log(level::INFO, "webdevices-http: tcp accepted"));
        let result = axum::serve(listener, app).await;
        WEBDEVICES_HTTP_PORT.store(0, Ordering::Release);
        return result;
    }
}

fn main() {
    logl::log(level::INFO, "webdevices-http: blueprint start");
    let runtime = match runtime::current_thread_net().build() {
        Ok(runtime) => runtime,
        Err(err) => {
            logl::log(
                level::ERROR,
                format_args!("webdevices-http: runtime build failed {}", err),
            );
            return;
        }
    };
    let local = tokio::task::LocalSet::new();
    local.block_on(&runtime, async {
        if let Err(err) = webdevices_http_runtime().await {
            logl::log(
                level::ERROR,
                format_args!("webdevices-http: runtime failed {:?}", err),
            );
        }
    });
    platform::poll_once();
}
