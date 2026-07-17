use core::net::{Ipv4Addr, SocketAddr};
use std::collections::BTreeMap;
use std::io::Cursor;

use ipp::prelude::{
    IppAttribute, IppOperationBuilder, IppPayload, IppRequestResponse, IppValue, IppVersion, Uri,
};
use ipp::{operation::IppOperation, parser::IppParser};
use trueos::{
    env,
    logl::{self, level},
    platform::{String, ToString, Vec, format},
    t,
};

const APP: &str = "ipp-printer";
const DEFAULT_DISCOVERY_MS: u64 = 3_000;
const IPP_TIMEOUT_MS: u64 = 30_000;
const MAX_IPP_RESPONSE_BYTES: usize = 8 * 1024 * 1024;
const MDNS_PORT: u16 = 5_353;
const MDNS_ADDR: SocketAddr = SocketAddr::new(
    core::net::IpAddr::V4(Ipv4Addr::new(224, 0, 0, 251)),
    MDNS_PORT,
);
const MDNS_SERVICES: [&str; 3] = [
    "_ipp._tcp.local.",
    "_print._sub._ipp._tcp.local.",
    "_ipps._tcp.local.",
];
const CAPABILITY_ATTRIBUTES: [&str; 15] = [
    "printer-info",
    "printer-make-and-model",
    "printer-location",
    "printer-state",
    "printer-state-reasons",
    "printer-is-accepting-jobs",
    "queued-job-count",
    "ipp-versions-supported",
    "document-format-supported",
    "media-ready",
    "media-supported",
    "sides-supported",
    "print-color-mode-supported",
    "printer-resolution-supported",
    "operations-supported",
];

type AppResult<T> = Result<T, String>;

#[derive(Clone, Debug, Default)]
struct PrintOptions {
    format: Option<String>,
    copies: Option<i32>,
    media: Option<String>,
    sides: Option<String>,
    color: Option<String>,
    quality: Option<i32>,
}

