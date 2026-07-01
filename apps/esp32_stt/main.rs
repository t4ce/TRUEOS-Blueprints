// trueos-blueprint: features=["tokio-net-probe"]

use core::net::{Ipv4Addr, SocketAddr};
use std::{env, fs, path::Path};

use oxiwhisper::{InferenceBuffer, TranscribeOptions, WhisperModel};
use trueos::{
    logl::{self, level},
    platform::format,
    t,
};

const RTP_PORT: u16 = 5004;
const RTP_PAYLOAD_TYPE: u8 = 96;
const RTP_SAMPLE_RATE: u32 = 48_000;
const RTP_CHANNELS: usize = 2;
const FLUSH_SECONDS: u32 = 24;
const FLUSH_SAMPLES: usize = RTP_SAMPLE_RATE as usize * FLUSH_SECONDS as usize;
const UDP_BUFFER_BYTES: usize = 2048;
const TELEMETRY_PACKETS: u64 = 200;
const WHISPER_SAMPLE_RATE: usize = 16_000;
const MODEL_RETRY_WINDOWS: u32 = 3;
const MIN_TRANSCRIBE_RMS: u32 = 80;
const MIN_TRANSCRIBE_PEAK: i16 = 1_200;
const WHISPER_TARGET_RMS: f32 = 0.08;
const WHISPER_MAX_GAIN: f32 = 16.0;

#[derive(Clone, Copy, Debug)]
struct RtpHeader {
    sequence: u16,
    timestamp: u32,
    ssrc: u32,
    payload_offset: usize,
    payload_type: u8,
}

#[derive(Clone, Copy, Debug, Default)]
struct AudioStats {
    packets: u64,
    bad_packets: u64,
    lost_packets: u64,
    duplicate_or_reordered: u64,
    audio_frames: u64,
    peak: i16,
    sum_squares: u64,
    last_sequence: Option<u16>,
    last_timestamp: Option<u32>,
    last_ssrc: Option<u32>,
}

impl AudioStats {
    fn observe_packet(&mut self, header: RtpHeader, frames: usize) {
        self.packets = self.packets.saturating_add(1);
        self.audio_frames = self.audio_frames.saturating_add(frames as u64);

        if let Some(previous) = self.last_sequence {
            let expected = previous.wrapping_add(1);
            if header.sequence != expected {
                let delta = header.sequence.wrapping_sub(expected);
                if delta <= 512 {
                    self.lost_packets = self.lost_packets.saturating_add(delta as u64);
                } else {
                    self.duplicate_or_reordered = self.duplicate_or_reordered.saturating_add(1);
                }
            }
        }

        self.last_sequence = Some(header.sequence);
        self.last_timestamp = Some(header.timestamp);
        self.last_ssrc = Some(header.ssrc);
    }

    fn observe_sample(&mut self, sample: i16) {
        let magnitude = sample.saturating_abs();
        if magnitude > self.peak {
            self.peak = magnitude;
        }
        let value = sample as i64;
        self.sum_squares = self
            .sum_squares
            .saturating_add(value.saturating_mul(value) as u64);
    }

    fn rms_dbfs(&self) -> i32 {
        if self.audio_frames == 0 || self.sum_squares == 0 {
            return -120;
        }
        let mean_square = self.sum_squares / self.audio_frames;
        if mean_square == 0 {
            return -120;
        }

        let rms = int_sqrt(mean_square);
        if rms == 0 {
            return -120;
        }

        let percent = (rms.saturating_mul(100)) / 32768;
        match percent {
            0 => -120,
            1 => -40,
            2..=3 => -34,
            4..=6 => -28,
            7..=12 => -22,
            13..=24 => -16,
            25..=49 => -10,
            50..=79 => -6,
            80..=100 => -2,
            _ => 0,
        }
    }
}

struct AudioWindow {
    samples: Vec<i16>,
}

impl AudioWindow {
    fn new() -> Self {
        Self {
            samples: Vec::with_capacity(FLUSH_SAMPLES + RTP_SAMPLE_RATE as usize),
        }
    }

    fn push_mono(&mut self, sample: i16) {
        self.samples.push(sample);
    }

    fn is_ready(&self) -> bool {
        self.samples.len() >= FLUSH_SAMPLES
    }

