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
use core::net::SocketAddr;
use serde::Serialize;
use trueos::{
    clock, logl,
    logl::level,
    pci,
    platform::io,
    printers, rapl, system_services, thermal,
    time::{self, Duration},
    tokio::{self},
    usb,
};

const WEBDEVICES_HTTP_TCP_PORT: u16 = 10;
const WEBDEVICES_BIND_RETRY_MS: u64 = 1000;
const RAPL_UI_HISTORY_BYTES: usize = 256 * 1024;
const RAPL_UI_HISTORY_POINTS: usize = 240;
const WEBDEVICES_INDEX_HTML: &str = include_str!("webdevices/index.html");
const TRUEOS_TAILWIND_CSS: &str = include_str!("webdevices/tailwind.css");

static WEBDEVICES_HTTP_PORT: AtomicU16 = AtomicU16::new(0);

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct HardwareSnapshot {
    schema: &'static str,
    generated_at_s: u64,
    service: ServiceSnapshot,
    rapl: RaplSnapshot,
    thermal: ThermalSnapshot,
    printers: PrinterSnapshot,
    task_profile: TaskProfileSnapshot,
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
struct TaskProfileSnapshot {
    vlayer_available: bool,
    history_count: usize,
    history_capacity: usize,
    latest: Option<TaskProfilePoint>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct TaskProfilePoint {
    sequence: u64,
    now_ms: u64,
    heartbeat_gap_ms: u64,
    polls: u64,
    busy_us: u64,
    busy_permille: u64,
    top_task: String,
    top_total_us: u64,
    longest_task: String,
    longest_poll_us: u64,
    slow_polls: u64,
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
struct PrinterSnapshot {
    vlayer_available: bool,
    snapshot_bytes: usize,
    generated_at_ms: Option<u64>,
    discovery_interval_ms: Option<u64>,
    stale_after_ms: Option<u64>,
    printer_count: usize,
    printers: Vec<PrinterRow>,
    latest_text: String,
    unavailable_reason: Option<&'static str>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PrinterRow {
    id: String,
    name: String,
    uri: String,
    secure: bool,
    make_and_model: Option<String>,
    formats: Vec<String>,
    last_seen_ms: u64,
    age_ms: Option<u64>,
    default_candidate: bool,
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
    vlayer_available: bool,
    snapshot_bytes: usize,
    count: usize,
    device_count: usize,
    controller_count: usize,
    topology_count: usize,
    devices: Vec<serde_json::Value>,
    controllers: Vec<serde_json::Value>,
    topology: Vec<serde_json::Value>,
    probe_error: Option<String>,
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

fn printer_payload() -> PrinterSnapshot {
    match printers::snapshot_text() {
        Ok(latest_text) if !latest_text.is_empty() => {
            let generated_at_ms = parse_snapshot_scalar(&latest_text, "generated_at_ms");
            let discovery_interval_ms =
                parse_snapshot_scalar(&latest_text, "discovery_interval_ms");
            let stale_after_ms = parse_snapshot_scalar(&latest_text, "stale_after_ms");
            let discovered = printers::parse_snapshot_text(&latest_text);
            let mut default_selected = false;
            let printers = discovered
                .into_iter()
                .map(|printer| {
                    let supports_pwg_raster = printer.formats.is_empty()
                        || printer
                            .formats
                            .iter()
                            .any(|format| format.eq_ignore_ascii_case("image/pwg-raster"));
                    let default_candidate =
                        !default_selected && !printer.secure && supports_pwg_raster;
                    default_selected |= default_candidate;
                    PrinterRow {
                        id: format!("printer-{}", printer.uri),
                        name: printer.name,
                        uri: printer.uri,
                        secure: printer.secure,
                        make_and_model: printer.make_and_model,
                        formats: printer.formats,
                        last_seen_ms: printer.last_seen_ms,
                        age_ms: generated_at_ms
                            .map(|generated| generated.saturating_sub(printer.last_seen_ms)),
                        default_candidate,
                    }
                })
                .collect::<Vec<_>>();
            PrinterSnapshot {
                vlayer_available: true,
                snapshot_bytes: latest_text.len(),
                generated_at_ms,
                discovery_interval_ms,
                stale_after_ms,
                printer_count: printers.len(),
                printers,
                latest_text,
                unavailable_reason: None,
            }
        }
        Ok(_) | Err(_) => PrinterSnapshot {
            vlayer_available: false,
            snapshot_bytes: 0,
            generated_at_ms: None,
            discovery_interval_ms: None,
            stale_after_ms: None,
            printer_count: 0,
            printers: Vec::new(),
            latest_text: String::new(),
            unavailable_reason: Some("printer vlayer surface is unavailable"),
        },
    }
}

fn parse_snapshot_scalar(text: &str, key: &str) -> Option<u64> {
    let prefix = format!("{}=", key);
    text.lines()
        .map(str::trim)
        .find_map(|line| line.strip_prefix(&prefix).and_then(parse_optional_u64))
}

fn task_profile_payload() -> TaskProfileSnapshot {
    let Ok(text) = system_services::snapshot_text() else {
        return TaskProfileSnapshot {
            vlayer_available: false,
            history_count: 0,
            history_capacity: 0,
            latest: None,
        };
    };
    let history_capacity =
        parse_snapshot_scalar(&text, "task_profile_history_capacity").unwrap_or(0) as usize;
    let mut history_count = 0usize;
    let mut latest = None;
    for line in text.lines().filter(|line| {
        line.starts_with("task-profile\t") && !line.starts_with("task-profile\tsequence\t")
    }) {
        let fields = line.split('\t').collect::<Vec<_>>();
        if fields.len() != 21 {
            continue;
        }
        let number = |index: usize| fields[index].parse::<u64>().ok();
        let Some(point) = (|| {
            Some(TaskProfilePoint {
                sequence: number(1)?,
                now_ms: number(2)?,
                heartbeat_gap_ms: number(3)?,
                polls: number(7)?,
                busy_us: number(8)?,
                busy_permille: number(9)?,
                top_task: fields[10].to_string(),
                top_total_us: number(13)?,
                longest_task: fields[14].to_string(),
                longest_poll_us: number(16)?,
                slow_polls: number(17)?,
            })
        })() else {
            continue;
        };
        history_count = history_count.saturating_add(1);
        latest = Some(point);
    }
    TaskProfileSnapshot {
        vlayer_available: !text.is_empty(),
        history_count,
        history_capacity,
        latest,
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

fn usb_payload(pci_devices: &[serde_json::Value]) -> UsbSnapshot {
    match usb::snapshot_text() {
        Ok(text) if !text.is_empty() => parse_usb_snapshot(&text),
        Ok(_) | Err(_) => usb_payload_from_pci(pci_devices),
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
        vlayer_available: false,
        snapshot_bytes: 0,
        count: 0,
        device_count: 0,
        controller_count: controllers.len(),
        topology_count: 0,
        devices: Vec::new(),
        controllers,
        topology: Vec::new(),
        probe_error: None,
        unavailable_reason: "USB controllers are projected from PCI; USB devices and input topology are not exported yet",
    }
}

fn parse_usb_snapshot(text: &str) -> UsbSnapshot {
    let mut devices = Vec::new();
    let mut controllers = Vec::new();
    let mut topology = Vec::new();
    let mut probe_error = None;

    for line in text.lines() {
        let fields: Vec<&str> = line.split('\t').collect();
        match fields.first().copied() {
            Some("summary") if fields.len() >= 4 => {
                if fields[3] != "-" && !fields[3].is_empty() {
                    probe_error = Some(fields[3].to_string());
                }
            }
            Some("controller") if fields.len() >= 12 => {
                controllers.push(serde_json::json!({
                    "id": parse_u64(fields[1]).unwrap_or(0),
                    "bdf": fields[2],
                    "vendorId": fields[3],
                    "deviceId": fields[4],
                    "phase": fields[5],
                    "lifecycle": fields[6],
                    "eventReady": fields[7] == "1",
                    "rootPortChangeSeen": fields[8] == "1",
                    "emptyProbeStreak": parse_u64(fields[9]).unwrap_or(0),
                    "lastProbeState": fields[10],
                    "lastProbeDeviceCount": parse_u64(fields[11]).unwrap_or(0),
                    "ports": [],
                }));
            }
            Some("port") if fields.len() >= 10 => {
                let controller_id = parse_u64(fields[1]).unwrap_or(0);
                if let Some(controller) = controllers.iter_mut().find(|controller| {
                    controller.get("id").and_then(serde_json::Value::as_u64) == Some(controller_id)
                }) {
                    if let Some(ports) = controller
                        .get_mut("ports")
                        .and_then(serde_json::Value::as_array_mut)
                    {
                        ports.push(serde_json::json!({
                            "id": parse_u64(fields[2]).unwrap_or(0),
                            "connected": fields[3] == "1",
                            "enabled": fields[4] == "1",
                            "speed": fields[5],
                            "linkState": fields[6],
                            "portsc": fields[7],
                            "portpmsc": fields[8],
                            "portli": fields[9],
                        }));
                    }
                }
            }
            Some("device") if fields.len() >= 22 => {
                let stable_id = fields[1].to_string();
                devices.push(serde_json::json!({
                    "id": format!("usb-{}", stable_id),
                    "stableId": stable_id,
                    "controllerId": parse_u64(fields[2]).unwrap_or(0),
                    "slotId": parse_u64(fields[3]).unwrap_or(0),
                    "rootPortId": parse_u64(fields[4]).unwrap_or(0),
                    "portId": parse_u64(fields[5]).unwrap_or(0),
                    "route": fields[6],
                    "speed": fields[7],
                    "vendorId": fields[8],
                    "productId": fields[9],
                    "class": fields[10],
                    "subclass": fields[11],
                    "protocol": fields[12],
                    "classCode": format!("{}/{}/{}", fields[10], fields[11], fields[12]),
                    "className": usb_class_name(fields[10]),
                    "usbVersion": fields[13],
                    "deviceVersion": fields[14],
                    "numConfigurations": parse_u64(fields[15]).unwrap_or(0),
                    "maxPacketSize0": parse_u64(fields[16]).unwrap_or(0),
                    "parentHubSlotId": parse_optional_u64(fields[17]),
                    "manufacturer": empty_to_none(fields[18]),
                    "product": empty_to_none(fields[19]),
                    "serial": empty_to_none(fields[20]),
                    "path": fields[21].split('.').filter_map(|part| parse_u64(part)).collect::<Vec<_>>(),
                    "hubPath": [],
                    "configurations": [],
                    "interfaceCount": 0,
                    "endpointCount": 0,
                }));
            }
            Some("config") if fields.len() >= 5 => {
                if let Some(device) = usb_device_mut(&mut devices, fields[1]) {
                    if let Some(configurations) = device
                        .get_mut("configurations")
                        .and_then(serde_json::Value::as_array_mut)
                    {
                        configurations.push(serde_json::json!({
                            "value": parse_u64(fields[2]).unwrap_or(0),
                            "attributes": fields[3],
                            "maxPower": parse_u64(fields[4]).unwrap_or(0),
                            "interfaces": [],
                        }));
                    }
                }
            }
            Some("interface") if fields.len() >= 8 => {
                if let Some(configuration) =
                    usb_configuration_mut(&mut devices, fields[1], fields[2])
                {
                    if let Some(interfaces) = configuration
                        .get_mut("interfaces")
                        .and_then(serde_json::Value::as_array_mut)
                    {
                        interfaces.push(serde_json::json!({
                            "number": parse_u64(fields[3]).unwrap_or(0),
                            "alternateSetting": parse_u64(fields[4]).unwrap_or(0),
                            "class": fields[5],
                            "subclass": fields[6],
                            "protocol": fields[7],
                            "classCode": format!("{}/{}/{}", fields[5], fields[6], fields[7]),
                            "className": usb_class_name(fields[5]),
                            "endpoints": [],
                        }));
                    }
                }
            }
            Some("endpoint") if fields.len() >= 9 => {
                if let Some(interface) =
                    usb_interface_mut(&mut devices, fields[1], fields[2], fields[3], fields[4])
                {
                    if let Some(endpoints) = interface
                        .get_mut("endpoints")
                        .and_then(serde_json::Value::as_array_mut)
                    {
                        endpoints.push(serde_json::json!({
                            "address": fields[5],
                            "transferType": fields[6],
                            "maxPacketSize": parse_u64(fields[7]).unwrap_or(0),
                            "interval": parse_u64(fields[8]).unwrap_or(0),
                        }));
                    }
                }
            }
            Some("hop") if fields.len() >= 6 => {
                if let Some(device) = usb_device_mut(&mut devices, fields[1]) {
                    if let Some(hub_path) = device
                        .get_mut("hubPath")
                        .and_then(serde_json::Value::as_array_mut)
                    {
                        hub_path.push(serde_json::json!({
                            "slotId": parse_u64(fields[2]).unwrap_or(0),
                            "portId": parse_u64(fields[3]).unwrap_or(0),
                            "depth": parse_u64(fields[4]).unwrap_or(0),
                            "speed": fields[5],
                        }));
                    }
                }
            }
            Some("topology") if fields.len() >= 14 => {
                let vendor_id = optional_text(fields[9]);
                let product_id = optional_text(fields[10]);
                topology.push(serde_json::json!({
                    "kind": fields[1],
                    "controllerId": parse_u64(fields[2]).unwrap_or(0),
                    "rootPortId": parse_u64(fields[3]).unwrap_or(0),
                    "portId": parse_u64(fields[4]).unwrap_or(0),
                    "depth": parse_u64(fields[5]).unwrap_or(0),
                    "slotId": parse_optional_u64(fields[6]),
                    "parentSlotId": parse_optional_u64(fields[7]),
                    "speed": fields[8],
                    "vendorId": vendor_id,
                    "productId": product_id,
                    "vidPid": match (vendor_id, product_id) {
                        (Some(vendor), Some(product)) => format!("{}:{}", vendor, product),
                        _ => "-".to_string(),
                    },
                    "class": optional_text(fields[11]),
                    "subclass": optional_text(fields[12]),
                    "protocol": optional_text(fields[13]),
                }));
            }
            _ => {}
        }
    }

    for device in &mut devices {
        finalize_usb_device(device);
    }
    UsbSnapshot {
        vlayer_available: true,
        snapshot_bytes: text.len(),
        count: devices.len(),
        device_count: devices.len(),
        controller_count: controllers.len(),
        topology_count: topology.len(),
        devices,
        controllers,
        topology,
        probe_error,
        unavailable_reason: "",
    }
}

fn usb_device_mut<'a>(
    devices: &'a mut [serde_json::Value],
    stable_id: &str,
) -> Option<&'a mut serde_json::Value> {
    devices.iter_mut().find(|device| {
        device.get("stableId").and_then(serde_json::Value::as_str) == Some(stable_id)
    })
}

fn usb_configuration_mut<'a>(
    devices: &'a mut [serde_json::Value],
    stable_id: &str,
    value: &str,
) -> Option<&'a mut serde_json::Value> {
    let value = parse_u64(value)?;
    usb_device_mut(devices, stable_id)?
        .get_mut("configurations")?
        .as_array_mut()?
        .iter_mut()
        .find(|configuration| {
            configuration
                .get("value")
                .and_then(serde_json::Value::as_u64)
                == Some(value)
        })
}

fn usb_interface_mut<'a>(
    devices: &'a mut [serde_json::Value],
    stable_id: &str,
    configuration_value: &str,
    interface_number: &str,
    alternate_setting: &str,
) -> Option<&'a mut serde_json::Value> {
    let interface_number = parse_u64(interface_number)?;
    let alternate_setting = parse_u64(alternate_setting)?;
    usb_configuration_mut(devices, stable_id, configuration_value)?
        .get_mut("interfaces")?
        .as_array_mut()?
        .iter_mut()
        .find(|interface| {
            interface.get("number").and_then(serde_json::Value::as_u64) == Some(interface_number)
                && interface
                    .get("alternateSetting")
                    .and_then(serde_json::Value::as_u64)
                    == Some(alternate_setting)
        })
}