#[derive(Clone, Debug, Default)]
struct PartialPrinter {
    instance: String,
    service: String,
    target: String,
    port: u16,
    txt: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct DiscoveredPrinter {
    name: String,
    uri: String,
    secure: bool,
    make_and_model: Option<String>,
    formats: Vec<String>,
}

#[derive(Default)]
struct DiscoveryState {
    printers: BTreeMap<String, PartialPrinter>,
    ipv4: BTreeMap<String, Ipv4Addr>,
}

fn main() {
    let runtime = match t::runtime::current_thread_net().build() {
        Ok(runtime) => runtime,
        Err(runtime_error) => {
            error(format!(
                "could not start Tokio network runtime: {runtime_error}"
            ));
            return;
        }
    };

    runtime.block_on(async {
        if let Err(message) = run().await {
            error(message);
        }
    });
}

async fn run() -> AppResult<()> {
    let mut args = env::args();
    let _archive_name = args.next();
    let Some(command) = args.next() else {
        print_usage();
        return discover_and_print(DEFAULT_DISCOVERY_MS).await;
    };

    match command.as_str() {
        "discover" | "scan" => {
            let timeout_ms = args
                .next()
                .map(|value| parse_u64(&value, "discovery milliseconds"))
                .transpose()?
                .unwrap_or(DEFAULT_DISCOVERY_MS);
            discover_and_print(timeout_ms).await
        }
        "info" | "status" => {
            let uri = args
                .next()
                .ok_or_else(|| String::from("info requires an ipp:// or ipps:// URI"))?;
            print_capabilities(&uri).await.map(|_| ())
        }
        "print" => {
            let destination = args.next().ok_or_else(|| {
                String::from("print requires a printer URI (or 'auto') and a document path")
            })?;
            let path = args.next().ok_or_else(|| {
                String::from("print requires a printer URI (or 'auto') and a document path")
            })?;
            let options = parse_print_options(args.collect())?;
            let uri = if destination == "auto" {
                choose_auto_printer(DEFAULT_DISCOVERY_MS).await?.uri
            } else {
                destination
            };
            submit_job(&uri, &path, options).await
        }
        "help" | "--help" | "-h" => {
            print_usage();
            Ok(())
        }
        value if value.starts_with("ipp://") || value.starts_with("ipps://") => {
            print_capabilities(value).await.map(|_| ())
        }
        _ => Err(format!("unknown command '{command}'; use '{APP} help'")),
    }
}

fn print_usage() {
    info("TRUEOS IPP Everywhere printer client");
    info(format!("usage: {APP} discover [milliseconds]"));
    info(format!(
        "       {APP} info <ipp[s]://host[:port]/ipp/print>"
    ));
    info(format!(
        "       {APP} print <URI|auto> <document> [--format MIME] [--copies N]"
    ));
    info("              [--media PWG-NAME] [--sides MODE] [--color MODE]");
    info("              [--quality draft|normal|high]");
}

fn info(message: impl AsRef<str>) {
    logl::log(level::INFO, message.as_ref());
}

fn warn(message: impl AsRef<str>) {
    logl::log(level::WARN, message.as_ref());
}

fn error(message: impl AsRef<str>) {
    logl::log(level::ERROR, message.as_ref());
}

async fn discover_and_print(timeout_ms: u64) -> AppResult<()> {
    info(format!(
        "{APP}: looking for IPP Everywhere printers on mDNS"
    ));
    let printers = discover(timeout_ms).await?;
    if printers.is_empty() {
        warn(format!(
            "{APP}: no printer answered; direct IP still works with '{APP} info ipp://ADDRESS/ipp/print'"
        ));
        return Ok(());
    }

    for (index, printer) in printers.iter().enumerate() {
        let security = if printer.secure { "IPPS" } else { "IPP" };
        info(format!(
            "{}. {} [{}] {}",
            index + 1,
            printer.name,
            security,
            printer.uri
        ));
        if let Some(model) = &printer.make_and_model {
            info(format!("   model: {model}"));
        }
        if !printer.formats.is_empty() {
            info(format!("   formats: {}", printer.formats.join(", ")));
        }
    }
    Ok(())
}

async fn choose_auto_printer(timeout_ms: u64) -> AppResult<DiscoveredPrinter> {
    let mut printers = discover(timeout_ms).await?;
    printers.sort_by_key(|printer| printer.secure);
    printers.into_iter().next().ok_or_else(|| {
        String::from("no IPP printer was discovered; pass an explicit ipp:// address")
    })
}

async fn discover(timeout_ms: u64) -> AppResult<Vec<DiscoveredPrinter>> {
    let socket = match t::net::UdpSocket::bind((Ipv4Addr::UNSPECIFIED, MDNS_PORT)).await {
        Ok(socket) => socket,
        Err(_) => t::net::UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0))
            .await
            .map_err(|error| format!("could not open mDNS UDP socket: {error}"))?,
    };
    let query = build_mdns_query(&MDNS_SERVICES)?;
    socket
        .send_to(&query, MDNS_ADDR)
        .await
        .map_err(|error| format!("could not send mDNS query: {error}"))?;

    let mut state = DiscoveryState::default();
    let deadline = t::time::Instant::now() + t::time::Duration::from_millis(timeout_ms.max(1));
    let mut buffer = [0u8; 9_000];
    while t::time::Instant::now() < deadline {
        let remaining = deadline.saturating_duration_since(t::time::Instant::now());
        let slice = remaining.min(t::time::Duration::from_millis(500));
        match t::time::timeout(slice, socket.recv_from(&mut buffer)).await {
            Ok(Ok((read, _peer))) => {
                if let Err(message) = parse_mdns_packet(&buffer[..read], &mut state) {
                    warn(format!("{APP}: ignored malformed mDNS response: {message}"));
                }
            }
            Ok(Err(error)) => return Err(format!("mDNS receive failed: {error}")),
            Err(_) => {}
        }
    }
    Ok(state.finish())
}

fn build_mdns_query(services: &[&str]) -> AppResult<Vec<u8>> {
    let count = u16::try_from(services.len())
        .map_err(|_| String::from("too many mDNS service questions"))?;
    let mut out = Vec::with_capacity(128);
    out.extend_from_slice(&0u16.to_be_bytes());
    out.extend_from_slice(&0u16.to_be_bytes());
    out.extend_from_slice(&count.to_be_bytes());
    out.extend_from_slice(&0u16.to_be_bytes());
    out.extend_from_slice(&0u16.to_be_bytes());
    out.extend_from_slice(&0u16.to_be_bytes());
    for service in services {
        encode_dns_name(service, &mut out)?;
        out.extend_from_slice(&12u16.to_be_bytes());
        // IN plus the mDNS unicast-response bit. This also lets the ephemeral-port
        // fallback receive replies when another local responder owns port 5353.
        out.extend_from_slice(&0x8001u16.to_be_bytes());
    }
    Ok(out)
}

