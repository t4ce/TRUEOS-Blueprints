//! Finite service/temperature/audio handoff checks against a warm ptts sidekick.
use std::io::{Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpStream};
use std::time::{Duration, Instant};

type Result<T> = std::result::Result<T, String>;
const DEFAULT_PORT: u16 = 9773;
const MAX_HEADER: usize = 8192;
const MAX_BODY: usize = 8192;
const SPEECH_TIMEOUT: Duration = Duration::from_secs(185);
const SMALL_TIMEOUT: Duration = Duration::from_secs(5);

fn notice(message: impl AsRef<str>) {
    let text = message.as_ref();
    #[cfg(target_os = "trueos")]
    {
        trueos::logl::log(trueos::logl::level::INFO, text);
        let _ =
            trueos::logl::log_record(trueos::logl::level::IMPORTANT, "blueprint/ptts-check", text);
    }
    #[cfg(not(target_os = "trueos"))]
    println!("{text}");
}

fn remaining(deadline: Instant) -> Result<Duration> {
    let duration = deadline.saturating_duration_since(Instant::now());
    if duration.is_zero() {
        Err("HTTP request deadline expired".into())
    } else {
        Ok(duration)
    }
}

struct Response {
    status: u16,
    json: serde_json::Value,
}

fn response_header(header: &[u8]) -> Result<(u16, usize)> {
    if header.len() > MAX_HEADER {
        return Err("response headers exceed bound".into());
    }
    let header = std::str::from_utf8(header).map_err(|e| e.to_string())?;
    let mut lines = header.split("\r\n");
    let mut status = lines.next().unwrap_or_default().split_whitespace();
    if !matches!(status.next(), Some("HTTP/1.1" | "HTTP/1.0")) {
        return Err("invalid HTTP response version".into());
    }
    let code = status
        .next()
        .ok_or("missing HTTP status")?
        .parse::<u16>()
        .map_err(|e| e.to_string())?;
    let mut length = None;
    for line in lines.filter(|line| !line.is_empty()) {
        let (name, value) = line.split_once(':').ok_or("malformed response header")?;
        if name.eq_ignore_ascii_case("transfer-encoding") {
            return Err("transfer encoding unsupported by probe".into());
        }
        if name.eq_ignore_ascii_case("content-length") {
            if length.is_some() {
                return Err("duplicate Content-Length".into());
            }
            length = Some(value.trim().parse::<usize>().map_err(|e| e.to_string())?);
        }
    }
    let length = length.ok_or("missing response Content-Length")?;
    if length > MAX_BODY {
        return Err("response body exceeds bound".into());
    }
    Ok((code, length))
}