    fn clear(&mut self) {
        self.samples.clear();
    }
}

#[derive(Clone, Copy, Debug)]
struct WindowMetrics {
    peak: i16,
    rms: u32,
}

impl WindowMetrics {
    fn speech_like(self) -> bool {
        self.peak >= MIN_TRANSCRIBE_PEAK && self.rms >= MIN_TRANSCRIBE_RMS
    }
}

#[derive(Clone, Debug)]
struct Transcript {
    text: String,
    confidence_percent: u8,
}

trait Transcriber {
    fn transcribe_48k_mono_i16(&mut self, samples: &[i16]) -> Transcript;
}

struct PlaceholderTranscriber;

impl Transcriber for PlaceholderTranscriber {
    fn transcribe_48k_mono_i16(&mut self, samples: &[i16]) -> Transcript {
        let metrics = measure_window(samples);
        let active = metrics.speech_like();
        let text = if active {
            "[stt-adapter pending: speech-like audio received]"
        } else {
            "[silence]"
        };

        Transcript {
            text: text.into(),
            confidence_percent: if active { 1 } else { 100 },
        }
    }
}

enum ActiveTranscriber {
    Whisper(OxiWhisperTranscriber),
    Placeholder(PlaceholderState),
}

struct PlaceholderState {
    transcriber: PlaceholderTranscriber,
    windows_until_retry: u32,
}

impl ActiveTranscriber {
    fn new() -> Self {
        announce(format_args!(
            "esp32-stt: whisper lazy-load enabled retry_windows={}",
            MODEL_RETRY_WINDOWS
        ));
        Self::Placeholder(PlaceholderState {
            transcriber: PlaceholderTranscriber,
            windows_until_retry: 0,
        })
    }

    fn try_load_whisper() -> Option<OxiWhisperTranscriber> {
        let candidates = model_path_candidates();
        let mut last_error = String::new();

        for path in candidates {
            announce(format_args!(
                "esp32-stt: loading whisper model path={}",
                path
            ));
            match OxiWhisperTranscriber::load(path.as_str()) {
                Ok(transcriber) => {
                    announce(format_args!("esp32-stt: whisper model ready path={}", path));
                    return Some(transcriber);
                }
                Err(err) => {
                    last_error = err;
                }
            }
        }

        if !last_error.is_empty() {
            announce(format_args!(
                "esp32-stt: whisper still unavailable {}",
                last_error
            ));
        }
        None
    }
}

impl Transcriber for ActiveTranscriber {
    fn transcribe_48k_mono_i16(&mut self, samples: &[i16]) -> Transcript {
        match self {
            Self::Whisper(transcriber) => transcriber.transcribe_48k_mono_i16(samples),
            Self::Placeholder(state) => {
                if state.windows_until_retry == 0 {
                    state.windows_until_retry = MODEL_RETRY_WINDOWS;
                    if let Some(transcriber) = Self::try_load_whisper() {
                        *self = Self::Whisper(transcriber);
                        return Transcript {
                            text: "[whisper ready; dropped pre-load audio]".into(),
                            confidence_percent: 100,
                        };
                    }
                } else {
                    state.windows_until_retry = state.windows_until_retry.saturating_sub(1);
                }
                state.transcriber.transcribe_48k_mono_i16(samples)
            }
        }
    }
}

struct OxiWhisperTranscriber {
    model: WhisperModel,
    buffer: InferenceBuffer,
    options: TranscribeOptions<'static>,
    downsampled: Vec<f32>,
}

