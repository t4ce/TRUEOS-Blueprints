//! Pocket TTS inference on the Rust/XN CPU backend using prepared text tokens,
//! a verified checkpoint, and the embedded Azelma voice state.

use anyhow::{Context, Result, bail};
use ptts_core::flow_lm::{FlowLMState, NormalRng, StepInput};
use ptts_core::transformer::{LayerAttentionState, StreamingMHAState, StreamingTransformerState};
use ptts_core::tts_model::{TTSConfig, TTSModel, TTSState};
use safetensors::{Dtype, SafeTensors};
use std::collections::HashMap;
use std::time::Instant;
use xn::{CpuDevice, Tensor, Unquantized};

type Backend = Unquantized<f32, CpuDevice>;

const SAMPLE_RATE: usize = 24_000;
const SEED: u64 = 42;
const MIN_FRAMES_BEFORE_EOS: usize = 6;
const MAX_GENERATION_FRAMES: usize = 1_500;

/// The frontend prepares text tokens before calling the low-level model.
struct PreparedTokens;

impl ptts_core::Tokenizer for PreparedTokens {
    fn encode(&self, _: &str) -> xn::Result<Vec<u32>> {
        Err(xn::Error::msg("ptts engine requires prepared token IDs"))
    }

    fn decode(&self, _: &[u32]) -> xn::Result<String> {
        Err(xn::Error::msg("ptts engine does not decode text tokens"))
    }
}

pub struct Engine {
    model: TTSModel<Backend>,
    voice: StreamingTransformerState<f32, CpuDevice>,
    voice_positions: usize,
}

impl Engine {
    pub fn load(checkpoint: Vec<u8>) -> Result<Self> {
        // The size-one XN pool uses the calling thread for inference.
        xn::set_num_threads(1);
        let config: TTSConfig = serde_json::from_slice(crate::bundle::config())
            .context("invalid bundled ptts configuration")?;

        // Move the verified checkpoint into XN and release it after the model
        // tensors are loaded.
        let weights = xn::nn::VB::from_bytes_with_key_map(vec![checkpoint], xn::CPU, |key| {
            let key = ptts_core::loader::remap_key(key)?;
            if key == "flow_lm.bos_before_voice" {
                Some("flow_lm.condition_provider.conditioners.speaker_wavs.sink".into())
            } else {
                Some(key)
            }
        })
        .context("cannot load ptts weights")?
        .root();
        let model =
            TTSModel::<Backend>::load(&weights, Box::new(PreparedTokens), &config, &HashMap::new())
                .context("cannot initialize ptts CPU model")?;
        drop(weights);
        if model.sample_rate() != SAMPLE_RATE {
            bail!("ptts model must produce 24 kHz audio")
        }

        let (voice, voice_positions) = import_voice(crate::bundle::voice(), &config)?;
        Ok(Self {
            model,
            voice,
            voice_positions,
        })
    }

    /// Emit mono 24 kHz PCM and return the number of 80 ms frames decoded.
    pub fn generate_with_limits(
        &self,
        tokens: &[u32],
        temperature: f32,
        frames_after_eos: usize,
        deadline: Instant,
        on_pcm: &mut dyn FnMut(&[f32]) -> Result<()>,
    ) -> Result<usize> {
        if tokens.is_empty() || tokens.len() > 512 {
            bail!("ptts requires between 1 and 512 prepared tokens")
        }
        if !temperature.is_finite() || temperature < 0.0 {
            bail!("ptts temperature must be finite and nonnegative")
        }
        if !(1..=8).contains(&frames_after_eos) {
            bail!("invalid EOS tail frame count")
        }
        let max_frames =
            (((tokens.len() as f64 / 3.0 + 2.0) * 12.5).ceil() as usize).min(MAX_GENERATION_FRAMES);
        let seq_budget = self.voice_positions + tokens.len() + max_frames;
        let mut state = TTSState {
            flow_lm_state: FlowLMState {
                transformer_state: self.voice.with_seq_budget(seq_budget)?,
                conditions: None,
            },
        };
        self.model.prompt_text(&mut state, tokens)?;
        let mut mimi = self.model.init_mimi_state(1)?;
        let mut rng = NormalRng::new(temperature, SEED)?;
        let mut previous = None;
        let mut eos_step = None;
        let mut frames = 0;
        let mut emitted_audio = false;
        let mut stopped_at_eos = false;
        for step in 0..max_frames {
            if Instant::now() >= deadline {
                bail!("speech generation deadline exceeded")
            }
            let input = match previous.as_ref() {
                None => StepInput::Bos { batch: 1 },
                Some(latent) => StepInput::Latent(latent),
            };
            let (latent, is_eos) = self.model.generate_step(&mut state, input, &mut rng)?;
            if is_eos && eos_step.is_none() && step >= MIN_FRAMES_BEFORE_EOS {
                eos_step = Some(step);
            }
            if eos_step.is_some_and(|end| step >= end + frames_after_eos) {
                stopped_at_eos = true;
                break;
            }
            let audio = self.model.decode_latent(&latent, &mut mimi)?;
            let mut pcm = audio.to_vec()?;
            if !pcm.is_empty() {
                if !emitted_audio {
                    // Match the official frontend's initial 5 ms click fade.
                    let fade = SAMPLE_RATE / 200;
                    let count = fade.min(pcm.len());
                    for (i, sample) in pcm[..count].iter_mut().enumerate() {
                        *sample *= i as f32 / (fade - 1) as f32;
                    }
                }
                if pcm.iter().any(|sample| !sample.is_finite()) {
                    bail!("ptts generated non-finite PCM")
                }
                on_pcm(&pcm)?;
                emitted_audio = true;
            }
            frames += 1;
            previous = Some(latent);
            // Preserve a cooperative boundary between expensive inference steps.
            std::thread::yield_now();
        }
        if !stopped_at_eos {
            bail!("ptts reached its generation frame budget without EOS")
        }
        if !emitted_audio {
            bail!("ptts generated no audio")
        }
        Ok(frames)
    }
}