fn request(
    port: u16,
    method: &str,
    path: &str,
    body: Option<serde_json::Value>,
    timeout: Duration,
) -> Result<Response> {
    let deadline = Instant::now() + timeout;
    let mut stream = TcpStream::connect_timeout(
        &SocketAddr::from((Ipv4Addr::LOCALHOST, port)),
        remaining(deadline)?.min(SMALL_TIMEOUT),
    )
    .map_err(|e| format!("connect 127.0.0.1:{port}: {e}"))?;
    let body = body
        .map(|value| serde_json::to_vec(&value))
        .transpose()
        .map_err(|e| e.to_string())?
        .unwrap_or_default();
    let mut wire = format!(
        "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    )
    .into_bytes();
    wire.extend_from_slice(&body);
    let mut pending = wire.as_slice();
    while !pending.is_empty() {
        stream
            .set_write_timeout(Some(remaining(deadline)?.min(SMALL_TIMEOUT)))
            .map_err(|e| e.to_string())?;
        let count = stream.write(pending).map_err(|e| e.to_string())?;
        if count == 0 {
            return Err("HTTP write made no progress".into());
        }
        pending = &pending[count..];
    }
    let mut reply = Vec::new();
    let mut expected = None;
    let mut buffer = [0u8; 2048];
    loop {
        if let Some((status, start, length)) = expected {
            if reply.len() == start + length {
                let json = serde_json::from_slice(&reply[start..]).map_err(|e| e.to_string())?;
                return Ok(Response { status, json });
            }
            if reply.len() > start + length {
                return Err("HTTP response exceeds declared Content-Length".into());
            }
        }
        stream
            .set_read_timeout(Some(remaining(deadline)?))
            .map_err(|e| e.to_string())?;
        let count = stream.read(&mut buffer).map_err(|e| e.to_string())?;
        if count == 0 {
            return Err("HTTP response ended before complete body".into());
        }
        if reply.len() + count > MAX_HEADER + MAX_BODY {
            return Err("HTTP response exceeds bound".into());
        }
        reply.extend_from_slice(&buffer[..count]);
        if expected.is_none() {
            if let Some(end) = reply.windows(4).position(|bytes| bytes == b"\r\n\r\n") {
                let start = end + 4;
                let (status, length) = response_header(&reply[..start])?;
                expected = Some((status, start, length));
            } else if reply.len() >= MAX_HEADER {
                return Err("HTTP response headers exceed bound".into());
            }
        }
    }
}

fn require_status(response: Response, expected: u16) -> Result<serde_json::Value> {
    if response.status != expected {
        return Err(format!(
            "expected HTTP {expected}, got {}: {}",
            response.status, response.json
        ));
    }
    Ok(response.json)
}

fn health(port: u16) -> Result<()> {
    let json = require_status(request(port, "GET", "/health", None, SMALL_TIMEOUT)?, 200)?;
    if json["protocol"] != "ptts-v1"
        || json["ready"] != true
        || json["tts"] != true
        || json["language"] != "en"
        || json["voice"] != "azelma"
    {
        return Err(format!("unexpected ptts health: {json}"));
    }
    Ok(())
}

#[cfg(target_os = "trueos")]
fn clip_id() -> Result<u64> {
    let json = require_status(
        request(82, "GET", "/audio/status", None, SMALL_TIMEOUT)?,
        200,
    )?;
    json["clip_id"]
        .as_str()
        .ok_or("audio status has no string clip_id")?
        .parse::<u64>()
        .map_err(|e| e.to_string())
}

fn check_speech(json: &serde_json::Value, expected_temperature: f64) -> Result<(u64, u64)> {
    let temperature = json["temperature"].as_f64().ok_or("missing temperature")?;
    let audio = json["audio_ms"].as_u64().ok_or("missing audio_ms")?;
    let chunks = json["chunks"].as_u64().ok_or("missing chunks")?;
    if json["ok"] != true
        || (temperature - expected_temperature).abs() > 0.000001
        || audio == 0
        || chunks == 0
    {
        return Err(format!("unexpected speech response: {json}"));
    }
    Ok((audio, chunks))
}

fn run(port: u16) -> Result<()> {
    health(port)?;
    notice(format!(
        "ptts-check: ready port={port} protocol=ptts-v1 voice=azelma"
    ));
    for (label, temperature, text) in [
        (
            None,
            0.3,
            "Codex can read this completed answer using the default Azelma voice.",
        ),
        (
            Some("Happy"),
            0.4,
            "The build passed, and TRUEOS can now read this message with its new CPU voice.",
        ),
        (
            Some("Tired"),
            0.2,
            "This second sentence uses the tired label while the cpu prepares the next response.",
        ),
    ] {
        #[cfg(target_os = "trueos")]
        let before = clip_id()?;
        let started = Instant::now();
        // Match Codex's text-only request exactly when no Lily label is present.
        let body = match label {
            Some(label) => serde_json::json!({"text":text,"label":label}),
            None => serde_json::json!({"text":text}),
        };
        let label = label.unwrap_or("default");
        let response = request(port, "POST", "/speech", Some(body), SPEECH_TIMEOUT)?;
        let json = require_status(response, 200)?;
        let (audio_ms, chunks) = check_speech(&json, temperature)?;
        #[cfg(target_os = "trueos")]
        {
            let after = clip_id()?;
            if after <= before {
                return Err(format!(
                    "{label}: audio clip ID did not advance ({before}->{after})"
                ));
            }
            notice(format!(
                "ptts-check: label={label} temp={temperature} audio_ms={audio_ms} chunks={chunks} clip={before}->{after} wall_ms={}",
                started.elapsed().as_millis()
            ));
        }
        #[cfg(not(target_os = "trueos"))]
        notice(format!(
            "ptts-check: label={label} temp={temperature} audio_ms={audio_ms} chunks={chunks} wall_ms={} native_audio=skipped",
            started.elapsed().as_millis()
        ));
    }
    #[cfg(target_os = "trueos")]
    let before = clip_id()?;
    let emoji = require_status(
        request(
            port,
            "POST",
            "/speech",
            Some(serde_json::json!({"text":"😄😭👍"})),
            SPEECH_TIMEOUT,
        )?,
        200,
    )?;
    if emoji["ok"] != true || emoji["audio_ms"] != 0 || emoji["chunks"] != 0 {
        return Err(format!("emoji-only speech was not silent: {emoji}"));
    }
    #[cfg(target_os = "trueos")]
    if clip_id()? != before {
        return Err("emoji-only input changed native audio clip ID".into());
    }
    notice("ptts-check: emoji-only silent; clip unchanged");

    let rejected = request(
        port,
        "POST",
        "/speech",
        Some(serde_json::json!({"text":"This voice must be rejected.","voice":"af_heart"})),
        SMALL_TIMEOUT,
    )?;
    let error = require_status(rejected, 422)?;
    if error["error"].as_str().is_none_or(str::is_empty) {
        return Err("invalid voice did not return an error".into());
    }
    health(port)?;
    notice("ptts-check: invalid voice rejected; health remains ready");
    notice(format!(
        "ptts-check: all checks passed native_audio={}",
        cfg!(target_os = "trueos")
    ));
    Ok(())
}

fn entry() -> Result<()> {
    let mut port = DEFAULT_PORT;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--port" => {
                port = args
                    .next()
                    .ok_or("--port needs a value")?
                    .parse::<u16>()
                    .map_err(|e| e.to_string())?;
                if port == 0 {
                    return Err("--port must be nonzero".into());
                }
            }
            "--help" | "-h" => {
                println!(
                    "ptts-check [--port PORT]\nFinite checks against a running ptts service (default 9773). Host skips native audio clip checks."
                );
                return Ok(());
            }
            _ => return Err(format!("unknown argument {arg:?}")),
        }
    }
    run(port)
}

