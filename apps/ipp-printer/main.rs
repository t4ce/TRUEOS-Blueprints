use std::io::Cursor;

use ipp::prelude::{
    IppAttribute, IppOperationBuilder, IppPayload, IppRequestResponse, IppValue, IppVersion, Uri,
};
use ipp::{operation::IppOperation, parser::IppParser};
use trueos::{
    env,
    logl::{self, level},
    platform::{String, ToString, Vec, format},
    printers, t,
};

const APP: &str = "ipp-printer";
const PRINTER_REGISTRY_WAIT_MS: u64 = 16_000;
const PRINTER_REGISTRY_POLL_MS: u64 = 250;
const IPP_TIMEOUT_MS: u64 = 30_000;
const MAX_IPP_RESPONSE_BYTES: usize = 8 * 1024 * 1024;
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
        return print_known_printers().await;
    };

    match command.as_str() {
        "printers" | "list" => print_known_printers().await,
        "info" | "status" => {
            let destination = args
                .next()
                .ok_or_else(|| String::from("info requires 'auto' or an IPP URI"))?;
            let uri = resolve_destination(&destination).await?;
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
            let uri = resolve_destination(&destination).await?;
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
    info(format!("usage: {APP} printers"));
    info(format!("       {APP} info <auto|IPP-URI>"));
    info(format!(
        "       {APP} print <auto|IPP-URI> <document> [--format MIME] [--copies N]"
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

async fn print_known_printers() -> AppResult<()> {
    let printers = match wait_for_known_printers().await {
        Ok(printers) => printers,
        Err(_) => {
            warn(format!(
                "{APP}: kernel printer registry is empty; direct ipp:// URIs remain available"
            ));
            return Ok(());
        }
    };
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

async fn choose_auto_printer() -> AppResult<printers::Printer> {
    let mut printers = wait_for_known_printers().await?;
    printers.sort_by_key(|printer| printer.secure);
    printers.into_iter().next().ok_or_else(|| {
        String::from("kernel printer registry is empty; pass an explicit ipp:// URI")
    })
}

async fn wait_for_known_printers() -> AppResult<Vec<printers::Printer>> {
    let deadline =
        t::time::Instant::now() + t::time::Duration::from_millis(PRINTER_REGISTRY_WAIT_MS);
    loop {
        let printers = printers::snapshot()
            .map_err(|code| format!("could not read kernel printer registry: {code}"))?;
        if !printers.is_empty() {
            return Ok(printers);
        }
        if t::time::Instant::now() >= deadline {
            return Err(String::from("kernel printer registry is empty"));
        }
        t::time::sleep(t::time::Duration::from_millis(PRINTER_REGISTRY_POLL_MS)).await;
    }
}

async fn resolve_destination(destination: &str) -> AppResult<String> {
    if destination == "auto" {
        Ok(choose_auto_printer().await?.uri)
    } else {
        Ok(destination.to_string())
    }
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
    if !supported.is_empty()
        && !supported
            .iter()
            .any(|value| value.eq_ignore_ascii_case(&format))
    {
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
    fn common_print_formats_are_inferred() {
        assert_eq!(format_for_path("page.PDF"), Some("application/pdf"));
        assert_eq!(format_for_path("photo.jpeg"), Some("image/jpeg"));
        assert_eq!(format_for_path("page.pwg"), Some("image/pwg-raster"));
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