impl OxiWhisperTranscriber {
    fn load(path: &str) -> Result<Self, String> {
        match fs::metadata(path) {
            Ok(metadata) => {
                announce(format_args!(
                    "esp32-stt: whisper model file path={} bytes={} readonly={}",
                    path,
                    metadata.len(),
                    metadata.permissions().readonly()
                ));
            }
            Err(err) => {
                return Err(format!("path={} metadata_err={}", path, err));
            }
        }

        announce(format_args!("esp32-stt: whisper load begin path={}", path));
        let model = WhisperModel::from_file(Path::new(path))
            .map_err(|err| format!("path={} err={}", path, err))?;
        announce(format_args!("esp32-stt: whisper load parsed path={}", path));
        let info = model.info();
        announce(format_args!(
            "esp32-stt: whisper info vocab={} audio_layers={} text_layers={} d_model={}",
            info.n_vocab, info.n_audio_layers, info.n_text_layers, info.d_model
        ));

        announce(format_args!(
            "esp32-stt: whisper buffer create begin path={}",
            path
        ));
        let buffer = model.create_buffer();
        announce(format_args!(
            "esp32-stt: whisper buffer create done path={}",
            path
        ));
        Ok(Self {
            model,
            buffer,
            options: TranscribeOptions {
                language: Some("en"),
                no_repeat_ngram_size: 3,
                ..TranscribeOptions::default()
            },
            downsampled: Vec::with_capacity(FLUSH_SECONDS as usize * WHISPER_SAMPLE_RATE),
        })
    }
}

impl Transcriber for OxiWhisperTranscriber {
    fn transcribe_48k_mono_i16(&mut self, samples: &[i16]) -> Transcript {
        let metrics = measure_window(samples);
        if !metrics.speech_like() {
            return Transcript {
                text: format!("[weak-audio rms={} peak={}]", metrics.rms, metrics.peak),
                confidence_percent: 100,
            };
        }

        downsample_48k_to_16k_f32(samples, &mut self.downsampled);
        if self.downsampled.is_empty() {
            return Transcript {
                text: "[silence]".into(),
                confidence_percent: 100,
            };
        }
        let gain = normalize_whisper_audio(&mut self.downsampled);
        announce(format_args!(
            "esp32-stt: whisper audio rms={} peak={} gain_x100={}",
            metrics.rms,
            metrics.peak,
            (gain * 100.0) as u32
        ));

        match self.model.transcribe_with_buffer(
            self.downsampled.as_slice(),
            &self.options,
            &mut self.buffer,
        ) {
            Ok(text) => {
                let text = normalize_transcript(text.as_str());
                let silent = text.is_empty();
                let noisy = transcript_looks_repetitive(text.as_str());
                Transcript {
                    text: if silent {
                        "[silence]".into()
                    } else if noisy {
                        format!(
                            "[weak-audio rms={} peak={} repeated-output]",
                            metrics.rms, metrics.peak
                        )
                    } else {
                        text
                    },
                    confidence_percent: if silent || noisy { 100 } else { 80 },
                }
            }
            Err(err) => Transcript {
                text: format!("[stt error: {}]", err),
                confidence_percent: 0,
            },
        }
    }
}

fn measure_window(samples: &[i16]) -> WindowMetrics {
    if samples.is_empty() {
        return WindowMetrics { peak: 0, rms: 0 };
    }

    let mut peak = 0i16;
    let mut sum_squares = 0u64;
    for &sample in samples {
        let magnitude = sample.saturating_abs();
        if magnitude > peak {
            peak = magnitude;
        }
        let value = sample as i64;
        sum_squares = sum_squares.saturating_add(value.saturating_mul(value) as u64);
    }

    let mean_square = sum_squares / samples.len() as u64;
    WindowMetrics {
        peak,
        rms: int_sqrt(mean_square) as u32,
    }
}

fn model_path_candidates() -> Vec<String> {
    let mut paths = Vec::new();
    push_env_path(&mut paths, "ESP32_STT_MODEL");

    if let Ok(root) = env::var("TRUEOS_APP_FS_ROOT") {
        push_joined_path(&mut paths, root.as_str(), "models/ggml-tiny.bin");
        push_joined_path(&mut paths, root.as_str(), "ggml-tiny.bin");
    }

    if let Ok(common) = env::var("TRUEOS_APP_COMMON").or_else(|_| env::var("TRUEOS_APP_FS_COMMON"))
    {
        push_joined_path(&mut paths, common.as_str(), "models/ggml-tiny.bin");
        push_joined_path(&mut paths, common.as_str(), "esp32_stt/ggml-tiny.bin");
    }

    paths.push("models/ggml-tiny.bin".into());
    paths.push("ggml-tiny.bin".into());
    paths.push("/common/models/ggml-tiny.bin".into());
    paths.push("/common/esp32_stt/ggml-tiny.bin".into());
    dedup_paths(paths)
}