/// Import the official exported K/V state directly from embedded safetensors.
fn import_voice(
    bytes: &[u8],
    config: &TTSConfig,
) -> Result<(StreamingTransformerState<f32, CpuDevice>, usize)> {
    let file = SafeTensors::deserialize(bytes).context("invalid bundled Azelma voice state")?;
    let transformer = &config.flow_lm;
    let heads = transformer.num_kv_heads.unwrap_or(transformer.num_heads);
    let head_dim = transformer.attn_dim.unwrap_or(transformer.d_model) / transformer.num_heads;
    let mut layer_states = Vec::with_capacity(transformer.num_layers);
    let mut positions = None;
    for layer in 0..transformer.num_layers {
        let prefix = format!("transformer.layers.{layer}.self_attn");
        let cache = file.tensor(&format!("{prefix}/cache"))?;
        let [2, 1, capacity, h, d] = cache.shape()[..] else {
            bail!("Azelma layer {layer} cache must have shape [2,1,time,heads,dim]")
        };
        if h != heads || d != head_dim {
            bail!("Azelma layer {layer} does not match the bundled model")
        }
        let used = match file.tensor(&format!("{prefix}/offset")) {
            Ok(offset) => {
                if offset.dtype() != Dtype::I64 || offset.data().len() != 8 {
                    bail!("Azelma layer {layer} offset must be one i64")
                }
                usize::try_from(i64::from_le_bytes(offset.data().try_into().unwrap()))
                    .context("negative Azelma cache offset")?
            }
            Err(_) => match file.tensor(&format!("{prefix}/current_end")) {
                Ok(current_end) => current_end.shape().first().copied().unwrap_or(0),
                Err(_) => capacity,
            },
        };
        if used == 0 || used > capacity || positions.is_some_and(|other| other != used) {
            bail!("Azelma layer {layer} has an invalid or inconsistent cache offset")
        }
        positions = Some(used);
        let data = tensor_f32(&cache)?;
        let plane = capacity * h * d;
        let length = used * h * d;
        let take = |start| -> Result<Tensor<f32, CpuDevice>> {
            Ok(Tensor::from_vec(
                data[start..start + length].to_vec(),
                (1, used, h, d),
                &xn::CPU,
            )?)
        };
        layer_states.push(LayerAttentionState::FlowLm(StreamingMHAState {
            k_cache: take(0)?,
            v_cache: take(plane)?,
            current_end: used,
        }));
    }
    let positions = positions.context("Azelma voice state has no layers")?;
    Ok((StreamingTransformerState { layer_states }, positions))
}

fn tensor_f32(tensor: &safetensors::tensor::TensorView<'_>) -> Result<Vec<f32>> {
    Ok(match tensor.dtype() {
        Dtype::F32 => tensor
            .data()
            .chunks_exact(4)
            .map(|bytes| f32::from_le_bytes(bytes.try_into().unwrap()))
            .collect(),
        Dtype::BF16 => tensor
            .data()
            .chunks_exact(2)
            .map(|bytes| {
                f32::from_bits((u16::from_le_bytes(bytes.try_into().unwrap()) as u32) << 16)
            })
            .collect(),
        other => bail!("unsupported Azelma cache dtype {other:?}"),
    })
}
