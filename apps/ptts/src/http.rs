//! Warm loopback speech sidekick for Codex. One owner serializes speech requests.
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
const LILY_PORT: u16 = 9771;
const LILY_TIMEOUT: Duration = Duration::from_millis(500);

fn lily_label(text: &str, deadline: Instant) -> Option<String> {
    #[derive(Deserialize)]
    struct Classification {
        label: String,
    }

    let deadline = deadline.min(Instant::now() + LILY_TIMEOUT);
    let body = serde_json::to_vec(&serde_json::json!({"text": text})).ok()?;
    let mut request = format!(
        "POST /v1/classify HTTP/1.1\r\nHost: 127.0.0.1:{LILY_PORT}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len(),
    )
    .into_bytes();
    request.extend_from_slice(&body);
    let response = crate::loopback::exchange(LILY_PORT, &request, deadline).ok()?;
    let result: Classification = serde_json::from_slice(&response).ok()?;
    (!result.label.is_empty() && result.label.len() <= 64).then_some(result.label)
}

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
    pub intro: Option<crate::cues::Intro>,
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
    pub samples: usize,
    pub temperature: f32,
    pub chunks: usize,
    pub first_pcm_ms: Option<u128>,
    pub generation_ms: u128,
}
pub enum SpeechAudio<'a> {
    /// Prepared, speakable text exists; start the intro before Lily/inference.
    Start,
    Pcm(&'a [f32]),
}
impl SpeechModel {
    pub fn load() -> Result<Self> {
        let started = Instant::now();
        crate::notice("ptts: loading CPU model and Azelma state");
        let engine = Engine::load(crate::model::load()?)?;
        crate::notice(format!(
            "ptts: model ready elapsed_ms={} loading tokenizer",
            started.elapsed().as_millis()
        ));
        let frontend = Frontend::from_bytes(bundle::tokenizer())?;
        crate::notice(format!(
            "ptts: tokenizer ready elapsed_ms={}",
            started.elapsed().as_millis()
        ));
        Ok(Self {
            engine,
            frontend,
            rules: ToneRules::embedded().map_err(anyhow::Error::msg)?,
        })
    }
    pub fn synthesize(
        &self,
        request: &SpeechRequest,
        deadline: Instant,
        on_audio: &mut dyn FnMut(SpeechAudio<'_>) -> Result<()>,
    ) -> Result<SpeechResult> {
        request.validate()?;
        let started = Instant::now();
        let chunks = self.frontend.prepare(&request.text)?;
        if !chunks.is_empty() {
            on_audio(SpeechAudio::Start)?;
        }
        let inferred_label = if request.label.is_none() && !chunks.is_empty() {
            lily_label(&request.text, deadline)
        } else {
            None
        };
        let label = request.label.as_deref().or(inferred_label.as_deref());
        let temperature = self.rules.temperature_for(label);
        if let Some(label) = inferred_label {
            crate::notice(format!(
                "ptts: Lily label={label:?} temperature={temperature}"
            ));
        }
        let mut total_samples = 0;
        let mut first_pcm_ms = None;
        for chunk in &chunks {
            self.engine.generate_with_limits(
                &chunk.tokens,
                temperature,
                chunk.frames_after_eos,
                deadline,
                &mut |samples| {
                    if total_samples + samples.len() > 24_000 * 120 {
                        bail!("speech exceeds the 120-second clip limit")
                    }
                    if first_pcm_ms.is_none() {
                        first_pcm_ms = Some(started.elapsed().as_millis());
                    }
                    on_audio(SpeechAudio::Pcm(samples))?;
                    total_samples += samples.len();
                    Ok(())
                },
            )?;
        }
        Ok(SpeechResult {
            samples: total_samples,
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
    #[cfg(target_os = "trueos")]
    let mut previous_playback = None;
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
                serde_json::json!({"protocol":"ptts-v1","ready":true,"tts":true,"stt":false,"language":"en","voice":"azelma","model":"english_2026-10","bundled":false,"turn_cues":true}),
            ),
            Ok(r) if r.method == "POST" && r.path == "/speech" => {
                job = job.saturating_add(1);
                let started = Instant::now();
                let deadline = started + REQUEST_TIMEOUT;
                let result = (|| -> Result<serde_json::Value> {
                    let request: SpeechRequest = serde_json::from_slice(&r.body)?;
                    request.validate()?;
                    crate::notice(format!(
                        "ptts: job={job} submitted bytes={} label={} intro={:?}",
                        request.text.len(),
                        request.label.as_deref().unwrap_or("auto"),
                        request.intro,
                    ));
                    #[cfg(target_os = "trueos")]
                    let (generated, first_stream_ms, stream_id) = {
                        let mut playback =
                            crate::playback::Stream::new(deadline, previous_playback);
                        let generated =
                            model.synthesize(&request, deadline, &mut |audio| match audio {
                                SpeechAudio::Start => {
                                    if let Some(intro) = request.intro {
                                        playback.push_cue(intro.pcm())?;
                                    }
                                    Ok(())
                                }
                                SpeechAudio::Pcm(samples) => playback.push(samples),
                            })?;
                        playback.finish()?;
                        if let Some(id) = playback.completed_stream() {
                            previous_playback = Some(id);
                        }
                        (
                            generated,
                            playback.first_write().map(|at| (at - started).as_millis()),
                            playback.completed_stream(),
                        )
                    };
                    #[cfg(not(target_os = "trueos"))]
                    let (generated, first_stream_ms, stream_id) = {
                        let mut playback = crate::playback::HostClip::default();
                        let generated =
                            model.synthesize(&request, deadline, &mut |audio| match audio {
                                SpeechAudio::Start => {
                                    if let Some(intro) = request.intro {
                                        playback.push_cue(intro.pcm());
                                    }
                                    Ok(())
                                }
                                SpeechAudio::Pcm(samples) => playback.push(samples),
                            })?;
                        playback.finish();
                        if !playback.bytes.is_empty() {
                            playback.write(&output_dir.join(format!("speech-{job:06}.wav")))?;
                        }
                        (generated, None::<u128>, None::<u64>)
                    };
                    let audio_ms = generated.samples / 24;
                    crate::notice(format!(
                        "ptts: job={job} done first_pcm_ms={} generation_ms={} audio_ms={audio_ms} chunks={} temperature={} total_ms={} first_stream_ms={}",
                        generated.first_pcm_ms.unwrap_or_default(),
                        generated.generation_ms,
                        generated.chunks,
                        generated.temperature,
                        started.elapsed().as_millis(),
                        first_stream_ms.unwrap_or_default()
                    ));
                    Ok(
                        serde_json::json!({"ok":true,"audio_ms":audio_ms,"temperature":generated.temperature,"chunks":generated.chunks,"first_pcm_ms":generated.first_pcm_ms,"generation_ms":generated.generation_ms,"first_stream_ms":first_stream_ms,"stream_id":stream_id}),
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
            Ok(r) if r.method == "POST" && r.path == "/cue" => {
                job = job.saturating_add(1);
                let result = (|| -> Result<serde_json::Value> {
                    let request: crate::cues::CueRequest = serde_json::from_slice(&r.body)?;
                    let crate::cues::Done::Done = request.cue;
                    #[cfg(target_os = "trueos")]
                    let stream_id = {
                        let mut playback = crate::playback::Stream::new(
                            Instant::now() + REQUEST_TIMEOUT,
                            previous_playback,
                        );
                        playback.push_cue(crate::cues::done_pcm())?;
                        playback.finish()?;
                        previous_playback = playback.completed_stream();
                        previous_playback
                    };
                    #[cfg(not(target_os = "trueos"))]
                    let stream_id = {
                        let mut playback = crate::playback::HostClip::default();
                        playback.push_cue(crate::cues::done_pcm());
                        playback.write(&output_dir.join(format!("cue-{job:06}-done.wav")))?;
                        None::<u64>
                    };
                    crate::notice(format!(
                        "ptts: job={job} cue=done queued after previous audio"
                    ));
                    Ok(serde_json::json!({"ok":true,"stream_id":stream_id}))
                })();
                match result {
                    Ok(value) => respond(&mut stream, "200 OK", value),
                    Err(e) => respond(
                        &mut stream,
                        "422 Unprocessable Content",
                        serde_json::json!({"error":format!("{e:#}")}),
                    ),
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