fn push_env_path(paths: &mut Vec<String>, name: &str) {
    if let Ok(value) = env::var(name) {
        if !value.is_empty() {
            paths.push(value);
        }
    }
}

fn push_joined_path(paths: &mut Vec<String>, root: &str, rel: &str) {
    if root.is_empty() {
        return;
    }
    let sep = if root.ends_with('/') { "" } else { "/" };
    paths.push(format!("{}{}{}", root, sep, rel));
}

fn dedup_paths(paths: Vec<String>) -> Vec<String> {
    let mut unique = Vec::new();
    for path in paths {
        if !unique.iter().any(|seen| seen == &path) {
            unique.push(path);
        }
    }
    unique
}

fn downsample_48k_to_16k_f32(input: &[i16], output: &mut Vec<f32>) {
    output.clear();
    output.reserve(input.len() / 3);

    for chunk in input.chunks_exact(3) {
        let avg = (chunk[0] as f32 + chunk[1] as f32 + chunk[2] as f32) / (3.0 * 32768.0);
        output.push(avg.clamp(-1.0, 1.0));
    }
}

fn normalize_whisper_audio(samples: &mut [f32]) -> f32 {
    if samples.is_empty() {
        return 1.0;
    }

    let mut sum_squares = 0.0f32;
    let mut peak = 0.0f32;
    for &sample in samples.iter() {
        let abs = sample.abs();
        if abs > peak {
            peak = abs;
        }
        sum_squares += sample * sample;
    }

    if peak <= 0.00001 || sum_squares <= 0.0 {
        return 1.0;
    }

    let rms = (sum_squares / samples.len() as f32).sqrt();
    if rms <= 0.00001 {
        return 1.0;
    }

    let rms_gain = (WHISPER_TARGET_RMS / rms).min(WHISPER_MAX_GAIN);
    let peak_gain = 0.95 / peak;
    let gain = rms_gain.min(peak_gain).max(1.0);
    for sample in samples.iter_mut() {
        *sample = (*sample * gain).clamp(-1.0, 1.0);
    }
    gain
}

fn normalize_transcript(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn transcript_looks_repetitive(text: &str) -> bool {
    if text.len() < 48 {
        return false;
    }

    let tokens = text
        .split(|ch: char| !ch.is_alphanumeric())
        .filter(|part| !part.is_empty())
        .take(16)
        .collect::<Vec<_>>();
    if tokens.len() >= 6 && tokens.iter().all(|token| *token == tokens[0]) {
        return true;
    }

    let mut punctuation = 0usize;
    let mut visible = 0usize;
    for ch in text.chars() {
        if ch.is_whitespace() {
            continue;
        }
        visible += 1;
        if !ch.is_alphanumeric() {
            punctuation += 1;
        }
    }

    visible >= 48 && punctuation.saturating_mul(100) / visible >= 65
}

fn parse_rtp(packet: &[u8]) -> Result<RtpHeader, &'static str> {
    if packet.len() < 12 {
        return Err("short");
    }
    if packet[0] >> 6 != 2 {
        return Err("version");
    }

    let cc = (packet[0] & 0x0f) as usize;
    let extension = packet[0] & 0x10 != 0;
    let payload_type = packet[1] & 0x7f;
    let sequence = u16::from_be_bytes([packet[2], packet[3]]);
    let timestamp = u32::from_be_bytes([packet[4], packet[5], packet[6], packet[7]]);
    let ssrc = u32::from_be_bytes([packet[8], packet[9], packet[10], packet[11]]);

    let mut offset = 12 + cc.saturating_mul(4);
    if packet.len() < offset {
        return Err("csrc");
    }

    if extension {
        if packet.len() < offset + 4 {
            return Err("extension");
        }
        let extension_words = u16::from_be_bytes([packet[offset + 2], packet[offset + 3]]) as usize;
        offset += 4 + extension_words.saturating_mul(4);
        if packet.len() < offset {
            return Err("extension-len");
        }
    }

    Ok(RtpHeader {
        sequence,
        timestamp,
        ssrc,
        payload_offset: offset,
        payload_type,
    })
}

