//! Warm loopback speech sidekick for Codex. One owner serializes complete clips.
use crate::{bundle, engine::Engine, frontend::Frontend, tone::ToneRules};
use anyhow::{Result, bail};
use serde::Deserialize;
use std::io::{self, BufRead, BufReader, Write};
use std::net::{Ipv4Addr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::time::{Duration, Instant};
pub const PORT: u16 = 9773;
pub const MAX_TEXT_BYTES: usize = 16 * 1024;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(180);
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

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SpeechRequest {
    pub text: String,
    pub label: Option<String>,
    pub voice: Option<String>,
    #[serde(default = "normal_speed")]
    pub speed: f32,
}
fn normal_speed() -> f32 {
    1.0
}
impl SpeechRequest {
    fn validate(&self) -> Result<()> {
        if self.text.trim().is_empty() || self.text.len() > MAX_TEXT_BYTES {
            bail!("text must contain 1..16384 UTF-8 bytes")
        }
        if self.voice.as_deref().is_some_and(|v| v != "azelma") {
            bail!("this bundle supports only English Azelma")
        }
        if self.label.as_ref().is_some_and(|v| v.len() > 64) {
            bail!("label exceeds 64 bytes")
        }
        if self.speed != 1.0 {
            bail!("ptts currently supports only speed 1.0")
        }
        Ok(())
    }
}

pub struct SpeechModel {
    engine: Engine,
    frontend: Frontend,
    rules: ToneRules,
}
pub struct SpeechResult {
    pub pcm: Vec<f32>,
    pub temperature: f32,
    pub chunks: usize,
    pub first_pcm_ms: Option<u128>,
    pub generation_ms: u128,
}
impl SpeechModel {
    pub fn load() -> Result<Self> {
        Ok(Self {
            engine: Engine::load()?,
            frontend: Frontend::from_bytes(bundle::tokenizer())?,
            rules: ToneRules::embedded().map_err(anyhow::Error::msg)?,
        })
    }
    pub fn synthesize(&self, request: &SpeechRequest, deadline: Instant) -> Result<SpeechResult> {
        request.validate()?;
        let temperature = self.rules.temperature_for(request.label.as_deref());
        let started = Instant::now();
        let chunks = self.frontend.prepare(&request.text)?;
        let mut pcm = Vec::new();
        let mut first_pcm_ms = None;
        for chunk in &chunks {
            self.engine.generate_with_limits(
                &chunk.tokens,
                temperature,
                chunk.frames_after_eos,
                deadline,
                &mut |samples| {
                    if pcm.len() + samples.len() > 24_000 * 120 {
                        bail!("speech exceeds the 120-second clip limit")
                    }
                    if first_pcm_ms.is_none() {
                        first_pcm_ms = Some(started.elapsed().as_millis());
                    }
                    pcm.try_reserve(samples.len())
                        .map_err(|_| anyhow::anyhow!("speech PCM allocation failed"))?;
                    pcm.extend_from_slice(samples);
                    Ok(())
                },
            )?;
        }
        Ok(SpeechResult {
            pcm,
            temperature,
            chunks: chunks.len(),
            first_pcm_ms,
            generation_ms: started.elapsed().as_millis(),
        })
    }
}

pub fn serve(port: u16, host_wav_dir: Option<PathBuf>) -> Result<()> {
    // A duplicate launch fails before allocating a second model or tokenizer.
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, port))?;
    crate::notice(format!(
        "ptts: loading service endpoint=127.0.0.1:{port} backend=xn-cpu threads=1"
    ));
    let startup = Instant::now();
    let model = SpeechModel::load()?;
    #[cfg(not(target_os = "trueos"))]
    let output_dir = {
        let dir =
            host_wav_dir.ok_or_else(|| anyhow::anyhow!("host --serve requires --wav-dir DIR"))?;
        std::fs::create_dir_all(&dir)?;
        dir
    };
    #[cfg(target_os = "trueos")]
    let _ = host_wav_dir;
    crate::notice(format!(
        "ptts: ready protocol=ptts-v1 voice=azelma language=en load_ms={} listen=127.0.0.1:{port}",
        startup.elapsed().as_millis()
    ));
    let mut job = 0u64;
    for accepted in listener.incoming() {
        let mut stream = match accepted {
            Ok(s) => s,
            Err(e) => {
                crate::notice(format!("ptts: accept failed: {e}"));
                continue;
            }
        };
        if let Err(e) = stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .and_then(|_| stream.set_write_timeout(Some(Duration::from_secs(5))))
        {
            crate::notice(format!("ptts: connection setup failed: {e}"));
            continue;
        }
        let parsed = read_request(&mut BufReader::new(&mut stream));
        let response = match parsed {
            Err(e) => respond(
                &mut stream,
                "400 Bad Request",
                serde_json::json!({"error":e.to_string()}),
            ),
            Ok(r) if r.method == "GET" && r.path == "/health" => respond(
                &mut stream,
                "200 OK",
                serde_json::json!({"protocol":"ptts-v1","ready":true,"tts":true,"stt":false,"language":"en","voice":"azelma","model":"english_2026-10","bundled":true}),
            ),
            Ok(r) if r.method == "POST" && r.path == "/speech" => {
                job = job.saturating_add(1);
                let started = Instant::now();
                let deadline = started + REQUEST_TIMEOUT;
                let result = (|| -> Result<serde_json::Value> {
                    let request: SpeechRequest = serde_json::from_slice(&r.body)?;
                    request.validate()?;
                    crate::notice(format!(
                        "ptts: job={job} submitted bytes={} label={}",
                        request.text.len(),
                        request.label.as_deref().unwrap_or("Static")
                    ));
                    let generated = model.synthesize(&request, deadline)?;
                    if !generated.pcm.is_empty() {
                        #[cfg(target_os = "trueos")]
                        crate::playback::submit_with_deadline(&generated.pcm, deadline)?;
                        #[cfg(not(target_os = "trueos"))]
                        crate::playback::write_wav(
                            output_dir
                                .join(format!("speech-{job:06}.wav"))
                                .to_str()
                                .ok_or_else(|| anyhow::anyhow!("invalid host output path"))?,
                            &generated.pcm,
                        )?;
                    }
                    crate::notice(format!(
                        "ptts: job={job} done first_pcm_ms={} generation_ms={} audio_ms={} chunks={} temperature={} total_ms={}",
                        generated.first_pcm_ms.unwrap_or_default(),
                        generated.generation_ms,
                        generated.pcm.len() / 24,
                        generated.chunks,
                        generated.temperature,
                        started.elapsed().as_millis()
                    ));
                    Ok(
                        serde_json::json!({"ok":true,"audio_ms":generated.pcm.len()/24,"temperature":generated.temperature,"chunks":generated.chunks,"first_pcm_ms":generated.first_pcm_ms,"generation_ms":generated.generation_ms}),
                    )
                })();
                match result {
                    Ok(value) => respond(&mut stream, "200 OK", value),
                    Err(e) => {
                        crate::notice(format!("ptts: job={job} failed: {e:#}"));
                        respond(
                            &mut stream,
                            "422 Unprocessable Content",
                            serde_json::json!({"error":format!("{e:#}")}),
                        )
                    }
                }
            }
            Ok(_) => respond(
                &mut stream,
                "404 Not Found",
                serde_json::json!({"error":"unknown endpoint"}),
            ),
        };
        if let Err(e) = response {
            crate::notice(format!("ptts: response failed: {e}"));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn validates_codex_request_and_optional_policy_label() {
        let request: SpeechRequest =
            serde_json::from_str(r#"{"text":"TrueOS is ready.","label":"Happy"}"#).unwrap();
        request.validate().unwrap();
        assert_eq!(request.speed, 1.0);
        for bad in [
            r#"{"text":""}"#,
            r#"{"text":"hello","voice":"af_heart"}"#,
            r#"{"text":"hello","speed":2.0}"#,
        ] {
            assert!(
                serde_json::from_str::<SpeechRequest>(bad)
                    .unwrap()
                    .validate()
                    .is_err()
            );
        }
        assert!(
            serde_json::from_str::<SpeechRequest>(r#"{"text":"hello","unknown":true}"#).is_err()
        );
    }
    #[test]
    fn reads_unicode_and_rejects_ambiguous_or_truncated_framing() {
        let body = r#"{"text":"Grüße"}"#;
        let wire = format!(
            "POST /speech HTTP/1.1\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        );
        assert_eq!(
            read_request(&mut io::Cursor::new(wire)).unwrap().body,
            body.as_bytes()
        );
        for bad in [
            "POST /speech HTTP/1.1\r\nContent-Length: 1\r\nContent-Length: 1\r\n\r\nx".to_owned(),
            "POST /speech HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n".into(),
            format!(
                "POST /speech HTTP/1.1\r\nContent-Length: {}\r\n\r\n",
                MAX_BODY_BYTES + 1
            ),
            "POST /speech HTTP/1.1\r\nContent-Length: 10\r\n\r\nx".into(),
        ] {
            assert!(read_request(&mut io::Cursor::new(bad)).is_err());
        }
    }
}
