//! Single-owner, cooperative TTS runner. No Whisper asset or STT startup.
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

pub const TTS_PCM_CHANNELS: usize = 2;
pub const TTS_PCM_CHUNK_MAX_FRAMES: usize = 12_000;
pub const MAX_TEXT_BYTES: usize = 16 * 1024;
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
    pub fn is_emoji_only(&self) -> bool {
        trueos_kokoro_g2p::is_emoji_only(&self.text)
    }

    pub fn validate(&self) -> Result<(), &'static str> {
        if self.text.trim().is_empty() || self.text.len() > MAX_TEXT_BYTES {
            return Err("text must contain 1..16384 UTF-8 bytes");
        }
        if self.voice != "af_heart" {
            return Err("this bundle supports only the Heart voice (af_heart)");
        }
        if !self.speed.is_finite() || !(0.5..=2.0).contains(&self.speed) {
            return Err("speed must be between 0.5 and 2.0");
        }
        Ok(())
    }
}

/// Borrow aligned static bundle data without opening TRUEOSFS or copying weights.
pub struct ModelImage {
    path: &'static str,
    bytes: &'static [u8],
}
impl ModelImage {
    pub fn path(&self) -> &str {
        self.path
    }
    pub fn bytes(&self) -> &'static [u8] {
        self.bytes
    }
}

