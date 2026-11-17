use anyhow::{Result, bail};

#[cfg(any(target_os = "trueos", test))]
use std::time::{Duration, Instant};

#[cfg(any(target_os = "trueos", test))]
const STATUS_HEADER_LIMIT: usize = 8192;
#[cfg(any(target_os = "trueos", test))]
const STATUS_BODY_LIMIT: usize = 1024;
#[cfg(any(target_os = "trueos", test))]
const IDLE_POLL_INTERVAL: Duration = Duration::from_millis(25);

#[cfg(target_os = "trueos")]
unsafe extern "C" {
    fn trueos_cabi_http_audio_clip_submit_v1(pcm_ptr: *const u8, pcm_bytes: usize) -> i32;
}

#[cfg(target_os = "trueos")]
pub fn submit(mono: &[f32]) -> Result<()> {
    submit_with_deadline(mono, Instant::now() + Duration::from_secs(120))
}

/// Wait for the existing complete-clip sink before handing off this utterance.
/// The caller's deadline includes synthesis, conversion and this wait.
///
/// This prevents overlap between jobs owned by this serial service. The old
/// kernel ABI cannot reserve the sink: another producer can still win between
/// the final status check and submit, whose success code includes busy discard.
#[cfg(target_os = "trueos")]
pub fn submit_with_deadline(mono: &[f32], deadline: Instant) -> Result<()> {
    if Instant::now() >= deadline {
        bail!("native playback deadline expired before conversion")
    }
    if mono.is_empty() || mono.len() > 24_000 * 120 {
        bail!("invalid native clip length")
    }
    let mut stereo = vec![0i16; mono.len() * 4];
    trueos_kokoro_audio::convert_frame_range(mono, 0, &mut stereo)
        .map_err(|e| anyhow::anyhow!("PCM conversion failed: {e:?}"))?;
    let bytes: Vec<u8> = stereo.iter().flat_map(|s| s.to_le_bytes()).collect();
    wait_until_idle_with(
        deadline,
        || sink_playing(deadline),
        Instant::now,
        std::thread::sleep,
    )?;
    let status = unsafe { trueos_cabi_http_audio_clip_submit_v1(bytes.as_ptr(), bytes.len()) };
    if status != 0 {
        bail!("native audio handoff failed: {status}")
    }
    crate::notice(format!(
        "ptts: clip submitted bytes={} format=stereo-s16le-48000",
        bytes.len()
    ));
    Ok(())
}

#[cfg(any(target_os = "trueos", test))]
fn wait_until_idle_with(
    deadline: Instant,
    mut playing: impl FnMut() -> Result<bool>,
    mut now: impl FnMut() -> Instant,
    mut pause: impl FnMut(Duration),
) -> Result<()> {
    loop {
        if now() >= deadline {
            bail!("native playback deadline expired while waiting for idle audio sink")
        }
        let busy = playing()
            .map_err(|error| anyhow::anyhow!("native audio status check failed: {error:#}"))?;
        let remaining = deadline.saturating_duration_since(now());
        if remaining.is_zero() {
            bail!("native playback deadline expired while checking audio sink")
        }
        if !busy {
            return Ok(());
        }
        pause(IDLE_POLL_INTERVAL.min(remaining));
    }
}

#[cfg(target_os = "trueos")]
fn status_budget(deadline: Instant) -> Result<Duration> {
    let remaining = deadline.saturating_duration_since(Instant::now());
    if remaining.is_zero() {
        bail!("audio status request deadline expired")
    }
    Ok(remaining.min(Duration::from_secs(1)))
}

