//! Single-owner, cooperative TTS runner. No Whisper asset or STT startup.
use std::fs::File;
use std::io::{self, Read};
use std::path::Path;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

pub const TTS_PCM_CHANNELS: usize = 2;
pub const TTS_PCM_CHUNK_MAX_FRAMES: usize = 12_000;
pub const MAX_TEXT_BYTES: usize = 16 * 1024;
const MODEL_FILE_MAX_BYTES: u64 = 512 * 1024 * 1024;
const MODEL_SET_MAX_BYTES: usize = 768 * 1024 * 1024;
const SYNTHESIS_TIMEOUT: Duration = Duration::from_secs(180);

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TtsRequest {
    pub text: String,
    #[serde(default = "default_voice")]
    pub voice: String,
    #[serde(default = "default_speed")]
    pub speed: f32,
}
fn default_voice() -> String {
    "af_heart".into()
}
fn default_speed() -> f32 {
    1.0
}
impl TtsRequest {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.text.trim().is_empty() || self.text.len() > MAX_TEXT_BYTES {
            return Err("text must contain 1..16384 UTF-8 bytes");
        }
        if self.voice.is_empty() || self.voice.len() > 64 {
            return Err("invalid voice");
        }
        if !self.speed.is_finite() || !(0.5..=2.0).contains(&self.speed) {
            return Err("speed must be between 0.5 and 2.0");
        }
        Ok(())
    }
}

#[repr(C, align(64))]
#[derive(Clone, Copy, Debug)]
struct ModelImageLine([u8; 64]);

#[derive(Debug)]
pub struct ModelImage {
    path: String,
    lines: Vec<ModelImageLine>,
    len: usize,
}
impl ModelImage {
    fn load(root: &Path, name: &str) -> io::Result<Self> {
        let path = root.join(name);
        let mut file = File::open(&path)?;
        let len = file.metadata()?.len();
        if len == 0 || len > MODEL_FILE_MAX_BYTES {
            return Err(io::Error::other(format!(
                "invalid model size: {} ({len})",
                path.display()
            )));
        }
        let len = usize::try_from(len).map_err(io::Error::other)?;
        let count = len.div_ceil(64);
        let mut lines = Vec::new();
        lines.try_reserve_exact(count).map_err(io::Error::other)?;
        lines.resize(count, ModelImageLine([0; 64]));
        let mut image = Self {
            path: format!("models/kokoro/{name}"),
            lines,
            len,
        };
        // SAFETY: each aligned line contains exactly 64 initialized bytes;
        // len never exceeds their combined size, and the borrow is exclusive.
        let bytes =
            unsafe { std::slice::from_raw_parts_mut(image.lines.as_mut_ptr().cast::<u8>(), len) };
        file.read_exact(bytes)?;
        let mut extra = [0];
        if file.read(&mut extra)? != 0 {
            return Err(io::Error::other("model changed while loading"));
        }
        Ok(image)
    }
    pub fn path(&self) -> &str {
        &self.path
    }
    pub fn bytes(&self) -> &[u8] {
        // SAFETY: same aligned storage and bounds proof as load().
        unsafe { std::slice::from_raw_parts(self.lines.as_ptr().cast::<u8>(), self.len) }
    }
}