pub struct ModelSet {
    kokoro: ModelImage,
    heart: ModelImage,
    g2p: ModelImage,
}
impl ModelSet {
    fn bundled() -> Self {
        Self {
            kokoro: ModelImage {
                path: "models/kokoro/kokoro.kkaot",
                bytes: crate::bundle::model(),
            },
            heart: ModelImage {
                path: "models/kokoro/af_heart.npy",
                bytes: crate::bundle::heart(),
            },
            g2p: ModelImage {
                path: "models/kokoro/en.g2p",
                bytes: crate::bundle::g2p(),
            },
        }
    }
    pub fn kokoro(&self) -> &ModelImage {
        &self.kokoro
    }
    pub fn kokoro_heart(&self) -> &ModelImage {
        &self.heart
    }
    pub fn kokoro_g2p(&self) -> &ModelImage {
        &self.g2p
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

    #[cfg(any(feature = "async-convolution", feature = "native-convolution"))]
    fn wait_ready(&mut self) -> core::pin::Pin<Box<dyn core::future::Future<Output = Result<(), &'static str>> + '_>> {
        Box::pin(async {
            #[cfg(feature = "async-convolution")]
            smol::future::yield_now().await;
            #[cfg(not(feature = "async-convolution"))]
            trueos_kokoro_dispatch::native_conv::yield_now().await;
            Ok(())
        })
    }
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
    started: Instant,
    first_pcm_us: Option<u128>,
    frames: u64,
    sink_us: u128,
}
#[derive(Clone)]
pub struct TtsOutput(Arc<Mutex<OutputState>>);
impl TtsOutput {
    fn new(sink: AudioSink) -> Self {
        Self(Arc::new(Mutex::new(OutputState {
            sink,
            finished: false,
            error: None,
            started: Instant::now(),
            first_pcm_us: None,
            frames: 0,
            sink_us: 0,
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
        // The caller owns output collection (bounded completed clip or WAV file).
        // Playback is handed off only after the complete synthesis succeeds.
        let first_pcm_us = state.started.elapsed().as_micros();
        let sink_started = Instant::now();
        let result = (state.sink)(&chunk.samples_i16_stereo_48k);
        state.sink_us += sink_started.elapsed().as_micros();
        if let Err(reason) = result {
            state.error = Some(reason);
            state.finished = true;
            return Err(TtsOutputError::Closed(chunk));
        }
        state.first_pcm_us.get_or_insert(first_pcm_us);
        state.frames += (count / TTS_PCM_CHANNELS) as u64;
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
    next_job: u64,
}
impl Engine {
    pub fn load() -> Result<Self, String> {
        #[cfg(all(target_os = "trueos", feature = "native-convolution"))]
        {
            match trueos_kokoro_dispatch::native_conv::install_default() {
                Ok(pool) => crate::diagnostics::record(false, format_args!("ttstt: shared preparation requested_compute_aps={} preference=performance-first", pool.worker_count())),
                Err(error) => crate::diagnostics::record(true, format_args!("ttstt: native workers unavailable; serial fallback: {error}")),
            }
        }
        let started = Instant::now();
        let cached = INITIALIZED.get().is_some();
        crate::log_info!(target: "ttstt"; "ttstt: warmup phase=begin cached={} quant_lane={}", cached as u8,
            trueos_kokoro_dispatch::CpuDispatchPlan::detect().quantized_lane());
        let models = INITIALIZED
            .get_or_init(|| {
                let models = ModelSet::bundled();
                let models = Box::leak(Box::new(models));
                crate::ttstt_kokoro::install();
                let backend = *BACKEND.get().ok_or("speech-backend-missing")?;
                let mut warm = backend.create_warm_job()?;
                run_job(warm.as_mut(), models, &mut JobTiming::default())?;
                if !backend.tts_ready() {
                    return Err(backend
                        .warm_failure_reason()
                        .unwrap_or("kokoro-not-ready")
                        .into());
                }
                Ok(models as &'static ModelSet)
            })
            .as_ref();
        crate::diagnostics::detail(
            models.is_err(),
            format_args!(
                "ttstt: warmup elapsed_us={} cached={} status={}",
                started.elapsed().as_micros(),
                cached as u8,
                if models.is_ok() { "ok" } else { "failed" }
            ),
        );
        let models = models.map_err(Clone::clone)?;
        Ok(Self {
            models,
            next_job: 1,
        })
    }
    pub fn synthesize(&mut self, request: TtsRequest, sink: AudioSink) -> Result<(), String> {
        #[cfg(all(feature = "native-convolution", not(feature = "async-convolution")))]
        if trueos_kokoro_dispatch::native_conv::installed() {
            return trueos_kokoro_dispatch::native_conv::block_on(self.synthesize_async(request,sink)).map_err(String::from)?;
        }
        #[cfg(feature = "async-graph")]
        if trueos_kokoro_dispatch::async_graph::installed() {
            return Err("async-graph requires synthesize_async".into());
        }
        #[cfg(feature = "async-convolution")]
        if trueos_kokoro_dispatch::async_conv::installed() {
            return Err("async convolution pool requires synthesize_async".into());
        }
        request.validate()?;
        let job_id = self.next_job;
        self.next_job += 1;
        let text_bytes = request.text.len();
        crate::diagnostics::record(
            false,
            format_args!("ttstt: job={job_id} submitted bytes={text_bytes}"),
        );
        let output = TtsOutput::new(sink);
        let mut timing = JobTiming::default();
        // All admission, inference and output errors share the completion path,
        // so every submitted job gets one short done or failed notice.
        let result: Result<(), String> = (|| {
            let mut job = BACKEND
                .get()
                .ok_or("speech-backend-missing")?
                .create_tts_job(BackendTtsRequest {
                    request,
                    output: output.clone(),
                })?;
            let result = run_job(job.as_mut(), self.models, &mut timing);
            let state = output.0.lock().unwrap();
            if let Some(reason) = state.error {
                return Err(reason.into());
            }
            result?;
            if !state.finished {
                return Err("speech ended without completion".into());
            }
            Ok(())
        })();
        let state = output.0.lock().unwrap();
        let compute_us = timing.slice_us.saturating_sub(state.sink_us);
        let compute_ms = compute_us / 1000;
        match &result {
            Ok(()) => crate::diagnostics::record(
                false,
                format_args!("ttstt: job={job_id} done compute_ms={compute_ms}"),
            ),
            Err(error) => crate::diagnostics::record(
                true,
                format_args!("ttstt: job={job_id} failed compute_ms={compute_ms} error={error}"),
            ),
        }
        crate::diagnostics::detail(
            result.is_err(),
            format_args!(
                "ttstt: synthesis job={} text_bytes={} total_us={} first_pcm_us={} synth_work_us={} sink_us={} audio_ms={} frames={} slices={} status={}",
                job_id,
                text_bytes,
                state.started.elapsed().as_micros(),
                state
                    .first_pcm_us
                    .map_or_else(|| "none".into(), |v| v.to_string()),
                compute_us,
                state.sink_us,
                state.frames * 1000 / 48_000,
                state.frames,
                timing.slices,
                if result.is_ok() { "ok" } else { "failed" }
            ),
        );
        result
    }
    /// Await owned native work or the optional host experiment without blocking the coordinator.
    #[cfg(any(feature = "async-convolution", feature = "native-convolution"))]
    pub async fn synthesize_async(&mut self, request: TtsRequest, sink: AudioSink) -> Result<(), String> {
        request.validate()?;
        let job_id = self.next_job;
        self.next_job += 1;
        let text_bytes = request.text.len();
        crate::diagnostics::record(
            false,
            format_args!("ttstt: job={job_id} submitted bytes={text_bytes}"),
        );
        let output = TtsOutput::new(sink);
        let mut timing = JobTiming::default();
        // All admission, inference and output errors share the completion path,
        // so every submitted job gets one short done or failed notice.
        let result: Result<(), String> = (async {
            let mut job = BACKEND
                .get()
                .ok_or("speech-backend-missing")?
                .create_tts_job(BackendTtsRequest {
                    request,
                    output: output.clone(),
                })?;
            let result = run_job_async(job.as_mut(), self.models, &mut timing).await;
            let state = output.0.lock().unwrap();
            if let Some(reason) = state.error {
                return Err(reason.into());
            }
            result?;
            if !state.finished {
                return Err("speech ended without completion".into());
            }
            Ok(())
        }).await;
        let state = output.0.lock().unwrap();
        let compute_us = timing.slice_us.saturating_sub(state.sink_us);
        let compute_ms = compute_us / 1000;
        match &result {
            Ok(()) => crate::diagnostics::record(
                false,
                format_args!("ttstt: job={job_id} done compute_ms={compute_ms}"),
            ),
            Err(error) => crate::diagnostics::record(
                true,
                format_args!("ttstt: job={job_id} failed compute_ms={compute_ms} error={error}"),
            ),
        }
        crate::diagnostics::detail(
            result.is_err(),
            format_args!(
                "ttstt: synthesis job={} text_bytes={} total_us={} first_pcm_us={} synth_work_us={} sink_us={} audio_ms={} frames={} slices={} status={}",
                job_id,
                text_bytes,
                state.started.elapsed().as_micros(),
                state
                    .first_pcm_us
                    .map_or_else(|| "none".into(), |v| v.to_string()),
                compute_us,
                state.sink_us,
                state.frames * 1000 / 48_000,
                state.frames,
                timing.slices,
                if result.is_ok() { "ok" } else { "failed" }
            ),
        );
        result
    }
}
#[derive(Default)]
struct JobTiming {
    slices: u64,
    slice_us: u128,
}

fn run_job(
    job: &mut dyn InferenceJob,
    models: &'static ModelSet,
    timing: &mut JobTiming,
) -> Result<(), String> {
    let deadline = Instant::now() + SYNTHESIS_TIMEOUT;
    loop {
        if Instant::now() >= deadline {
            return Err("speech-inference-timeout".into());
        }
        let started = Instant::now();
        let progress = job.run_slice(models, WorkerContext);
        timing.slices += 1;
        timing.slice_us += started.elapsed().as_micros();
        match progress {
            JobProgress::Pending => std::thread::yield_now(),
            JobProgress::Complete => return Ok(()),
            JobProgress::Failed(reason) => return Err(reason.into()),
        }
    }
}

#[cfg(any(feature = "async-convolution", feature = "native-convolution"))]
async fn run_job_async(
    job: &mut dyn InferenceJob, models: &'static ModelSet, timing: &mut JobTiming,
) -> Result<(), String> {
    let deadline = Instant::now() + SYNTHESIS_TIMEOUT;
    loop {
        if Instant::now() >= deadline { return Err("speech-inference-timeout".into()); }
        let started = Instant::now();
        let progress = job.run_slice(models, WorkerContext);
        timing.slices += 1;
        let result = match progress {
            #[cfg(feature = "async-convolution")]
            JobProgress::Pending => smol::future::or(job.wait_ready(), async {
                smol::Timer::after(deadline.saturating_duration_since(Instant::now())).await;
                Err("speech-inference-timeout")
            }).await.map_err(String::from),
            #[cfg(not(feature = "async-convolution"))]
            JobProgress::Pending => trueos_kokoro_dispatch::native_conv::with_timeout(
                deadline.saturating_duration_since(Instant::now()), job.wait_ready()
            ).await.map_err(String::from).and_then(|result|result.map_err(String::from)),
            JobProgress::Complete => {
                timing.slice_us += started.elapsed().as_micros();
                return Ok(());
            }
            JobProgress::Failed(reason) => Err(reason.into()),
        };
        // Calculation elapsed includes time parked awaiting compute workers.
        // Counting coordinator polls alone would falsely report a speedup.
        timing.slice_us += started.elapsed().as_micros();
        result?;
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
    fn model_views_borrow_the_embedded_assets_without_copying() {
        let models = ModelSet::bundled();
        assert_eq!(
            models.kokoro().bytes().as_ptr(),
            crate::bundle::model().as_ptr()
        );
        assert_eq!(
            models.kokoro_heart().bytes().as_ptr(),
            crate::bundle::heart().as_ptr()
        );
        assert_eq!(
            models.kokoro_g2p().bytes().as_ptr(),
            crate::bundle::g2p().as_ptr()
        );
    }
    #[test]
    fn rejects_other_voices_instead_of_claiming_them_available() {
        let request: TtsRequest =
            serde_json::from_str(r#"{"text":"Hello","voice":"am_michael"}"#).unwrap();
        assert!(request.validate().is_err());
    }
    #[test]
    fn emoji_only_job_finishes_without_pcm_or_mutating_request_text() {
        let request: TtsRequest =
            serde_json::from_str(r#"{"text":"😄 ❤️ 👩🏽‍💻 🇩🇪 1️⃣"}"#).unwrap();
        request.validate().unwrap();
        assert!(request.is_emoji_only());
        let original = request.text.clone();
        let mut engine = Engine::load().unwrap();
        engine
            .synthesize(request.clone(), Box::new(|_| panic!("emoji-only speech emitted PCM")))
            .unwrap();
        assert_eq!(request.text, original);
    }
}