fn encode_dns_name(name: &str, out: &mut Vec<u8>) -> AppResult<()> {
    for label in name.trim_end_matches('.').split('.') {
        if label.is_empty() || label.len() > 63 {
            return Err(format!("invalid DNS label in '{name}'"));
        }
        out.push(label.len() as u8);
        out.extend_from_slice(label.as_bytes());
    }
    out.push(0);
    Ok(())
}

fn parse_mdns_packet(packet: &[u8], state: &mut DiscoveryState) -> AppResult<()> {
    if packet.len() < 12 {
        return Err(String::from("packet shorter than DNS header"));
    }
    let question_count = read_u16(packet, 4)? as usize;
    let answer_count = read_u16(packet, 6)? as usize;
    let authority_count = read_u16(packet, 8)? as usize;
    let additional_count = read_u16(packet, 10)? as usize;
    let mut offset = 12usize;

    for _ in 0..question_count {
        let _ = read_dns_name(packet, &mut offset)?;
        checked_advance(packet, &mut offset, 4)?;
    }

    let record_count = answer_count
        .checked_add(authority_count)
        .and_then(|count| count.checked_add(additional_count))
        .ok_or_else(|| String::from("DNS record count overflow"))?;
    for _ in 0..record_count {
        let owner = read_dns_name(packet, &mut offset)?;
        let record_type = read_u16(packet, offset)?;
        checked_advance(packet, &mut offset, 2)?;
        checked_advance(packet, &mut offset, 2)?; // class
        checked_advance(packet, &mut offset, 4)?; // ttl
        let data_len = read_u16(packet, offset)? as usize;
        checked_advance(packet, &mut offset, 2)?;
        let data_start = offset;
        let data_end = data_start
            .checked_add(data_len)
            .filter(|end| *end <= packet.len())
            .ok_or_else(|| String::from("DNS record exceeds packet"))?;

        match record_type {
            1 if data_len == 4 => {
                state.ipv4.insert(
                    dns_key(&owner),
                    Ipv4Addr::new(
                        packet[data_start],
                        packet[data_start + 1],
                        packet[data_start + 2],
                        packet[data_start + 3],
                    ),
                );
            }
            12 => {
                let mut cursor = data_start;
                let instance = read_dns_name(packet, &mut cursor)?;
                if cursor > data_end {
                    return Err(String::from("PTR target exceeds record"));
                }
                let key = dns_key(&instance);
                let printer = state.printers.entry(key).or_default();
                printer.instance = instance;
                if owner.contains("_ipp._tcp") || owner.contains("_ipps._tcp") {
                    printer.service = owner;
                }
            }
            16 => {
                let values = parse_txt(&packet[data_start..data_end])?;
                let key = dns_key(&owner);
                let printer = state.printers.entry(key).or_default();
                printer.instance = owner;
                printer.txt.extend(values);
            }
            33 if data_len >= 6 => {
                let port = read_u16(packet, data_start + 4)?;
                let mut cursor = data_start + 6;
                let target = read_dns_name(packet, &mut cursor)?;
                if cursor > data_end {
                    return Err(String::from("SRV target exceeds record"));
                }
                let key = dns_key(&owner);
                let printer = state.printers.entry(key).or_default();
                printer.instance = owner;
                printer.target = target;
                printer.port = port;
            }
            _ => {}
        }
        offset = data_end;
    }
    Ok(())
}

fn parse_txt(data: &[u8]) -> AppResult<BTreeMap<String, String>> {
    let mut values = BTreeMap::new();
    let mut offset = 0usize;
    while offset < data.len() {
        let len = data[offset] as usize;
        offset += 1;
        let end = offset
            .checked_add(len)
            .filter(|end| *end <= data.len())
            .ok_or_else(|| String::from("truncated DNS TXT item"))?;
        if let Ok(text) = core::str::from_utf8(&data[offset..end]) {
            let (key, value) = text.split_once('=').unwrap_or((text, ""));
            values.insert(key.to_ascii_lowercase(), value.to_string());
        }
        offset = end;
    }
    Ok(values)
}