fn finalize_usb_device(device: &mut serde_json::Value) {
    let Some(configurations) = device
        .get("configurations")
        .and_then(serde_json::Value::as_array)
    else {
        return;
    };
    let mut interface_count = 0u64;
    let mut endpoint_count = 0u64;
    let mut effective_class = device
        .get("class")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("00");
    for configuration in configurations {
        for interface in configuration
            .get("interfaces")
            .and_then(serde_json::Value::as_array)
            .into_iter()
            .flatten()
        {
            interface_count = interface_count.saturating_add(1);
            endpoint_count = endpoint_count.saturating_add(
                interface
                    .get("endpoints")
                    .and_then(serde_json::Value::as_array)
                    .map(|endpoints| endpoints.len() as u64)
                    .unwrap_or(0),
            );
            if effective_class == "00" {
                effective_class = interface
                    .get("class")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("00");
            }
        }
    }
    let class_name = usb_class_name(effective_class);
    if let Some(object) = device.as_object_mut() {
        object.insert("interfaceCount".to_string(), interface_count.into());
        object.insert("endpointCount".to_string(), endpoint_count.into());
        object.insert("className".to_string(), class_name.into());
    }
}

fn usb_class_name(class: &str) -> &'static str {
    match class.trim_start_matches("0x").to_ascii_uppercase().as_str() {
        "00" => "per-interface",
        "01" => "audio",
        "02" | "0A" => "communications",
        "03" => "HID input",
        "06" => "imaging",
        "07" => "printer",
        "08" => "mass storage",
        "09" => "hub",
        "0B" => "smart card",
        "0E" => "video",
        "E0" => "wireless",
        "EF" => "miscellaneous",
        "FE" => "application specific",
        "FF" => "vendor specific",
        _ => "unknown",
    }
}

