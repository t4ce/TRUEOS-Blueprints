//! Collect a finished utterance, then hand it to the kernel's HTTP playback sink.
use crate::ttstt_service::AudioSink;
use std::sync::{Arc, Mutex};

// Must match the versioned kernel handoff contract: stereo s16le/48 kHz, 120 s.
const MAX_CLIP_BYTES: usize = 48_000 * 4 * 120;

#[cfg(target_os = "trueos")]
unsafe extern "C" {
    fn trueos_cabi_http_audio_clip_submit_v1(pcm_ptr: *const u8, pcm_bytes: usize) -> i32;
}

#[derive(Default)]
struct ClipBuffer {
    bytes: Vec<u8>,
    handed_off: bool,
}
impl ClipBuffer {
    fn append(&mut self, samples: &[i16]) -> Result<(), &'static str> {
        if self.handed_off {
            return Err("clip-already-handed-off");
        }
        if samples.is_empty() || samples.len() % 2 != 0 {
            return Err("invalid-pcm-chunk");
        }
        let bytes = samples
            .len()
            .checked_mul(2)
            .ok_or("clip-too-long-maximum-120-seconds")?;
        let total = self
            .bytes
            .len()
            .checked_add(bytes)
            .ok_or("clip-too-long-maximum-120-seconds")?;
        if total > MAX_CLIP_BYTES {
            return Err("clip-too-long-maximum-120-seconds");
        }
        self.bytes
            .try_reserve(bytes)
            .map_err(|_| "clip-allocation-failed")?;
        for sample in samples {
            self.bytes.extend_from_slice(&sample.to_le_bytes());
        }
        Ok(())
    }
}

pub struct Playback(Arc<Mutex<ClipBuffer>>);
impl Playback {
    pub fn open() -> Result<Self, String> {
        #[cfg(not(any(target_os = "trueos", test)))]
        return Err("HTTP playback requires TRUEOS; use --wav on the host".into());
        #[cfg(any(target_os = "trueos", test))]
        Ok(Self(Arc::new(Mutex::new(ClipBuffer::default()))))
    }

    /// Inference produces chunks at any speed. Nothing reaches playback here.
    pub fn sink(&self) -> AudioSink {
        let output = self.0.clone();
        Box::new(move |samples| output.lock().unwrap().append(samples))
    }

    fn handoff_with(&self, submit: impl FnOnce(&[u8]) -> i32) -> Result<(), String> {
        let bytes = {
            let mut clip = self.0.lock().unwrap();
            if clip.handed_off {
                return Err("clip-already-handed-off".into());
            }
            if clip.bytes.is_empty() {
                return Err("empty-speech-clip".into());
            }
            clip.handed_off = true;
            std::mem::take(&mut clip.bytes)
        };
        // Kernel copies before returning. A busy sink is a successful discard;
        // no retry could append this utterance behind a competing source.
        match submit(&bytes) {
            0 => Ok(()),
            code => Err(format!("HTTP playback clip handoff failed: {code}")),
        }
    }

    /// Call only after synthesis succeeded. Returns after handoff, not playback.
    pub fn handoff(&self) -> Result<(), String> {
        #[cfg(target_os = "trueos")]
        return self.handoff_with(|bytes| unsafe {
            trueos_cabi_http_audio_clip_submit_v1(bytes.as_ptr(), bytes.len())
        });
        #[cfg(not(target_os = "trueos"))]
        Err("HTTP playback requires TRUEOS; use --wav on the host".into())
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
    fn inference_chunks_are_one_ordered_complete_handoff() {
        let playback = Playback::open().unwrap();
        let mut sink = playback.sink();
        sink(&[1, -2]).unwrap();
        sink(&[3, -4]).unwrap();
        playback
            .handoff_with(|bytes| {
                assert_eq!(bytes, &[1, 0, 254, 255, 3, 0, 252, 255]);
                0
            })
            .unwrap();
        assert!(playback.0.lock().unwrap().bytes.is_empty());
        assert_eq!(sink(&[5, 6]), Err("clip-already-handed-off"));
        assert!(
            playback
                .handoff_with(|_| panic!("must not submit twice"))
                .is_err()
        );
    }

    #[test]
    fn failed_synthesis_can_drop_the_collected_prefix_without_playing() {
        let playback = Playback::open().unwrap();
        let mut sink = playback.sink();
        sink(&[1, 2]).unwrap();
        // The caller's synthesis error returns before handoff. Neither dropping
        // the collector nor its callback submits a partial utterance.
        assert!(!playback.0.lock().unwrap().handed_off);
        drop(sink);
        drop(playback);
    }

    #[test]
    fn malformed_and_oversize_input_does_not_extend_the_clip() {
        let mut clip = ClipBuffer::default();
        assert_eq!(clip.append(&[1]), Err("invalid-pcm-chunk"));
        clip.bytes.resize(MAX_CLIP_BYTES, 0);
        assert_eq!(
            clip.append(&[1, 2]),
            Err("clip-too-long-maximum-120-seconds")
        );
        assert_eq!(clip.bytes.len(), MAX_CLIP_BYTES);
    }

    #[test]
    fn handoff_errors_are_reported_without_retrying_or_partial_publication() {
        let playback = Playback::open().unwrap();
        assert!(playback.handoff_with(|_| panic!("empty clip")).is_err());
        playback.sink()(&[1, 2]).unwrap();
        assert!(playback.handoff_with(|_| -12).unwrap_err().contains("-12"));
        assert!(playback.handoff_with(|_| panic!("retry")).is_err());
    }
}