fn read_dns_name(packet: &[u8], offset: &mut usize) -> AppResult<String> {
    let mut labels = Vec::new();
    let mut cursor = *offset;
    let mut jumped = false;
    let mut jumps = 0usize;
    loop {
        let length = *packet
            .get(cursor)
            .ok_or_else(|| String::from("truncated DNS name"))?;
        if length & 0xc0 == 0xc0 {
            let next = *packet
                .get(cursor + 1)
                .ok_or_else(|| String::from("truncated DNS compression pointer"))?;
            if !jumped {
                *offset = cursor + 2;
                jumped = true;
            }
            cursor = (((length & 0x3f) as usize) << 8) | next as usize;
            jumps += 1;
            if jumps > 32 || cursor >= packet.len() {
                return Err(String::from("invalid DNS compression pointer"));
            }
            continue;
        }
        if length & 0xc0 != 0 {
            return Err(String::from("unsupported DNS label encoding"));
        }
        cursor += 1;
        if length == 0 {
            if !jumped {
                *offset = cursor;
            }
            break;
        }
        let end = cursor
            .checked_add(length as usize)
            .filter(|end| *end <= packet.len())
            .ok_or_else(|| String::from("truncated DNS label"))?;
        let label = core::str::from_utf8(&packet[cursor..end])
            .map_err(|_| String::from("non-UTF-8 DNS label"))?;
        labels.push(label.to_string());
        cursor = end;
    }
    if labels.is_empty() {
        Ok(String::from("."))
    } else {
        Ok(format!("{}.", labels.join(".")))
    }
}

fn read_u16(data: &[u8], offset: usize) -> AppResult<u16> {
    let bytes = data
        .get(offset..offset + 2)
        .ok_or_else(|| String::from("truncated 16-bit network value"))?;
    Ok(u16::from_be_bytes([bytes[0], bytes[1]]))
}

fn checked_advance(data: &[u8], offset: &mut usize, count: usize) -> AppResult<()> {
    *offset = offset
        .checked_add(count)
        .filter(|end| *end <= data.len())
        .ok_or_else(|| String::from("truncated DNS field"))?;
    Ok(())
}

fn dns_key(name: &str) -> String {
    name.trim_end_matches('.').to_ascii_lowercase()
}

impl DiscoveryState {
    fn finish(self) -> Vec<DiscoveredPrinter> {
        let mut result = Vec::new();
        for printer in self.printers.into_values() {
            if printer.target.is_empty() || printer.port == 0 {
                continue;
            }
            let secure =
                printer.service.contains("_ipps._tcp") || printer.instance.contains("._ipps._tcp");
            let scheme = if secure { "ipps" } else { "ipp" };
            let host = if secure {
                printer.target.trim_end_matches('.').to_string()
            } else {
                self.ipv4
                    .get(&dns_key(&printer.target))
                    .map(ToString::to_string)
                    .unwrap_or_else(|| printer.target.trim_end_matches('.').to_string())
            };
            let resource = printer
                .txt
                .get("rp")
                .map(|value| value.trim_start_matches('/'))
                .filter(|value| !value.is_empty())
                .unwrap_or("ipp/print");
            let uri = format!("{scheme}://{host}:{}/{resource}", printer.port);
            let name = printer
                .txt
                .get("ty")
                .filter(|value| !value.is_empty())
                .cloned()
                .unwrap_or_else(|| display_instance_name(&printer.instance));
            let make_and_model = printer
                .txt
                .get("product")
                .map(|value| value.trim_matches(['(', ')']).to_string())
                .filter(|value| !value.is_empty());
            let formats = printer
                .txt
                .get("pdl")
                .map(|value| {
                    value
                        .split(',')
                        .map(str::trim)
                        .filter(|value| !value.is_empty())
                        .map(ToString::to_string)
                        .collect()
                })
                .unwrap_or_default();
            let candidate = DiscoveredPrinter {
                name,
                uri,
                secure,
                make_and_model,
                formats,
            };
            if !result
                .iter()
                .any(|existing: &DiscoveredPrinter| existing.uri == candidate.uri)
            {
                result.push(candidate);
            }
        }
        result.sort_by(|left, right| {
            left.secure
                .cmp(&right.secure)
                .then_with(|| left.name.cmp(&right.name))
        });
        result
    }
}

fn display_instance_name(instance: &str) -> String {
    let lowered = instance.to_ascii_lowercase();
    for marker in ["._ipp._tcp.", "._ipps._tcp."] {
        if let Some(index) = lowered.find(marker) {
            return instance[..index].to_string();
        }
    }
    instance.trim_end_matches('.').to_string()
}

