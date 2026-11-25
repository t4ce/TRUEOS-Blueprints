use anyhow::{Result, bail};

#[cfg(target_os = "trueos")]
use crate::cues::SpiritCue;
#[cfg(target_os = "trueos")]
use std::num::NonZeroU64;
#[cfg(any(target_os = "trueos", test))]
use std::time::{Duration, Instant};

#[cfg(any(target_os = "trueos", test))]
const IDLE_POLL_INTERVAL: Duration = Duration::from_millis(25);

#[cfg(target_os = "trueos")]
unsafe extern "C" {
    fn trueos_cabi_http_audio_clip_submit_v1(pcm_ptr: *const u8, pcm_bytes: usize) -> i32;
    fn trueos_cabi_http_audio_stream_v1(
        action: u32,
        stream: u64,
        pcm_ptr: *const u8,
        pcm_bytes: usize,
    ) -> i64;
}

#[cfg(target_os = "trueos")]
pub struct Stream {
    deadline: Instant,
    token: Option<u64>,
    previous: Option<f32>,
    bytes: Vec<u8>,
    first_write: Option<Instant>,
    previous_stream: Option<u64>,
    completed_stream: Option<u64>,
    spirit_cue: Option<(NonZeroU64, SpiritCue)>,
}

#[cfg(target_os = "trueos")]
impl Stream {
    pub fn new(
        deadline: Instant,
        previous_stream: Option<u64>,
        spirit_cue: Option<(NonZeroU64, SpiritCue)>,
    ) -> Self {
        Self {
            deadline,
            token: None,
            previous: None,
            bytes: Vec::new(),
            first_write: None,
            previous_stream,
            completed_stream: None,
            spirit_cue,
        }
    }

    pub fn push(&mut self, mono: &[f32]) -> Result<()> {
        // Two 48 kHz stereo s16le frames per 24 kHz mono sample.
        for block in mono.chunks(64 * 1024 / 8) {
            self.bytes.clear();
            append_interpolated(block, &mut self.previous, &mut self.bytes)?;
            self.write()?;
        }
        Ok(())
    }

    pub fn finish(&mut self) -> Result<()> {
        self.bytes.clear();
        flush_interpolation(&mut self.previous, &mut self.bytes);
        self.write()?;
        if let Some(token) = self.token {
            let status = stream_call(2, token, &[]);
            if status != 0 {
                bail!("native audio stream finish failed: {status}")
            }
            self.token = None;
            self.completed_stream = Some(token);
        }
        Ok(())
    }

    pub fn first_write(&self) -> Option<Instant> {
        self.first_write
    }

    pub fn completed_stream(&self) -> Option<u64> {
        self.completed_stream
    }

    pub fn push_cue(&mut self, pcm: &[u8]) -> Result<()> {
        self.bytes.clear();
        flush_interpolation(&mut self.previous, &mut self.bytes);
        self.write()?;
        for block in pcm.chunks(64 * 1024) {
            self.bytes.clear();
            self.bytes.extend_from_slice(block);
            self.write()?;
        }
        Ok(())
    }

    fn write(&mut self) -> Result<()> {
        if self.bytes.is_empty() {
            return Ok(());
        }
        if self.token.is_none() {
            if let Some(previous) = self.previous_stream.take() {
                wait_until_idle_with(
                    self.deadline,
                    || source_playing(previous, self.deadline),
                    Instant::now,
                    std::thread::sleep,
                )?;
            }
            loop {
                let remaining = self.deadline.saturating_duration_since(Instant::now());
                if remaining.is_zero() {
                    bail!("native audio stream deadline expired while waiting for the sink")
                }
                match stream_call(0, 0, &[]) {
                    token @ 1.. => {
                        self.token = Some(token as u64);
                        break;
                    }
                    -16 => std::thread::sleep(remaining.min(Duration::from_millis(5))),
                    status => bail!("native audio stream begin failed: {status}"),
                }
            }
        }
        if Instant::now() >= self.deadline {
            bail!("native audio stream deadline expired before writing")
        }
        let token = self.token.expect("stream reserved");
        let status = stream_call(1, token, &self.bytes);
        if status != 0 {
            bail!("native audio stream write failed: {status}")
        }
        self.first_write.get_or_insert_with(Instant::now);
        if let Some((turn, cue)) = self.spirit_cue.take() {
            notify_spirit(token, turn, cue);
        }
        Ok(())
    }
}