fn optional_text(value: &str) -> Option<&str> {
    (value != "-" && !value.is_empty()).then_some(value)
}

fn empty_to_none(value: &str) -> Option<&str> {
    (!value.is_empty()).then_some(value)
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
    let usb = usb_payload(&pci.devices);
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
            printers: printer_payload(),
            task_profile: task_profile_payload(),
            pci,
            usb,
            note: "webdevices is running as a blueprint axum app with shared vlayer-backed device and BSP task snapshots",
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

async fn handle_usb_snapshot() -> Response {
    match usb::snapshot_text() {
        Ok(text) if !text.is_empty() => {
            response(200, "text/plain; charset=utf-8", text.into_bytes(), true)
        }
        Ok(_) | Err(_) => text_response(
            503,
            "text/plain; charset=utf-8",
            "USB vlayer surface is unavailable\n",
        ),
    }
}

async fn handle_printer_snapshot() -> Response {
    let snapshot = printer_payload();
    let status = if snapshot.vlayer_available { 200 } else { 503 };
    json_response(status, &snapshot)
}

async fn handle_printer_snapshot_text() -> Response {
    match printers::snapshot_text() {
        Ok(text) if !text.is_empty() => {
            response(200, "text/plain; charset=utf-8", text.into_bytes(), true)
        }
        Ok(_) | Err(_) => text_response(
            503,
            "text/plain; charset=utf-8",
            "printer vlayer surface is unavailable\n",
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
        .route("/api/usb/snapshot", get(handle_usb_snapshot))
        .route("/api/printers/snapshot", get(handle_printer_snapshot))
        .route(
            "/api/printers/snapshot.txt",
            get(handle_printer_snapshot_text),
        )
}

/// Serve the WebDevices UI from the shared FileSystem Blueprint process.
///
/// The service retains its dedicated port so existing device-dashboard links
/// continue to work while avoiding a separate Blueprint instance.
pub async fn serve() -> Result<(), io::Error> {
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