fn parse_uri(uri: &str) -> AppResult<Uri> {
    if !uri.starts_with("ipp://") && !uri.starts_with("ipps://") {
        return Err(String::from(
            "printer URI must start with ipp:// or ipps://",
        ));
    }
    uri.parse::<Uri>()
        .map_err(|error| format!("invalid printer URI: {error}"))
}

async fn get_capabilities(uri_text: &str) -> AppResult<IppRequestResponse> {
    let uri = parse_uri(uri_text)?;
    let operation = IppOperationBuilder::get_printer_attributes(uri.clone())
        .attributes(CAPABILITY_ATTRIBUTES)
        .build()
        .map_err(|error| format!("could not build Get-Printer-Attributes: {error}"))?;
    let mut request = operation.into_ipp_request();
    request.header_mut().version = IppVersion::v2_0();
    let response = send_ipp(uri, request.to_bytes().to_vec()).await?;
    if !response.header().status_code().is_success() {
        return Err(format!(
            "printer rejected capability query: {}",
            response.header().status_code()
        ));
    }
    Ok(response)
}

async fn print_capabilities(uri: &str) -> AppResult<IppRequestResponse> {
    info(format!("{APP}: querying {uri}"));
    let response = get_capabilities(uri).await?;
    let version = response.header().version.0;
    info(format!(
        "IPP status: {} (version {}.{})",
        response.header().status_code(),
        version >> 8,
        version & 0xff
    ));
    for name in CAPABILITY_ATTRIBUTES {
        let values = attribute_values(&response, name);
        if !values.is_empty() {
            info(format!("{name}: {}", values.join(", ")));
        }
    }
    Ok(response)
}

fn attribute_values(response: &IppRequestResponse, name: &str) -> Vec<String> {
    let mut values = Vec::new();
    for group in response.attributes().groups() {
        for attribute in group.attributes().values() {
            if attribute.name().as_str() == name {
                values.extend(attribute.value().into_iter().map(ToString::to_string));
            }
        }
    }
    values
}

async fn submit_job(uri_text: &str, path: &str, options: PrintOptions) -> AppResult<()> {
    let uri = parse_uri(uri_text)?;
    let document = t::fs::read(path)
        .await
        .map_err(|error| format!("could not read '{path}': {error}"))?;
    if document.is_empty() {
        return Err(format!("refusing to print empty document '{path}'"));
    }
    let format = options
        .format
        .clone()
        .or_else(|| format_for_path(path).map(ToString::to_string))
        .ok_or_else(|| {
            String::from("unknown document format; pass --format with the exact MIME type")
        })?;

    let capabilities = get_capabilities(uri_text).await?;
    let supported = attribute_values(&capabilities, "document-format-supported");
    if !supported.is_empty() && !supported.iter().any(|value| value == &format) {
        return Err(format!(
            "printer does not advertise {format}; supported: {}",
            supported.join(", ")
        ));
    }
    let accepting = attribute_values(&capabilities, "printer-is-accepting-jobs");
    if accepting.first().is_some_and(|value| value == "false") {
        return Err(String::from("printer is not accepting jobs"));
    }

    let job_name = path.rsplit('/').next().unwrap_or(path);
    let mut builder = IppOperationBuilder::print_job(uri.clone(), IppPayload::empty())
        .user_name("trueos")
        .job_title(job_name)
        .document_format(&format);
    if let Some(copies) = options.copies {
        builder = builder.attribute(ipp_attribute("copies", IppValue::Integer(copies))?);
    }
    if let Some(media) = options.media {
        builder = builder.attribute(keyword_attribute("media", &media)?);
    }
    if let Some(sides) = options.sides {
        builder = builder.attribute(keyword_attribute("sides", &sides)?);
    }
    if let Some(color) = options.color {
        builder = builder.attribute(keyword_attribute("print-color-mode", &color)?);
    }
    if let Some(quality) = options.quality {
        builder = builder.attribute(ipp_attribute("print-quality", IppValue::Enum(quality))?);
    }
    let operation = builder
        .build()
        .map_err(|error| format!("could not build Print-Job: {error}"))?;
    let mut request = operation.into_ipp_request();
    request.header_mut().version = IppVersion::v2_0();
    let mut request_body = request.to_bytes().to_vec();
    request_body.extend_from_slice(&document);

    info(format!(
        "{APP}: submitting '{path}' as {format} to {uri_text}"
    ));
    let response = send_ipp(uri, request_body).await?;
    if !response.header().status_code().is_success() {
        return Err(format!(
            "printer rejected job: {}",
            response.header().status_code()
        ));
    }

    let job_id = attribute_values(&response, "job-id")
        .into_iter()
        .next()
        .unwrap_or_else(|| String::from("unknown"));
    let job_uri = attribute_values(&response, "job-uri")
        .into_iter()
        .next()
        .unwrap_or_else(|| String::from("not supplied"));
    info(format!(
        "{APP}: accepted; job-id={job_id} job-uri={job_uri}"
    ));
    Ok(())
}