#[cfg(target_os = "trueos")]
impl Drop for Stream {
    fn drop(&mut self) {
        if let Some(token) = self.token {
            let _ = stream_call(3, token, &[]);
        }
    }
}

#[cfg(target_os = "trueos")]
fn stream_call(action: u32, token: u64, bytes: &[u8]) -> i64 {
    let ptr = if bytes.is_empty() {
        std::ptr::null()
    } else {
        bytes.as_ptr()
    };
    unsafe { trueos_cabi_http_audio_stream_v1(action, token, ptr, bytes.len()) }
}

#[cfg(target_os = "trueos")]
fn notify_spirit(token: u64, turn: NonZeroU64, cue: SpiritCue) {
    let signal = cue.payload(turn);
    // The guest stream ABI accepts payload pointers from the guest heap.
    let mut payload = Vec::new();
    if let Err(error) = payload.try_reserve_exact(signal.len()) {
        crate::notice(format!(
            "ptts: Spirit cue {cue:?} turn={turn} allocation failed: {error}"
        ));
        return;
    }
    payload.extend_from_slice(&signal);
    let status = stream_call(6, token, &payload);
    if status != 0 {
        crate::notice(format!(
            "ptts: Spirit cue {cue:?} turn={turn} unavailable: {status}"
        ));
    }
}

#[cfg(target_os = "trueos")]
pub fn close_spirit_turn(
    turn: NonZeroU64,
    preceding_stream: Option<u64>,
    deadline: Instant,
    cue: SpiritCue,
) {
    if let Some(id) = preceding_stream
        && let Err(error) = wait_until_idle_with(
            deadline,
            || source_playing(id, deadline),
            Instant::now,
            std::thread::sleep,
        )
    {
        crate::notice(format!(
            "ptts: Spirit cue {cue:?} turn={turn} playback wait failed: {error:#}"
        ));
    }
    notify_spirit(0, turn, cue);
}

#[cfg(target_os = "trueos")]
pub fn submit(mono: &[f32]) -> Result<()> {
    submit_with_deadline(mono, Instant::now() + Duration::from_secs(120))
}

/// Wait for the existing complete-clip sink before handing off this utterance.
/// The caller's deadline includes synthesis, conversion and this wait.
///
/// This prevents overlap between jobs owned by this serial service. The old
/// kernel ABI cannot reserve the sink: another producer can still win between
/// the final status check and submit, whose success code includes busy discard.
#[cfg(target_os = "trueos")]
pub fn submit_with_deadline(mono: &[f32], deadline: Instant) -> Result<()> {
    if Instant::now() >= deadline {
        bail!("native playback deadline expired before conversion")
    }
    let bytes = native_clip_bytes(mono)?;
    wait_until_idle_with(
        deadline,
        || sink_playing(deadline),
        Instant::now,
        std::thread::sleep,
    )?;
    let status = unsafe { trueos_cabi_http_audio_clip_submit_v1(bytes.as_ptr(), bytes.len()) };
    if status != 0 {
        bail!("native audio handoff failed: {status}")
    }
    crate::notice(format!(
        "ptts: clip submitted bytes={} format=stereo-s16le-48000",
        bytes.len()
    ));
    Ok(())
}

/// Convert 24 kHz mono to 48 kHz stereo with one interpolated frame per sample.
#[cfg(any(target_os = "trueos", test))]
fn native_clip_bytes(mono: &[f32]) -> Result<Vec<u8>> {
    if mono.is_empty() || mono.len() > 24_000 * 120 {
        bail!("invalid native clip length")
    }
    let mut bytes = Vec::new();
    let mut previous = None;
    append_interpolated(mono, &mut previous, &mut bytes)?;
    flush_interpolation(&mut previous, &mut bytes);
    Ok(bytes)
}