pub struct ModelSet {
    kokoro: ModelImage,
    voices: ModelImage,
    g2p: ModelImage,
    lexicon: ModelImage,
}
impl ModelSet {
    fn load(root: &Path) -> io::Result<Self> {
        // Preflight all assets before bulk reads. TTS never waits for Whisper.
        let names = [
            "kokoro.kkaot",
            "voices-v1.0.bin",
            "en.g2p",
            "misaki-us.klex",
        ];
        let mut total = 0usize;
        for name in names {
            let len = std::fs::metadata(root.join(name))?.len();
            if len == 0 || len > MODEL_FILE_MAX_BYTES {
                return Err(io::Error::other("invalid model size"));
            }
            total = total
                .checked_add(usize::try_from(len).map_err(io::Error::other)?)
                .ok_or_else(|| io::Error::other("model set size overflow"))?;
        }
        if total > MODEL_SET_MAX_BYTES {
            return Err(io::Error::other("model set too large"));
        }
        Ok(Self {
            kokoro: ModelImage::load(root, names[0])?,
            voices: ModelImage::load(root, names[1])?,
            g2p: ModelImage::load(root, names[2])?,
            lexicon: ModelImage::load(root, names[3])?,
        })
    }
    pub fn kokoro(&self) -> &ModelImage {
        &self.kokoro
    }
    pub fn kokoro_voices(&self) -> &ModelImage {
        &self.voices
    }
    pub fn kokoro_g2p(&self) -> Option<&ModelImage> {
        Some(&self.g2p)
    }
    pub fn kokoro_lexicon(&self) -> Option<&ModelImage> {
        Some(&self.lexicon)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    Warmup,
    TextToSpeech,
}
#[derive(Clone, Copy)]
pub struct WorkerContext;
pub enum JobProgress {
    Pending,
    Complete,
    Failed(&'static str),
}
pub trait InferenceJob: Send {
    fn direction(&self) -> Direction;
    fn run_slice(&mut self, models: &'static ModelSet, worker: WorkerContext) -> JobProgress;
}

pub struct TtsAudioChunk {
    pub samples_i16_stereo_48k: Vec<i16>,
    pub model_chunk_index: u32,
    pub model_chunk_phonemes: u16,
    pub end_of_model_chunk: bool,
}
#[allow(dead_code)]
pub enum TtsOutputError {
    WouldBlock(TtsAudioChunk),
    Closed(TtsAudioChunk),
    Invalid { reason: &'static str },
}
pub type AudioSink = Box<dyn FnMut(&[i16]) -> Result<(), &'static str> + Send>;
struct OutputState {
    sink: AudioSink,
    finished: bool,
    error: Option<&'static str>,
}
#[derive(Clone)]
pub struct TtsOutput(Arc<Mutex<OutputState>>);
impl TtsOutput {
    fn new(sink: AudioSink) -> Self {
        Self(Arc::new(Mutex::new(OutputState {
            sink,
            finished: false,
            error: None,
        })))
    }
    pub fn cancelled(&self) -> bool {
        self.0.lock().unwrap().error.is_some()
    }
    pub fn try_push(&self, chunk: TtsAudioChunk) -> Result<(), TtsOutputError> {
        let mut state = self.0.lock().unwrap();
        if state.finished {
            return Err(TtsOutputError::Closed(chunk));
        }
        let count = chunk.samples_i16_stereo_48k.len();
        if count == 0
            || count % TTS_PCM_CHANNELS != 0
            || count / TTS_PCM_CHANNELS > TTS_PCM_CHUNK_MAX_FRAMES
        {
            return Err(TtsOutputError::Invalid {
                reason: "invalid-pcm-chunk",
            });
        }
        // The single owner applies backpressure synchronously. No audio queue
        // can grow while the playback endpoint is full.
        if let Err(reason) = (state.sink)(&chunk.samples_i16_stereo_48k) {
            state.error = Some(reason);
            state.finished = true;
            return Err(TtsOutputError::Closed(chunk));
        }
        Ok(())
    }
    pub fn finish_success(&self) -> bool {
        let mut state = self.0.lock().unwrap();
        if state.finished {
            return false;
        }
        state.finished = true;
        true
    }
    pub fn finish_error(&self, reason: &'static str) -> bool {
        let mut state = self.0.lock().unwrap();
        if state.finished {
            return false;
        }
        state.error = Some(reason);
        state.finished = true;
        true
    }
}
pub struct BackendTtsRequest {
    pub request: TtsRequest,
    pub output: TtsOutput,
}
// STT remains explicitly unavailable. No decoder, input device or model loads.
pub struct SttRequest;
pub trait SpeechBackend: Send + Sync {
    fn name(&self) -> &'static str;
    fn ready(&self) -> bool;
    fn tts_ready(&self) -> bool;
    fn stt_ready(&self) -> bool;
    fn warm_failure_reason(&self) -> Option<&'static str>;
    fn create_warm_job(&self) -> Result<Box<dyn InferenceJob>, &'static str>;
    fn create_tts_job(
        &self,
        request: BackendTtsRequest,
    ) -> Result<Box<dyn InferenceJob>, &'static str>;
    fn create_stt_job(&self, request: SttRequest) -> Result<Box<dyn InferenceJob>, &'static str>;
}
static BACKEND: OnceLock<&'static dyn SpeechBackend> = OnceLock::new();
pub fn install_speech_backend(backend: &'static dyn SpeechBackend) -> bool {
    BACKEND.set(backend).is_ok()
}

// Parsed views borrow model bytes for this Blueprint's lifetime. Initialization
// is once-only so failed retries cannot leak additional resident model sets.
static INITIALIZED: OnceLock<Result<&'static ModelSet, String>> = OnceLock::new();
pub struct Engine {
    models: &'static ModelSet,
}
impl Engine {
    pub fn load(root: &Path) -> Result<Self, String> {
        let models = INITIALIZED
            .get_or_init(|| {
                let models = ModelSet::load(root).map_err(|e| e.to_string())?;
                let models = Box::leak(Box::new(models));
                crate::ttstt_kokoro::install();
                let backend = *BACKEND.get().ok_or("speech-backend-missing")?;
                let mut warm = backend.create_warm_job()?;
                run_job(warm.as_mut(), models)?;
                if !backend.tts_ready() {
                    return Err(backend
                        .warm_failure_reason()
                        .unwrap_or("kokoro-not-ready")
                        .into());
                }
                Ok(models as &'static ModelSet)
            })
            .as_ref()
            .map_err(Clone::clone)?;
        Ok(Self { models })
    }
    pub fn synthesize(&mut self, request: TtsRequest, sink: AudioSink) -> Result<(), String> {
        request.validate()?;
        let output = TtsOutput::new(sink);
        let mut job = BACKEND
            .get()
            .ok_or("speech-backend-missing")?
            .create_tts_job(BackendTtsRequest {
                request,
                output: output.clone(),
            })?;
        let result = run_job(job.as_mut(), self.models);
        let state = output.0.lock().unwrap();
        if let Some(reason) = state.error {
            return Err(reason.into());
        }
        result?;
        if !state.finished {
            return Err("speech ended without completion".into());
        }
        Ok(())
    }
}
fn run_job(job: &mut dyn InferenceJob, models: &'static ModelSet) -> Result<(), String> {
    let deadline = Instant::now() + SYNTHESIS_TIMEOUT;
    loop {
        if Instant::now() >= deadline {
            return Err("speech-inference-timeout".into());
        }
        match job.run_slice(models, WorkerContext) {
            JobProgress::Pending => std::thread::yield_now(),
            JobProgress::Complete => return Ok(()),
            JobProgress::Failed(reason) => return Err(reason.into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn validates_requests_without_stt_assets() {
        let mut r: TtsRequest = serde_json::from_str(r#"{"text":"Hello"}"#).unwrap();
        assert_eq!(r.voice, "af_heart");
        assert!(r.validate().is_ok());
        r.speed = f32::NAN;
        assert!(r.validate().is_err());
        r.speed = 1.0;
        r.text = "a".repeat(MAX_TEXT_BYTES + 1);
        assert!(r.validate().is_err());
    }
    #[test]
    fn output_propagates_playback_failure_and_rejects_future_chunks() {
        let out = TtsOutput::new(Box::new(|_| Err("no-device")));
        let chunk = || TtsAudioChunk {
            samples_i16_stereo_48k: vec![0; 4],
            model_chunk_index: 0,
            model_chunk_phonemes: 1,
            end_of_model_chunk: true,
        };
        assert!(matches!(
            out.try_push(chunk()),
            Err(TtsOutputError::Closed(_))
        ));
        assert!(out.cancelled());
        assert!(!out.finish_success());
        assert!(matches!(
            out.try_push(chunk()),
            Err(TtsOutputError::Closed(_))
        ));
    }
    #[test]
    fn model_image_is_aligned_and_preserves_exact_bytes() {
        let dir = std::env::temp_dir().join(format!("ttstt-image-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("test"), [7u8; 65]).unwrap();
        let image = ModelImage::load(&dir, "test").unwrap();
        assert_eq!(image.bytes(), &[7u8; 65]);
        assert_eq!(image.bytes().as_ptr() as usize % 64, 0);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