async fn send_ipp(uri: Uri, body: Vec<u8>) -> AppResult<IppRequestResponse> {
    use t::io::{AsyncReadExt, AsyncWriteExt};

    if uri.scheme_str() == Some("ipps") {
        return Err(String::from(
            "IPPS was discovered but encrypted transport is not wired yet; use the printer's ipp:// service",
        ));
    }
    if uri.scheme_str() != Some("ipp") {
        return Err(String::from("unsupported printer URI scheme"));
    }
    let authority = uri
        .authority()
        .ok_or_else(|| String::from("printer URI has no host"))?;
    let host = authority.host();
    let port = authority.port_u16().unwrap_or(631);
    let path = uri
        .path_and_query()
        .map(|value| value.as_str())
        .filter(|value| !value.is_empty())
        .unwrap_or("/ipp/print");
    let address = format!("{host}:{port}");
    let mut stream = t::time::timeout(
        t::time::Duration::from_millis(IPP_TIMEOUT_MS),
        t::net::TcpStream::connect(address.as_str()),
    )
    .await
    .map_err(|_| format!("timed out connecting to {host}:{port}"))?
    .map_err(|error| format!("could not connect to {host}:{port}: {error}"))?;

    let request_head = format!(
        "POST {path} HTTP/1.1\r\nHost: {authority}\r\nUser-Agent: TRUEOS/{APP}\r\nContent-Type: application/ipp\r\nAccept: application/ipp\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    t::time::timeout(t::time::Duration::from_millis(IPP_TIMEOUT_MS), async {
        stream.write_all(request_head.as_bytes()).await?;
        stream.write_all(&body).await?;
        stream.flush().await
    })
    .await
    .map_err(|_| String::from("timed out sending IPP request"))?
    .map_err(|error| format!("could not send IPP request: {error}"))?;

    let mut response = Vec::new();
    let mut scratch = [0u8; 8_192];
    let deadline = t::time::Instant::now() + t::time::Duration::from_millis(IPP_TIMEOUT_MS);
    let mut expected_total = None;
    loop {
        if response.len() >= MAX_IPP_RESPONSE_BYTES {
            return Err(String::from("IPP response exceeded safety limit"));
        }
        let remaining = deadline.saturating_duration_since(t::time::Instant::now());
        if remaining.is_zero() {
            return Err(String::from("timed out reading IPP response"));
        }
        let read = t::time::timeout(remaining, stream.read(&mut scratch))
            .await
            .map_err(|_| String::from("timed out reading IPP response"))?
            .map_err(|error| format!("could not read IPP response: {error}"))?;
        if read == 0 {
            break;
        }
        response.extend_from_slice(&scratch[..read]);
        if let Some(header_end) = find_http_header_end(&response) {
            if expected_total.is_none()
                && let Some(length) = http_content_length(&response[..header_end])
            {
                expected_total = header_end.checked_add(length);
            }
            if http_is_chunked(&response[..header_end])
                && chunked_body_complete(&response[header_end..])?
            {
                break;
            }
        }
        if expected_total.is_some_and(|expected| response.len() >= expected) {
            break;
        }
    }

    let header_end = find_http_header_end(&response)
        .ok_or_else(|| String::from("printer returned an incomplete response header"))?;
    let status = http_status(&response[..header_end])
        .ok_or_else(|| String::from("printer returned an invalid response status"))?;
    if !(200..300).contains(&status) {
        return Err(format!(
            "printer transport rejected request with status {status}"
        ));
    }
    let headers = &response[..header_end];
    let wire_body = &response[header_end..];
    let ipp_body = if http_is_chunked(headers) {
        decode_chunked(wire_body)?
    } else if let Some(length) = http_content_length(headers) {
        wire_body
            .get(..length)
            .ok_or_else(|| String::from("truncated IPP response body"))?
            .to_vec()
    } else {
        wire_body.to_vec()
    };
    IppParser::new(Cursor::new(ipp_body))
        .parse()
        .map_err(|error| format!("invalid IPP response: {error}"))
}