/// Retain one mono sample so interpolation crosses decoder block boundaries.
fn append_interpolated(
    mono: &[f32],
    previous: &mut Option<f32>,
    bytes: &mut Vec<u8>,
) -> Result<()> {
    if mono.iter().any(|sample| !sample.is_finite()) {
        bail!("invalid PCM")
    }
    bytes.try_reserve_exact(mono.len() * 8)?;
    for &next in mono {
        if let Some(sample) = previous.replace(next) {
            append_stereo_pair(bytes, sample, next);
        }
    }
    Ok(())
}

fn flush_interpolation(previous: &mut Option<f32>, bytes: &mut Vec<u8>) {
    if let Some(sample) = previous.take() {
        append_stereo_pair(bytes, sample, sample);
    }
}

fn append_stereo_pair(bytes: &mut Vec<u8>, sample: f32, next: f32) {
    for value in [sample, sample * 0.5 + next * 0.5] {
        let sample_bytes = pcm_sample_le(value);
        bytes.extend_from_slice(&sample_bytes);
        bytes.extend_from_slice(&sample_bytes);
    }
}

fn pcm_sample_le(sample: f32) -> [u8; 2] {
    ((sample.clamp(-1.0, 1.0) * i16::MAX as f32) as i16).to_le_bytes()
}

#[cfg(any(target_os = "trueos", test))]
fn wait_until_idle_with(
    deadline: Instant,
    mut playing: impl FnMut() -> Result<bool>,
    mut now: impl FnMut() -> Instant,
    mut pause: impl FnMut(Duration),
) -> Result<()> {
    loop {
        if now() >= deadline {
            bail!("native playback deadline expired while waiting for idle audio sink")
        }
        let busy = playing()
            .map_err(|error| anyhow::anyhow!("native audio status check failed: {error:#}"))?;
        let remaining = deadline.saturating_duration_since(now());
        if remaining.is_zero() {
            bail!("native playback deadline expired while checking audio sink")
        }
        if !busy {
            return Ok(());
        }
        pause(IDLE_POLL_INTERVAL.min(remaining));
    }
}

#[cfg(target_os = "trueos")]
fn sink_playing(deadline: Instant) -> Result<bool> {
    let request = b"GET /audio/status HTTP/1.1\r\nHost: 127.0.0.1:82\r\nConnection: close\r\n\r\n";
    status_playing(&crate::loopback::exchange(82, request, deadline)?)
}

#[cfg(target_os = "trueos")]
fn source_playing(id: u64, _deadline: Instant) -> Result<bool> {
    source_playing_result(stream_call(4, id, &[]))
}

#[cfg(any(target_os = "trueos", test))]
fn source_playing_result(status: i64) -> Result<bool> {
    match status {
        0 => Ok(false),
        1 => Ok(true),
        _ => bail!("native audio source status failed: {status}"),
    }
}

/// Ubuntu's WAV capture uses exactly the same cue bytes and speech converter.
#[cfg(not(target_os = "trueos"))]
#[derive(Default)]
pub struct HostClip {
    previous: Option<f32>,
    pub bytes: Vec<u8>,
}
#[cfg(not(target_os = "trueos"))]
impl HostClip {
    pub fn push(&mut self, mono: &[f32]) -> Result<()> {
        append_interpolated(mono, &mut self.previous, &mut self.bytes)
    }
    pub fn push_cue(&mut self, pcm: &[u8]) {
        flush_interpolation(&mut self.previous, &mut self.bytes);
        self.bytes.extend_from_slice(pcm);
    }
    pub fn finish(&mut self) {
        flush_interpolation(&mut self.previous, &mut self.bytes);
    }
    pub fn write(&self, path: &std::path::Path) -> Result<()> {
        use std::io::Write;
        let bytes = u32::try_from(self.bytes.len())?;
        let mut file = std::fs::File::create(path)?;
        file.write_all(b"RIFF")?;
        file.write_all(
            &bytes
                .checked_add(36)
                .ok_or_else(|| anyhow::anyhow!("WAV too large"))?
                .to_le_bytes(),
        )?;
        file.write_all(b"WAVEfmt \x10\0\0\0\x01\0\x02\0")?;
        file.write_all(&48_000u32.to_le_bytes())?;
        file.write_all(&192_000u32.to_le_bytes())?;
        file.write_all(&4u16.to_le_bytes())?;
        file.write_all(&16u16.to_le_bytes())?;
        file.write_all(b"data")?;
        file.write_all(&bytes.to_le_bytes())?;
        file.write_all(&self.bytes)?;
        Ok(())
    }
}