fn decode_l16_stereo_to_mono(
    payload: &[u8],
    window: &mut AudioWindow,
    stats: &mut AudioStats,
) -> usize {
    let frame_bytes = RTP_CHANNELS * 2;
    let frames = payload.len() / frame_bytes;
    for frame in 0..frames {
        let base = frame * frame_bytes;
        let left = i16::from_be_bytes([payload[base], payload[base + 1]]) as i32;
        let right = i16::from_be_bytes([payload[base + 2], payload[base + 3]]) as i32;
        let mono = if left.abs() >= right.abs() {
            left as i16
        } else {
            right as i16
        };
        stats.observe_sample(mono);
        window.push_mono(mono);
    }
    frames
}

async fn run_stt_server() -> Result<(), &'static str> {
    let bind_addr = SocketAddr::from((Ipv4Addr::UNSPECIFIED, RTP_PORT));
    let socket = t::net::UdpSocket::bind(bind_addr)
        .await
        .map_err(|_| "udp.bind")?;

    let local = socket.local_addr().map_err(|_| "udp.local_addr")?;
    announce(format_args!(
        "esp32-stt: listening udp={} payload={} audio=L16/{}/{} flush={}s",
        local, RTP_PAYLOAD_TYPE, RTP_SAMPLE_RATE, RTP_CHANNELS, FLUSH_SECONDS
    ));

    let mut buf = [0u8; UDP_BUFFER_BYTES];
    let mut window = AudioWindow::new();
    let mut stats = AudioStats::default();
    let mut transcriber = ActiveTranscriber::new();

    loop {
        let (len, from) = socket
            .recv_from(&mut buf)
            .await
            .map_err(|_| "udp.recv_from")?;
        let packet = &buf[..len];
        let header = match parse_rtp(packet) {
            Ok(header) => header,
            Err(reason) => {
                stats.bad_packets = stats.bad_packets.saturating_add(1);
                if stats.bad_packets == 1 || stats.bad_packets % 50 == 0 {
                    announce(format_args!(
                        "esp32-stt: bad RTP packet reason={} len={} from={}",
                        reason, len, from
                    ));
                }
                continue;
            }
        };

        if header.payload_type != RTP_PAYLOAD_TYPE {
            stats.bad_packets = stats.bad_packets.saturating_add(1);
            continue;
        }

        let payload = &packet[header.payload_offset..];
        let frames = decode_l16_stereo_to_mono(payload, &mut window, &mut stats);
        stats.observe_packet(header, frames);

        if stats.packets == 1 || stats.packets % TELEMETRY_PACKETS == 0 {
            announce(format_args!(
                "esp32-stt: rx packets={} bad={} lost={} reorder={} frames={} rms~{}dBFS peak={} from={} ssrc={:08x}",
                stats.packets,
                stats.bad_packets,
                stats.lost_packets,
                stats.duplicate_or_reordered,
                stats.audio_frames,
                stats.rms_dbfs(),
                stats.peak,
                from,
                stats.last_ssrc.unwrap_or(0)
            ));
        }

        if window.is_ready() {
            let transcript = transcriber.transcribe_48k_mono_i16(window.samples.as_slice());
            announce(format_args!(
                "esp32-stt: transcript window={}ms confidence={} text={}",
                (window.samples.len() as u64 * 1000) / RTP_SAMPLE_RATE as u64,
                transcript.confidence_percent,
                transcript.text
            ));
            window.clear();
        }
    }
}

fn announce(args: core::fmt::Arguments<'_>) {
    let line = format!("{}", args);
    logl::log(level::INFO, line.as_str());
}

fn int_sqrt(value: u64) -> u64 {
    if value <= 1 {
        return value;
    }

    let mut x0 = value / 2;
    let mut x1 = (x0 + value / x0) / 2;
    while x1 < x0 {
        x0 = x1;
        x1 = (x0 + value / x0) / 2;
    }
    x0
}

fn main() {
    logl::log(level::INFO, "esp32-stt: blueprint start");

    let runtime = match t::runtime::current_thread_net().build() {
        Ok(runtime) => runtime,
        Err(err) => {
            announce(format_args!("esp32-stt: runtime build failed {}", err));
            return;
        }
    };

    runtime.block_on(async {
        if let Err(stage) = run_stt_server().await {
            announce(format_args!("esp32-stt: stopped stage={}", stage));
        }
    });
}