fn find_http_header_end(data: &[u8]) -> Option<usize> {
    data.windows(4)
        .position(|window| window == b"\r\n\r\n")
        .map(|index| index + 4)
}

fn http_status(headers: &[u8]) -> Option<u16> {
    let line_end = headers.windows(2).position(|window| window == b"\r\n")?;
    let line = core::str::from_utf8(&headers[..line_end]).ok()?;
    line.split_ascii_whitespace().nth(1)?.parse().ok()
}

fn http_content_length(headers: &[u8]) -> Option<usize> {
    http_header(headers, "content-length")?.parse().ok()
}

fn http_is_chunked(headers: &[u8]) -> bool {
    http_header(headers, "transfer-encoding").is_some_and(|value| {
        value
            .split(',')
            .any(|item| item.trim().eq_ignore_ascii_case("chunked"))
    })
}

fn http_header<'a>(headers: &'a [u8], wanted: &str) -> Option<&'a str> {
    let text = core::str::from_utf8(headers).ok()?;
    text.lines().skip(1).find_map(|line| {
        let (name, value) = line.split_once(':')?;
        name.eq_ignore_ascii_case(wanted).then_some(value.trim())
    })
}

fn decode_chunked(data: &[u8]) -> AppResult<Vec<u8>> {
    let mut out = Vec::new();
    let mut offset = 0usize;
    loop {
        let line_end = data
            .get(offset..)
            .and_then(|rest| rest.windows(2).position(|window| window == b"\r\n"))
            .ok_or_else(|| String::from("truncated chunked IPP response"))?;
        let size_text = core::str::from_utf8(&data[offset..offset + line_end])
            .map_err(|_| String::from("invalid chunk size"))?;
        let size = usize::from_str_radix(size_text.split(';').next().unwrap_or("").trim(), 16)
            .map_err(|_| String::from("invalid chunk size"))?;
        offset += line_end + 2;
        if size == 0 {
            return Ok(out);
        }
        let end = offset
            .checked_add(size)
            .filter(|end| end.checked_add(2).is_some_and(|tail| tail <= data.len()))
            .ok_or_else(|| String::from("truncated chunked IPP data"))?;
        if out.len().saturating_add(size) > MAX_IPP_RESPONSE_BYTES {
            return Err(String::from("decoded IPP response exceeded safety limit"));
        }
        out.extend_from_slice(&data[offset..end]);
        if &data[end..end + 2] != b"\r\n" {
            return Err(String::from("invalid chunk terminator"));
        }
        offset = end + 2;
    }
}

fn chunked_body_complete(data: &[u8]) -> AppResult<bool> {
    let mut offset = 0usize;
    loop {
        let Some(line_end) = data
            .get(offset..)
            .and_then(|rest| rest.windows(2).position(|window| window == b"\r\n"))
        else {
            return Ok(false);
        };
        let size_text = core::str::from_utf8(&data[offset..offset + line_end])
            .map_err(|_| String::from("invalid chunk size"))?;
        let size = usize::from_str_radix(size_text.split(';').next().unwrap_or("").trim(), 16)
            .map_err(|_| String::from("invalid chunk size"))?;
        offset += line_end + 2;
        if size == 0 {
            return Ok(true);
        }
        let Some(end) = offset.checked_add(size) else {
            return Err(String::from("chunked IPP response length overflow"));
        };
        let Some(tail) = end.checked_add(2) else {
            return Err(String::from("chunked IPP response length overflow"));
        };
        if tail > data.len() {
            return Ok(false);
        }
        if &data[end..tail] != b"\r\n" {
            return Err(String::from("invalid chunk terminator"));
        }
        offset = tail;
    }
}

fn ipp_attribute(name: &str, value: IppValue) -> AppResult<IppAttribute> {
    IppAttribute::with_name(name, value)
        .map_err(|error| format!("invalid IPP attribute '{name}': {error}"))
}

fn keyword_attribute(name: &str, value: &str) -> AppResult<IppAttribute> {
    let value = value
        .try_into()
        .map_err(|error| format!("invalid IPP keyword '{value}': {error}"))?;
    ipp_attribute(name, IppValue::Keyword(value))
}