fn main() {
    if let Err(error) = entry() {
        notice(format!("ptts-check: FAILED {error}"));
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_ambiguous_and_oversized_response_framing() {
        assert_eq!(
            response_header(b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\n\r\n").unwrap(),
            (200, 4)
        );
        for header in [
            "HTTP/1.1 200 OK\r\n\r\n".to_owned(),
            "HTTP/1.1 200 OK\r\nContent-Length: 1\r\nContent-Length: 1\r\n\r\n".to_owned(),
            "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n".to_owned(),
            format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n",
                MAX_BODY + 1
            ),
        ] {
            assert!(response_header(header.as_bytes()).is_err());
        }
    }

    #[test]
    fn checks_real_audio_and_policy_with_float_tolerance() {
        let good = serde_json::json!({"ok":true,"audio_ms":4000,"chunks":1,"temperature":0.4000000059604645});
        assert_eq!(check_speech(&good, 0.4).unwrap(), (4000, 1));
        for bad in [
            serde_json::json!({"ok":true,"audio_ms":0,"chunks":1,"temperature":0.4}),
            serde_json::json!({"ok":true,"audio_ms":4000,"chunks":0,"temperature":0.4}),
            serde_json::json!({"ok":true,"audio_ms":4000,"chunks":1,"temperature":0.3}),
        ] {
            assert!(check_speech(&bad, 0.4).is_err());
        }
    }
}