#[cfg(any(target_os = "trueos", test))]
fn status_playing(body: &[u8]) -> Result<bool> {
    if body.len() > crate::loopback::BODY_LIMIT {
        bail!("audio status JSON body is too large")
    }
    let value: serde_json::Value = serde_json::from_slice(body)?;
    value
        .get("playing")
        .and_then(|value| value.as_bool())
        .ok_or_else(|| anyhow::anyhow!("audio status JSON has no boolean playing field"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::loopback::{HEADER_LIMIT, response_body_length};

    #[test]
    fn waiting_for_pocket_does_not_wait_for_another_producer() {
        assert!(source_playing_result(1).unwrap());
        assert!(!source_playing_result(0).unwrap());
        // An unsupported query or transport error must never be treated as
        // permission to overlap or discard the preceding speech.
        for error in [-22, -5, -2, 2] {
            assert!(source_playing_result(error).is_err());
        }
    }

    #[test]
    fn cues_surround_speech_without_losing_the_last_interpolated_frame() {
        let intro = crate::cues::Intro::Starttalk.pcm();
        let done = crate::cues::done_pcm();
        let mut clip = HostClip::default();
        clip.push_cue(intro);
        clip.push(&[0.25, -0.5]).unwrap();
        clip.push(&[0.75]).unwrap();
        clip.push_cue(done);
        clip.finish();
        let voice = native_clip_bytes(&[0.25, -0.5, 0.75]).unwrap();
        assert_eq!(&clip.bytes[..intro.len()], intro);
        assert_eq!(&clip.bytes[intro.len()..intro.len() + voice.len()], &voice);
        assert_eq!(&clip.bytes[intro.len() + voice.len()..], done);
    }

    #[test]
    fn native_conversion_preserves_interpolation_stereo_and_clipping() {
        let expected = [
            0i16, 0, 16_383, 16_383, 32_767, 32_767, 0, 0, -32_767, -32_767, -32_767, -32_767,
        ];
        assert_eq!(
            native_clip_bytes(&[0.0, 1.0, -1.0]).unwrap(),
            expected
                .into_iter()
                .flat_map(i16::to_le_bytes)
                .collect::<Vec<_>>()
        );
        assert_eq!(native_clip_bytes(&[2.0]).unwrap(), [255, 127].repeat(4));
        assert!(native_clip_bytes(&[f32::NAN]).is_err());
    }

    #[test]
    fn streaming_conversion_matches_whole_clip_across_callback_boundaries() {
        let mono = [0.0, 0.91, -0.23, -1.1, 0.17, 1.2, -0.79];
        let expected = native_clip_bytes(&mono).unwrap();
        for width in 1..=mono.len() {
            let mut previous = None;
            let mut block = Vec::new();
            let mut output = Vec::new();
            for chunk in mono.chunks(width) {
                block.clear();
                append_interpolated(chunk, &mut previous, &mut block).unwrap();
                output.extend_from_slice(&block);
            }
            block.clear();
            flush_interpolation(&mut previous, &mut block);
            assert_eq!(block.len(), 8);
            output.extend_from_slice(&block);
            assert_eq!(output, expected, "callback width {width}");
        }
    }

    use std::cell::Cell;
    use std::collections::VecDeque;

    #[test]
    fn busy_sink_waits_until_idle_before_handoff_can_continue() {
        let start = Instant::now();
        let clock = Cell::new(start);
        let calls = Cell::new(0);
        let mut states = VecDeque::from([true, true, false]);
        wait_until_idle_with(
            start + Duration::from_secs(1),
            || {
                calls.set(calls.get() + 1);
                Ok(states.pop_front().unwrap())
            },
            || clock.get(),
            |duration| clock.set(clock.get() + duration),
        )
        .unwrap();
        assert_eq!(calls.get(), 3);
        assert_eq!(clock.get().duration_since(start), IDLE_POLL_INTERVAL * 2);
        assert!(states.is_empty());
    }

    #[test]
    fn busy_sink_has_a_finite_wait_and_expired_work_never_polls() {
        let start = Instant::now();
        let clock = Cell::new(start);
        let calls = Cell::new(0);
        let error = wait_until_idle_with(
            start + IDLE_POLL_INTERVAL * 2,
            || {
                calls.set(calls.get() + 1);
                Ok(true)
            },
            || clock.get(),
            |duration| clock.set(clock.get() + duration),
        )
        .unwrap_err();
        assert!(error.to_string().contains("deadline expired"));
        assert_eq!(calls.get(), 2);
        assert!(
            wait_until_idle_with(
                start,
                || panic!("expired request must not inspect sink"),
                || start,
                |_| panic!("expired request must not pause"),
            )
            .is_err()
        );
    }

    #[test]
    fn backend_errors_and_status_checks_finishing_late_cannot_submit() {
        let start = Instant::now();
        let error = wait_until_idle_with(
            start + Duration::from_secs(1),
            || bail!("connection refused"),
            || start,
            |_| panic!("transport error must not be retried or treated as idle"),
        )
        .unwrap_err();
        assert!(error.to_string().contains("connection refused"));

        let clock = Cell::new(start);
        assert!(
            wait_until_idle_with(
                start + Duration::from_secs(1),
                || {
                    clock.set(start + Duration::from_secs(2));
                    Ok(false)
                },
                || clock.get(),
                |_| panic!("late idle response must not be accepted"),
            )
            .is_err()
        );
    }

    #[test]
    fn status_requires_bounded_unambiguous_framing_and_a_boolean() {
        let body = br#"{"listeners":0,"capacity":4,"clip_id":"1","playing":false}"#;
        let header = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        assert_eq!(response_body_length(header.as_bytes()).unwrap(), body.len());
        assert!(!status_playing(body).unwrap());
        assert!(status_playing(br#"{"playing":true}"#).unwrap());
        for header in [
            "HTTP/1.1 503 Busy\r\nContent-Length: 0\r\n\r\n".to_owned(),
            "HTTP/1.1 200 OK\r\n\r\n".to_owned(),
            "HTTP/1.1 200 OK\r\nContent-Length: 1\r\nContent-Length: 1\r\n\r\n".to_owned(),
            "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n".to_owned(),
            format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n",
                crate::loopback::BODY_LIMIT + 1
            ),
            format!(
                "HTTP/1.1 200 OK\r\nX-Pad: {}\r\nContent-Length: 0\r\n\r\n",
                "x".repeat(HEADER_LIMIT)
            ),
        ] {
            assert!(response_body_length(header.as_bytes()).is_err());
        }
        for body in [
            &b"{}"[..],
            &b"{\"playing\":\"false\"}"[..],
            &b"not JSON"[..],
        ] {
            assert!(status_playing(body).is_err());
        }
    }
}

#[cfg(not(target_os = "trueos"))]
pub fn write_wav(path: &str, pcm: &[f32]) -> Result<()> {
    use std::io::Write;
    if pcm.is_empty() || pcm.iter().any(|s| !s.is_finite()) {
        bail!("invalid PCM")
    }
    let data_bytes = u32::try_from(
        pcm.len()
            .checked_mul(2)
            .ok_or_else(|| anyhow::anyhow!("WAV too large"))?,
    )?;
    let mut file = std::fs::File::create(path)?;
    file.write_all(b"RIFF")?;
    file.write_all(&(36 + data_bytes).to_le_bytes())?;
    file.write_all(b"WAVEfmt \x10\0\0\0\x01\0\x01\0")?;
    file.write_all(&24_000u32.to_le_bytes())?;
    file.write_all(&48_000u32.to_le_bytes())?;
    file.write_all(&2u16.to_le_bytes())?;
    file.write_all(&16u16.to_le_bytes())?;
    file.write_all(b"data")?;
    file.write_all(&data_bytes.to_le_bytes())?;
    for &sample in pcm {
        file.write_all(&pcm_sample_le(sample))?;
    }
    Ok(())
}