fn parse_print_options(args: Vec<String>) -> AppResult<PrintOptions> {
    let mut result = PrintOptions::default();
    let mut index = 0usize;
    while index < args.len() {
        let flag = &args[index];
        let value = args
            .get(index + 1)
            .ok_or_else(|| format!("{flag} requires a value"))?;
        match flag.as_str() {
            "--format" => result.format = Some(value.clone()),
            "--copies" => {
                let copies = value
                    .parse::<i32>()
                    .map_err(|_| format!("invalid copy count '{value}'"))?;
                if !(1..=999).contains(&copies) {
                    return Err(String::from("copy count must be between 1 and 999"));
                }
                result.copies = Some(copies);
            }
            "--media" => result.media = Some(value.clone()),
            "--sides" => result.sides = Some(value.clone()),
            "--color" => result.color = Some(value.clone()),
            "--quality" => {
                result.quality = Some(match value.as_str() {
                    "draft" | "3" => 3,
                    "normal" | "4" => 4,
                    "high" | "5" => 5,
                    _ => return Err(format!("invalid print quality '{value}'")),
                });
            }
            _ => return Err(format!("unknown print option '{flag}'")),
        }
        index += 2;
    }
    Ok(result)
}

fn parse_u64(value: &str, name: &str) -> AppResult<u64> {
    value
        .parse::<u64>()
        .map_err(|_| format!("invalid {name}: '{value}'"))
}

fn format_for_path(path: &str) -> Option<&'static str> {
    let extension = path.rsplit('.').next()?.to_ascii_lowercase();
    match extension.as_str() {
        "pdf" => Some("application/pdf"),
        "jpg" | "jpeg" => Some("image/jpeg"),
        "pwg" => Some("image/pwg-raster"),
        "urf" => Some("image/urf"),
        "ps" => Some("application/postscript"),
        "pcl" => Some("application/vnd.hp-pcl"),
        "txt" => Some("text/plain"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mdns_query_contains_all_driverless_services() {
        let query = build_mdns_query(&MDNS_SERVICES).unwrap();
        assert_eq!(read_u16(&query, 4).unwrap(), 3);
        assert!(query.windows(3).any(|window| window == b"ipp"));
        assert!(query.windows(4).any(|window| window == b"ipps"));
    }

    #[test]
    fn dns_compression_pointer_is_decoded() {
        let packet = [
            3, b'f', b'o', b'o', 5, b'l', b'o', b'c', b'a', b'l', 0, 3, b'b', b'a', b'r', 0xc0,
            0x00,
        ];
        let mut offset = 11;
        assert_eq!(
            read_dns_name(&packet, &mut offset).unwrap(),
            "bar.foo.local."
        );
        assert_eq!(offset, packet.len());
    }

    #[test]
    fn txt_and_formats_are_parsed() {
        let txt = b"\x0crp=ipp/print\x15pdl=application/pdf";
        let values = parse_txt(txt).unwrap();
        assert_eq!(values.get("rp").map(String::as_str), Some("ipp/print"));
        assert_eq!(
            values.get("pdl").map(String::as_str),
            Some("application/pdf")
        );
        assert_eq!(format_for_path("page.PDF"), Some("application/pdf"));
        assert_eq!(format_for_path("photo.jpeg"), Some("image/jpeg"));
    }

    #[test]
    fn print_options_cover_common_ipp_job_template_attributes() {
        let options = parse_print_options(vec![
            String::from("--copies"),
            String::from("2"),
            String::from("--sides"),
            String::from("two-sided-long-edge"),
            String::from("--quality"),
            String::from("high"),
        ])
        .unwrap();
        assert_eq!(options.copies, Some(2));
        assert_eq!(options.sides.as_deref(), Some("two-sided-long-edge"));
        assert_eq!(options.quality, Some(5));
    }

    #[test]
    fn chunked_ipp_body_is_detected_and_decoded() {
        let incomplete = b"4\r\nIPP/\r\n5\r\n2.0";
        assert!(!chunked_body_complete(incomplete).unwrap());

        let complete = b"4\r\nIPP/\r\n3\r\n2.0\r\n0\r\n\r\n";
        assert!(chunked_body_complete(complete).unwrap());
        assert_eq!(decode_chunked(complete).unwrap(), b"IPP/2.0");
    }
}