#[cfg(target_os = "trueos")]
fn sink_playing(deadline: Instant) -> Result<bool> {
    use std::io::{Read, Write};
    use std::net::{Ipv4Addr, SocketAddr, TcpStream};

    let endpoint = SocketAddr::from((Ipv4Addr::LOCALHOST, 82));
    let mut stream = TcpStream::connect_timeout(&endpoint, status_budget(deadline)?)
        .map_err(|error| anyhow::anyhow!("connect 127.0.0.1:82: {error}"))?;
    let mut request =
        &b"GET /audio/status HTTP/1.1\r\nHost: 127.0.0.1:82\r\nConnection: close\r\n\r\n"[..];
    while !request.is_empty() {
        stream.set_write_timeout(Some(status_budget(deadline)?))?;
        let written = stream.write(request)?;
        if written == 0 {
            bail!("audio status request write made no progress")
        }
        request = &request[written..];
    }

    let mut response = Vec::new();
    let mut expected = None;
    let mut buffer = [0u8; 2048];
    loop {
        if let Some((body_start, body_len)) = expected {
            if response.len() == body_start + body_len {
                return status_playing(&response[body_start..]);
            }
            if response.len() > body_start + body_len {
                bail!("audio status response has bytes beyond Content-Length")
            }
        }
        stream.set_read_timeout(Some(status_budget(deadline)?))?;
        let count = stream.read(&mut buffer)?;
        if count == 0 {
            bail!("audio status response ended before its complete body")
        }
        if response.len() + count > STATUS_HEADER_LIMIT + STATUS_BODY_LIMIT {
            bail!("audio status response is too large")
        }
        response.extend_from_slice(&buffer[..count]);
        if expected.is_none() {
            if let Some(end) = response.windows(4).position(|bytes| bytes == b"\r\n\r\n") {
                let body_start = end + 4;
                if body_start > STATUS_HEADER_LIMIT {
                    bail!("audio status response headers are too large")
                }
                expected = Some((body_start, status_body_length(&response[..body_start])?));
            } else if response.len() >= STATUS_HEADER_LIMIT {
                bail!("audio status response headers are too large")
            }
        }
    }
}

#[cfg(any(target_os = "trueos", test))]
fn status_body_length(header: &[u8]) -> Result<usize> {
    if header.len() > STATUS_HEADER_LIMIT {
        bail!("audio status response headers are too large")
    }
    let header = std::str::from_utf8(header)?;
    let mut lines = header.split("\r\n");
    let mut status = lines.next().unwrap_or_default().split_whitespace();
    if !matches!(status.next(), Some("HTTP/1.1" | "HTTP/1.0")) || status.next() != Some("200") {
        bail!("audio status endpoint did not return HTTP 200")
    }
    let mut length = None;
    for line in lines.filter(|line| !line.is_empty()) {
        let (name, value) = line
            .split_once(':')
            .ok_or_else(|| anyhow::anyhow!("malformed audio status header"))?;
        if name.eq_ignore_ascii_case("transfer-encoding") {
            bail!("audio status response must not use transfer encoding")
        }
        if name.eq_ignore_ascii_case("content-length") {
            if length.is_some() {
                bail!("duplicate audio status Content-Length")
            }
            length = Some(value.trim().parse::<usize>()?);
        }
    }
    let length =
        length.ok_or_else(|| anyhow::anyhow!("audio status response has no Content-Length"))?;
    if length > STATUS_BODY_LIMIT {
        bail!("audio status JSON body is too large")
    }
    Ok(length)
}

