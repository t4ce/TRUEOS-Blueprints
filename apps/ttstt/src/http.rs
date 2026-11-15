//! Deliberately small loopback protocol: GET /health, POST /speech.
//! One synchronous owner serializes inference and playback with backpressure.
use std::io::{self, BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::time::Duration;

use crate::playback::Playback;
use crate::ttstt_service::{Engine, MAX_TEXT_BYTES, TtsRequest};

const MAX_HEADER_BYTES: usize = 8192;
const MAX_BODY_BYTES: usize = MAX_TEXT_BYTES * 6 + 1024;

#[derive(Debug)]
struct Request {
    method: String,
    path: String,
    body: Vec<u8>,
}

fn read_request(reader: &mut impl BufRead) -> io::Result<Request> {
    // Bound the complete header before allocating the body. Connections never
    // support pipelining.
    let mut header = Vec::new();
    while !header.ends_with(b"\r\n\r\n") {
        if header.len() >= MAX_HEADER_BYTES {
            return Err(io::Error::other("headers too large"));
        }
        let mut byte = [0];
        reader.read_exact(&mut byte)?;
        header.push(byte[0]);
    }
    let header = std::str::from_utf8(&header).map_err(io::Error::other)?;
    let mut lines = header.split("\r\n");
    let mut start = lines.next().unwrap_or_default().split_whitespace();
    let method = start.next().unwrap_or_default().to_owned();
    let path = start.next().unwrap_or_default().to_owned();
    if start.next() != Some("HTTP/1.1") || start.next().is_some() {
        return Err(io::Error::other("invalid request line"));
    }
    let mut content_length = None;
    for line in lines.filter(|s| !s.is_empty()) {
        let (name, value) = line
            .split_once(':')
            .ok_or_else(|| io::Error::other("invalid header"))?;
        if name.eq_ignore_ascii_case("transfer-encoding") {
            return Err(io::Error::other("transfer encoding unsupported"));
        }
        if name.eq_ignore_ascii_case("content-length") {
            if content_length.is_some() {
                return Err(io::Error::other("duplicate content length"));
            }
            content_length = Some(value.trim().parse::<usize>().map_err(io::Error::other)?);
        }
    }
    let len = content_length.unwrap_or(0);
    if len > MAX_BODY_BYTES {
        return Err(io::Error::other("body too large"));
    }
    let mut body = vec![0; len];
    reader.read_exact(&mut body)?;
    Ok(Request { method, path, body })
}

fn respond(stream: &mut TcpStream, status: &str, value: serde_json::Value) -> io::Result<()> {
    let body = serde_json::to_vec(&value)?;
    write!(
        stream,
        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    )?;
    stream.write_all(&body)
}

pub fn serve(root: &Path) -> Result<(), String> {
    // Bind before model allocation. A second launch cannot load another model
    // set or disturb the running service, even during its warmup.
    let listener =
        TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 9772)).map_err(|e| e.to_string())?;
    let mut engine = Engine::load(root)?;
    eprintln!("ttstt: ready protocol=ttstt-v1 tts=true stt=false listen=127.0.0.1:9772");
    for accepted in listener.incoming() {
        let mut stream = match accepted {
            Ok(s) => s,
            Err(e) => {
                eprintln!("ttstt: accept: {e}");
                continue;
            }
        };
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .map_err(|e| e.to_string())?;
        stream
            .set_write_timeout(Some(Duration::from_secs(5)))
            .map_err(|e| e.to_string())?;
        let request = read_request(&mut BufReader::new(&mut stream));
        let result = match request {
            Err(e) => respond(
                &mut stream,
                "400 Bad Request",
                serde_json::json!({"error":e.to_string()}),
            ),
            Ok(r) if r.method == "GET" && r.path == "/health" => respond(
                &mut stream,
                "200 OK",
                serde_json::json!({"protocol":"ttstt-v1", "ready":true, "tts":true, "stt":false}),
            ),
            Ok(r) if r.method == "POST" && r.path == "/speech" => {
                let result = serde_json::from_slice::<TtsRequest>(&r.body)
                    .map_err(|e| e.to_string())
                    .and_then(|request| {
                        request.validate()?;
                        let playback = Playback::open()?;
                        engine.synthesize(request, playback.sink())?;
                        playback.drain()
                    });
                match result {
                    Ok(()) => respond(&mut stream, "200 OK", serde_json::json!({"ok":true})),
                    Err(e) => respond(
                        &mut stream,
                        "422 Unprocessable Content",
                        serde_json::json!({"error":e}),
                    ),
                }
            }
            Ok(_) => respond(
                &mut stream,
                "404 Not Found",
                serde_json::json!({"error":"unknown endpoint"}),
            ),
        };
        if let Err(e) = result {
            eprintln!("ttstt: response: {e}");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn accepts_unicode_body_by_bytes() {
        let body = r#"{"text":"こんにちは"}"#;
        let wire = format!(
            "POST /speech HTTP/1.1\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        );
        let r = read_request(&mut io::Cursor::new(wire)).unwrap();
        assert_eq!(r.body, body.as_bytes());
    }
    #[test]
    fn rejects_ambiguous_framing_and_large_requests() {
        for wire in [
            "POST /speech HTTP/1.1\r\nContent-Length: 1\r\nContent-Length: 1\r\n\r\nx".into(),
            "POST /speech HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n".into(),
            format!(
                "POST /speech HTTP/1.1\r\nContent-Length: {}\r\n\r\n",
                MAX_BODY_BYTES + 1
            ),
            "a".repeat(MAX_HEADER_BYTES + 1),
            "POST /speech HTTP/1.1\r\nContent-Length: 10\r\n\r\nshort".into(),
        ] {
            assert!(read_request(&mut io::Cursor::new(wire)).is_err());
        }
    }
}
