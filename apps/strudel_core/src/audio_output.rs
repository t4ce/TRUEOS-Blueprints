extern crate alloc;

use alloc::{format, string::String, vec::Vec};

use trueos::{
    audio::{
        ERR_BUSY, NativeBlockHeaderV3, NativeEngine, NativeRenderCommandV3, PlaybackParams, Stream,
    },
    vsys,
};

pub struct AudioOutput {
    stream: Stream,
    native: NativeEngine,
}

impl AudioOutput {
    pub fn open() -> Result<Self, String> {
        let stream = Stream::open_playback(PlaybackParams::s16le_stereo_48k())
            .map_err(|code| format!("audio open failed rc={code}"))?;
        stream
            .start()
            .map_err(|code| format!("audio start failed rc={code}"))?;
        let output = Self {
            native: NativeEngine::from_stream(stream),
            stream,
        };
        output.register_builtin_samples()?;
        Ok(output)
    }

    /// Register the small deterministic PCM set used by the native Strudel
    /// adapter.  These are deliberately named `trueos:*`, not upstream banks:
    /// callers can opt into real PCM with `.bank("trueos")`, while all other
    /// sounds retain oscillator fallback.
    fn register_builtin_samples(&self) -> Result<(), String> {
        for (name, kind) in [("bd", 0u8), ("hh", 1u8), ("sd", 2u8)] {
            let values = builtin_sample(kind);
            self.native
                .register_sample(source_id("trueos", name), 1, 48_000, &values)
                .map_err(|error| format!("register trueos sample {name} failed: {error:?}"))?;
        }
        Ok(())
    }

    pub fn queued_frames(&self) -> Result<usize, i32> {
        self.stream.queued_frames()
    }

    pub fn buffer_frames(&self) -> Result<usize, i32> {
        self.stream.buffer_frames()
    }

    /// Submit a fully validated block to the V3 native scheduler. `write_all`
    /// below deliberately remains for the existing PCM fallback path.
    pub fn render_native(
        &self,
        header: &NativeBlockHeaderV3,
        commands: &[NativeRenderCommandV3],
    ) -> Result<usize, String> {
        self.native
            .render_v3(header, commands)
            .map_err(|error| format!("native audio render failed: {error:?}"))
    }

    /// Write a complete interleaved stereo block, retrying the bounded queue on
    /// EBUSY. The C ABI returns frames, while the Rust slice is measured in
    /// samples, so every successful frame advances by two i16 values.
    pub fn write_all(&self, samples: &[i16]) -> Result<usize, String> {
        if samples.len() % 2 != 0 {
            return Err("audio block is not stereo aligned".into());
        }

        let mut sample_offset = 0usize;
        let mut frame_total = 0usize;
        while sample_offset < samples.len() {
            match self.stream.write_interleaved_i16(&samples[sample_offset..]) {
                Ok(0) => {
                    vsys::poll_once();
                    vsys::sleep_ms(1);
                }
                Ok(frames) => {
                    let advanced = frames
                        .checked_mul(2)
                        .ok_or_else(|| String::from("audio frame count overflow"))?;
                    if advanced > samples.len() - sample_offset {
                        return Err("audio ABI reported more frames than supplied".into());
                    }
                    sample_offset += advanced;
                    frame_total = frame_total.saturating_add(frames);
                }
                Err(ERR_BUSY) => {
                    vsys::poll_once();
                    vsys::sleep_ms(1);
                }
                Err(code) => return Err(format!("audio write failed rc={code}")),
            }
        }
        Ok(frame_total)
    }
}

fn source_id(bank: &str, sound: &str) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for byte in bank
        .bytes()
        .chain(core::iter::once(b':'))
        .chain(sound.bytes())
    {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash & 0x1f_ff_ff_ff_ff_ff_ff
}

fn builtin_sample(kind: u8) -> Vec<i16> {
    let frames = match kind {
        0 => 24_000, // kick: 500 ms pitch/body decay
        1 => 6_000,  // closed hat: 125 ms metallic noise
        _ => 12_000, // snare/rim fallback: 250 ms noise + body
    };
    let mut phase = 0u32;
    let mut noise = 0x6d2b_79f5u32 ^ u32::from(kind);
    let mut previous_noise = 0i64;
    let mut out = Vec::with_capacity(frames);
    for frame in 0..frames {
        let remaining = (frames - frame) as u64;
        let envelope_q15 = (remaining * remaining * 32_767 / (frames * frames) as u64) as i64;
        noise = noise.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let noise_sample = i64::from((noise >> 16) as i16);
        let value = match kind {
            0 => {
                // A short downward pitch sweep supplies the 909-like kick
                // transient without turning the drum's MIDI note into a held
                // oscillator. Frequency falls from roughly 150 Hz to 48 Hz.
                let sweep = 102u64 * remaining * remaining / (frames * frames) as u64;
                let frequency_hz = 48 + sweep;
                phase = phase.wrapping_add(((frequency_hz << 32) / 48_000) as u32);
                let body = triangle_q15(phase) * envelope_q15 / 32_767;
                let click = if frame < 144 {
                    noise_sample * (144 - frame) as i64 / 576
                } else {
                    0
                };
                body * 28_000 / 32_767 + click
            }
            1 => {
                // Differentiate white noise to remove the low body which made
                // the fallback hat sound pitched.
                let high = noise_sample - previous_noise;
                previous_noise = noise_sample;
                high * envelope_q15 * 18_000 / (32_767 * 32_767)
            }
            _ => {
                phase = phase.wrapping_add(((180u64 << 32) / 48_000) as u32);
                let body = triangle_q15(phase) * envelope_q15 / 32_767;
                let noise_body = noise_sample * envelope_q15 / 32_767;
                body * 7 / 20 + noise_body * 13 / 20
            }
        };
        out.push(value.clamp(-32_767, 32_767) as i16);
    }
    out
}

fn triangle_q15(phase: u32) -> i64 {
    let position = i64::from(phase >> 16);
    if position < 32_768 {
        position * 2 - 32_767
    } else {
        98_303 - position * 2
    }
}