#[cfg(any(target_os = "trueos", test))]
fn status_playing(body: &[u8]) -> Result<bool> {
    if body.len() > STATUS_BODY_LIMIT {
        bail!("audio status JSON body is too large")
    }
    let value: serde_json::Value = serde_json::from_slice(body)?;
    value
        .get("playing")
        .and_then(|value| value.as_bool())
        .ok_or_else(|| anyhow::anyhow!("audio status JSON has no boolean playing field"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use std::collections::VecDeque;

    #[test]
    fn busy_sink_waits_until_idle_before_handoff_can_continue() {
        let start = Instant::now();
        let clock = Cell::new(start);
        let calls = Cell::new(0);
        let mut states = VecDeque::from([true, true, false]);
        wait_until_idle_with(
            start + Duration::from_secs(1),
            || {
                calls.set(calls.get() + 1);
                Ok(states.pop_front().unwrap())
            },
            || clock.get(),
            |duration| clock.set(clock.get() + duration),
        )
        .unwrap();
        assert_eq!(calls.get(), 3);
        assert_eq!(clock.get().duration_since(start), IDLE_POLL_INTERVAL * 2);
        assert!(states.is_empty());
    }

    #[test]
    fn busy_sink_has_a_finite_wait_and_expired_work_never_polls() {
        let start = Instant::now();
        let clock = Cell::new(start);
        let calls = Cell::new(0);
        let error = wait_until_idle_with(
            start + IDLE_POLL_INTERVAL * 2,
            || {
                calls.set(calls.get() + 1);
                Ok(true)
            },
            || clock.get(),
            |duration| clock.set(clock.get() + duration),
        )
        .unwrap_err();
        assert!(error.to_string().contains("deadline expired"));
        assert_eq!(calls.get(), 2);
        assert!(
            wait_until_idle_with(
                start,
                || panic!("expired request must not inspect sink"),
                || start,
                |_| panic!("expired request must not pause"),
            )
            .is_err()
        );
    }

    #[test]
    fn backend_errors_and_status_checks_finishing_late_cannot_submit() {
        let start = Instant::now();
        let error = wait_until_idle_with(
            start + Duration::from_secs(1),
            || bail!("connection refused"),
            || start,
            |_| panic!("transport error must not be retried or treated as idle"),
        )
        .unwrap_err();
        assert!(error.to_string().contains("connection refused"));

        let clock = Cell::new(start);
        assert!(
            wait_until_idle_with(
                start + Duration::from_secs(1),
                || {
                    clock.set(start + Duration::from_secs(2));
                    Ok(false)
                },
                || clock.get(),
                |_| panic!("late idle response must not be accepted"),
            )
            .is_err()
        );
    }

    #[test]
    fn status_requires_bounded_unambiguous_framing_and_a_boolean() {
        let body = br#"{"listeners":0,"capacity":4,"clip_id":"1","playing":false}"#;
        let header = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        assert_eq!(status_body_length(header.as_bytes()).unwrap(), body.len());
        assert!(!status_playing(body).unwrap());
        assert!(status_playing(br#"{"playing":true}"#).unwrap());
        for header in [
            "HTTP/1.1 503 Busy\r\nContent-Length: 0\r\n\r\n".to_owned(),
            "HTTP/1.1 200 OK\r\n\r\n".to_owned(),
            "HTTP/1.1 200 OK\r\nContent-Length: 1\r\nContent-Length: 1\r\n\r\n".to_owned(),
            "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n".to_owned(),
            format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n",
                STATUS_BODY_LIMIT + 1
            ),
            format!(
                "HTTP/1.1 200 OK\r\nX-Pad: {}\r\nContent-Length: 0\r\n\r\n",
                "x".repeat(STATUS_HEADER_LIMIT)
            ),
        ] {
            assert!(status_body_length(header.as_bytes()).is_err());
        }
        for body in [
            &b"{}"[..],
            &b"{\"playing\":\"false\"}"[..],
            &b"not JSON"[..],
        ] {
            assert!(status_playing(body).is_err());
        }
    }
}

#[cfg(not(target_os = "trueos"))]
pub fn write_wav(path: &str, pcm: &[f32]) -> Result<()> {
    use std::io::Write;
    if pcm.is_empty() || pcm.iter().any(|s| !s.is_finite()) {
        bail!("invalid PCM")
    }
    let data_bytes = u32::try_from(
        pcm.len()
            .checked_mul(2)
            .ok_or_else(|| anyhow::anyhow!("WAV too large"))?,
    )?;
    let mut file = std::fs::File::create(path)?;
    file.write_all(b"RIFF")?;
    file.write_all(&(36 + data_bytes).to_le_bytes())?;
    file.write_all(b"WAVEfmt \x10\0\0\0\x01\0\x01\0")?;
    file.write_all(&24_000u32.to_le_bytes())?;
    file.write_all(&48_000u32.to_le_bytes())?;
    file.write_all(&2u16.to_le_bytes())?;
    file.write_all(&16u16.to_le_bytes())?;
    file.write_all(b"data")?;
    file.write_all(&data_bytes.to_le_bytes())?;
    for &sample in pcm {
        file.write_all(&((sample.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes())?;
    }
    Ok(())
}
