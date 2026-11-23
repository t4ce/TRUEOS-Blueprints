//! Finite service/temperature/audio handoff checks against a warm ptts sidekick.
use std::io::{Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpStream};
use std::time::{Duration, Instant};

type Result<T> = std::result::Result<T, String>;
const DEFAULT_PORT: u16 = 9773;
const LILY_PORT: u16 = 9771;
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
    let stream = send_request(port, method, path, body, deadline)?;
    read_response(stream, deadline)
}

fn send_request(
    port: u16,
    method: &str,
    path: &str,
    body: Option<serde_json::Value>,
    deadline: Instant,
) -> Result<TcpStream> {
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
    Ok(stream)
}

fn read_response(mut stream: TcpStream, deadline: Instant) -> Result<Response> {
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
fn audio_status() -> Result<serde_json::Value> {
    require_status(
        request(82, "GET", "/audio/status", None, SMALL_TIMEOUT)?,
        200,
    )
}

#[cfg(target_os = "trueos")]
fn clip_id() -> Result<u64> {
    audio_status()?["clip_id"]
        .as_str()
        .ok_or("audio status has no string clip_id")?
        .parse::<u64>()
        .map_err(|e| e.to_string())
}

#[cfg(target_os = "trueos")]
fn check_live_speech(port: u16, body: serde_json::Value) -> Result<Response> {
    use std::io::BufReader;

    if audio_status()?["playing"] != false {
        return Err("live onset measurement requires an idle audio sink".into());
    }
    let deadline = Instant::now() + SMALL_TIMEOUT;
    let stream = send_request(82, "GET", "/audio/live.wav", None, deadline)?;
    stream
        .set_read_timeout(Some(remaining(deadline)?))
        .map_err(|e| e.to_string())?;
    let mut live = BufReader::new(stream);
    let mut header = Vec::new();
    while !header.ends_with(b"\r\n\r\n") {
        if header.len() == MAX_HEADER {
            return Err("live audio HTTP headers exceed bound".into());
        }
        let mut byte = [0];
        live.read_exact(&mut byte).map_err(|e| e.to_string())?;
        header.push(byte[0]);
    }
    let mut wav = [0; 44];
    live.read_exact(&mut wav).map_err(|e| e.to_string())?;
    if !header.starts_with(b"HTTP/1.1 200 ")
        || &wav[0..4] != b"RIFF"
        || &wav[8..12] != b"WAVE"
        || &wav[20..28] != [1, 0, 2, 0, 128, 187, 0, 0]
        || &wav[34..40] != b"\x10\x00data"
    {
        return Err("unexpected live audio WAV format".into());
    }

    let started = Instant::now();
    let speech_deadline = started + SPEECH_TIMEOUT;
    let speech = send_request(port, "POST", "/speech", Some(body), speech_deadline)?;
    let live_deadline = started + SMALL_TIMEOUT;
    let mut block = [0u8; 1920];
    let mut first_nonzero_ms = None;
    let onset_ms = loop {
        live.get_ref()
            .set_read_timeout(Some(remaining(live_deadline)?))
            .map_err(|e| e.to_string())?;
        live.read_exact(&mut block)
            .map_err(|e| format!("read live audio: {e}"))?;
        let elapsed = started.elapsed().as_millis();
        if first_nonzero_ms.is_none() && block.iter().any(|byte| *byte != 0) {
            first_nonzero_ms = Some(elapsed);
        }
        // Detect a 10 ms window above roughly -60 dBFS rather than counting
        // digital silence or reporting the browser's audio device latency.
        let sum: u64 = block
            .chunks_exact(2)
            .map(|b| {
                let sample = i32::from(i16::from_le_bytes([b[0], b[1]]));
                (sample * sample) as u64
            })
            .sum();
        if sum >= (block.len() as u64 / 2) * 33 * 33 {
            break elapsed;
        }
    };
    drop(live);
    let response = read_response(speech, speech_deadline)?;
    let first_pcm_ms = response.json["first_pcm_ms"]
        .as_u64()
        .ok_or("missing first_pcm_ms")?;
    let first_stream_ms = response.json["first_stream_ms"]
        .as_u64()
        .ok_or("missing first_stream_ms")?;
    let generation_ms = response.json["generation_ms"]
        .as_u64()
        .ok_or("missing generation_ms")?;
    if onset_ms >= u128::from(generation_ms) {
        return Err(format!(
            "live PCM onset {onset_ms} ms did not precede generation end {generation_ms} ms"
        ));
    }
    notice(format!(
        "ptts-check: live first_pcm_ms={first_pcm_ms} first_stream_ms={first_stream_ms} first_nonzero_ms={} energy_onset_ms={onset_ms} generation_ms={generation_ms}",
        first_nonzero_ms.unwrap_or(onset_ms),
    ));
    Ok(response)
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
    let lily_ready =
        request(LILY_PORT, "GET", "/health", None, SMALL_TIMEOUT).is_ok_and(|response| {
            response.status == 200
                && response.json["protocol"] == "lily2vec-v1"
                && response.json["ready"] == true
        });
    let (automatic_case, automatic_temperature) = if lily_ready {
        ("Lily/Happy", 0.4)
    } else {
        ("default/Lily-unavailable", 0.3)
    };
    for (label, temperature, text) in [
        (
            None,
            automatic_temperature,
            "I am so happy. This is wonderful news!",
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
        // Match Codex's text-only request when Pocket should ask Lily itself.
        let body = match label {
            Some(label) => serde_json::json!({"text":text,"label":label}),
            None => serde_json::json!({"text":text}),
        };
        #[cfg(target_os = "trueos")]
        let response = if label.is_none() {
            check_live_speech(port, body)?
        } else {
            request(port, "POST", "/speech", Some(body), SPEECH_TIMEOUT)?
        };
        #[cfg(not(target_os = "trueos"))]
        let response = request(port, "POST", "/speech", Some(body), SPEECH_TIMEOUT)?;
        let label = label.unwrap_or(automatic_case);
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

#[cfg(target_os = "trueos")]
fn source_active(id: u64) -> Result<bool> {
    unsafe extern "C" {
        fn trueos_cabi_http_audio_stream_v1(
            action: u32,
            stream: u64,
            pcm: *const u8,
            bytes: usize,
        ) -> i64;
    }
    match unsafe { trueos_cabi_http_audio_stream_v1(4, id, core::ptr::null(), 0) } {
        0 => Ok(false),
        1 => Ok(true),
        error => Err(format!("native audio status query failed: {error}")),
    }
}

fn run_turn_cues(port: u16) -> Result<()> {
    let status = require_status(request(port, "GET", "/health", None, SMALL_TIMEOUT)?, 200)?;
    if status["turn_cues"] != true {
        return Err("service has no turn cues".into());
    }
    #[cfg(target_os = "trueos")]
    let mut previous = None;
    for (index, intro) in ["startthink", "starttalk"].into_iter().enumerate() {
        let response = require_status(
            request(
                port,
                "POST",
                "/speech",
                Some(serde_json::json!({
                    "text":if index == 0 { "I will check this." } else { "The sounds are ready." },
                    "label":"Static", "intro":intro,
                })),
                SPEECH_TIMEOUT,
            )?,
            200,
        )?;
        check_speech(&response, 0.3)?;
        #[cfg(target_os = "trueos")]
        {
            if response["first_stream_ms"]
                .as_u64()
                .ok_or("missing cue write timing")?
                > response["first_pcm_ms"]
                    .as_u64()
                    .ok_or("missing generated PCM timing")?
            {
                return Err("intro was not submitted before generated speech".into());
            }
            if let Some(old) = previous {
                if source_active(old)? {
                    return Err("previous Pocket speech still playing".into());
                }
            }
            let id = response["stream_id"]
                .as_u64()
                .ok_or("missing speech source ID")?;
            if !source_active(id)? {
                return Err("speech unexpectedly drained before the transition check".into());
            }
            previous = Some(id);
        }
        notice(format!(
            "ptts-check: cue={intro} first_stream_ms={} first_pcm_ms={} audio_ms={} stream_id={}",
            response["first_stream_ms"],
            response["first_pcm_ms"],
            response["audio_ms"],
            response["stream_id"]
        ));
    }
    let response = require_status(
        request(
            port,
            "POST",
            "/cue",
            Some(serde_json::json!({"cue":"done"})),
            SPEECH_TIMEOUT,
        )?,
        200,
    )?;
    #[cfg(target_os = "trueos")]
    {
        if source_active(previous.ok_or("missing previous speech")?)? {
            return Err("done overlapped the final speech".into());
        }
        let id = response["stream_id"]
            .as_u64()
            .ok_or("missing done source ID")?;
        if !source_active(id)? {
            return Err("done has no queued source".into());
        }
        notice(format!(
            "ptts-check: done stream_id={id}; previous speech drained"
        ));
    }
    #[cfg(not(target_os = "trueos"))]
    let _ = response;
    notice("ptts-check: all turn-cue checks passed");
    Ok(())
}

fn entry() -> Result<()> {
    let mut port = DEFAULT_PORT;
    // Shell3's normal Blueprint launch has no argv. Native bare launch checks
    // the cue experiment; Ubuntu retains the existing service-policy checks.
    let mut turn_cues = cfg!(target_os = "trueos");
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--turn-cues" => turn_cues = true,
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
                    "ptts-check [--port PORT] [--turn-cues]\nFinite checks against a running ptts service (default 9773). Native bare launch checks turn cues; host defaults to speech policy checks."
                );
                return Ok(());
            }
            _ => return Err(format!("unknown argument {arg:?}")),
        }
    }
    if turn_cues {
        run_turn_cues(port)
    } else {
        run(port)
    }
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
