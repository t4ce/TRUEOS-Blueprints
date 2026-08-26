extern crate alloc;

use alloc::{format, string::String};

use trueos::{
    audio::{PlaybackParams, Stream, ERR_BUSY},
    vsys,
};

pub struct AudioOutput {
    stream: Stream,
}

impl AudioOutput {
    pub fn open() -> Result<Self, String> {
        let stream = Stream::open_playback(PlaybackParams::s16le_stereo_48k())
            .map_err(|code| format!("audio open failed rc={code}"))?;
        stream
            .start()
            .map_err(|code| format!("audio start failed rc={code}"))?;
        Ok(Self { stream })
    }

    pub fn queued_frames(&self) -> Result<usize, i32> {
        self.stream.queued_frames()
    }

    pub fn buffer_frames(&self) -> Result<usize, i32> {
        self.stream.buffer_frames()
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
            match self
                .stream
                .write_interleaved_i16(&samples[sample_offset..])
            {
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
