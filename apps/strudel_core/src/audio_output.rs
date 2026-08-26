extern crate alloc;

use alloc::{format, string::String, vec::Vec};

use trueos::{
    audio::{
        ERR_BUSY, NativeBlockHeaderV2, NativeEngine, NativeRenderCommandV2, PlaybackParams, Stream,
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

    /// Submit a fully validated block to the V2 native scheduler. `write_all`
    /// below deliberately remains for the existing PCM fallback path.
    pub fn render_native(
        &self,
        header: &NativeBlockHeaderV2,
        commands: &[NativeRenderCommandV2],
    ) -> Result<usize, String> {
        self.native
            .render_v2(header, commands)
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
    const FRAMES: usize = 4_800;
    let mut out = Vec::with_capacity(FRAMES);
    for frame in 0..FRAMES {
        let decay = (FRAMES - frame) as i64;
        let value = match kind {
            0 => {
                let phase = (frame * 11) % 436;
                let triangle = if phase < 218 {
                    phase as i64
                } else {
                    (436 - phase) as i64
                };
                (triangle * 24_000 * decay) / (218 * FRAMES as i64)
            }
            1 => {
                let n = frame as u32;
                let noise = n.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                (i64::from((noise >> 16) as i16) * decay * 55) / (100 * FRAMES as i64)
            }
            _ => {
                let n = frame as u32;
                let noise = n.wrapping_mul(22_695_477).wrapping_add(1);
                (i64::from((noise >> 16) as i16) * decay * 80) / (100 * FRAMES as i64)
            }
        };
        out.push(value.clamp(-32_767, 32_767) as i16);
    }
    out
}
