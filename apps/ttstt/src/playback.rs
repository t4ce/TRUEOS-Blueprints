//! PCM output through the existing owner-scoped Blueprint audio ABI.
use crate::ttstt_service::AudioSink;

#[cfg(any(target_os = "trueos", test))]
fn write_pcm_with(
    samples: &[i16],
    mut write: impl FnMut(&[i16]) -> Result<usize, i32>,
) -> Result<(), &'static str> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    let mut offset = 0;
    while offset < samples.len() {
        // A full native PCM lane reports EBUSY (-16), including when a guest
        // could not commit even its first sub-transfer. Retry the same samples.
        let frames = match write(&samples[offset..]) {
            Ok(frames) => frames,
            Err(-16) => 0,
            Err(_) => return Err("audio-write-failed"),
        };
        let consumed = frames.checked_mul(2).ok_or("invalid-audio-write-count")?;
        if consumed > samples.len() - offset {
            return Err("invalid-audio-write-count");
        }
        offset += consumed;
        if consumed == 0 {
            if std::time::Instant::now() >= deadline {
                return Err("audio-write-timeout");
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }
    Ok(())
}

#[cfg(target_os = "trueos")]
pub struct Playback(trueos::audio::Stream);
#[cfg(target_os = "trueos")]
impl Playback {
    pub fn open() -> Result<Self, String> {
        let stream =
            trueos::audio::Stream::open_playback(trueos::audio::PlaybackParams::s16le_stereo_48k())
                .map_err(|e| format!("audio open: {e}"))?;
        let playback = Self(stream);
        playback
            .0
            .start()
            .map_err(|e| format!("audio start: {e}"))?;
        Ok(playback)
    }
    pub fn sink(&self) -> AudioSink {
        let stream = self.0;
        Box::new(move |samples| {
            write_pcm_with(samples, |remaining| stream.write_interleaved_i16(remaining))
        })
    }
    pub fn drain(&self) -> Result<(), String> {
        self.0
            .drain(30_000)
            .map_err(|e| format!("audio drain: {e}"))
    }
}
#[cfg(target_os = "trueos")]
impl Drop for Playback {
    fn drop(&mut self) {
        let _ = self.0.drop_stream();
        let _ = self.0.close();
    }
}

#[cfg(not(target_os = "trueos"))]
pub struct Playback;
#[cfg(not(target_os = "trueos"))]
impl Playback {
    pub fn open() -> Result<Self, String> {
        Err("live playback requires TRUEOS; use --wav on the host".into())
    }
    pub fn sink(&self) -> AudioSink {
        Box::new(|_| Err("live-playback-unavailable"))
    }
    pub fn drain(&self) -> Result<(), String> {
        Ok(())
    }
}

pub fn wav_header(frames: u32) -> Result<Vec<u8>, String> {
    let bytes = frames.checked_mul(4).ok_or("WAV size overflow")?;
    let total = bytes.checked_add(36).ok_or("WAV size overflow")?;
    let mut h = Vec::with_capacity(44);
    h.extend_from_slice(b"RIFF");
    h.extend_from_slice(&total.to_le_bytes());
    h.extend_from_slice(b"WAVEfmt ");
    h.extend_from_slice(&16u32.to_le_bytes());
    h.extend_from_slice(&1u16.to_le_bytes());
    h.extend_from_slice(&2u16.to_le_bytes());
    h.extend_from_slice(&48_000u32.to_le_bytes());
    h.extend_from_slice(&192_000u32.to_le_bytes());
    h.extend_from_slice(&4u16.to_le_bytes());
    h.extend_from_slice(&16u16.to_le_bytes());
    h.extend_from_slice(b"data");
    h.extend_from_slice(&bytes.to_le_bytes());
    Ok(h)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn busy_and_partial_writes_preserve_all_stereo_samples() {
        let samples = [1, 2, 3, 4, 5, 6];
        let mut calls = 0;
        let mut written = Vec::new();
        write_pcm_with(&samples, |remaining| {
            calls += 1;
            if calls == 1 {
                return Err(-16);
            }
            written.extend_from_slice(&remaining[..2]);
            Ok(1)
        })
        .unwrap();
        assert_eq!(written, samples);
        assert_eq!(calls, 4);
    }
    #[test]
    fn invalid_frame_count_and_device_failure_are_terminal() {
        assert_eq!(
            write_pcm_with(&[1, 2], |_| Ok(2)),
            Err("invalid-audio-write-count")
        );
        assert_eq!(
            write_pcm_with(&[1, 2], |_| Err(-19)),
            Err("audio-write-failed")
        );
    }
}
