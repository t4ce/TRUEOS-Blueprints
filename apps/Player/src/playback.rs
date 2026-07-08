use std::io;

const DEFAULT_RATE_HZ: u32 = 48_000;
const DEFAULT_CHANNELS: usize = 2;
const SAMPLE_BYTES: usize = 2;
const FRAME_BYTES: usize = DEFAULT_CHANNELS * SAMPLE_BYTES;
const MAX_AUDIO_FILE_BYTES: usize = 16 * 1024 * 1024;
const STREAM_CHUNK_FRAMES: usize = 4096;
const STREAM_TARGET_QUEUE_FRAMES: usize = DEFAULT_RATE_HZ as usize;
const STREAM_MAX_PUMP_CHUNKS: usize = 16;

#[derive(Debug, Clone)]
pub struct LoadedTrack {
    pub path: String,
    pub file_name: String,
    pub codec: String,
    pub frames: usize,
    pub duration_secs: u64,
    pub size_label: String,
}

#[derive(Debug, Clone)]
pub struct PlaybackEngine {
    loaded: Option<DecodedAudio>,
    cursor_samples: usize,
    chunk_samples: usize,
    volume_percent: u32,
    #[cfg(any(target_os = "trueos", target_os = "zkvm"))]
    stream: Option<trueos::audio::Stream>,
}

#[derive(Debug, Clone)]
struct DecodedAudio {
    path: String,
    samples: Vec<i16>,
    frames: usize,
    codec: AudioCodec,
    bytes_len: usize,
}

#[derive(Debug, Clone, Copy)]
enum AudioCodec {
    Aac,
    WavPcm,
    RawPcm,
}

impl AudioCodec {
    const fn label(self) -> &'static str {
        match self {
            Self::Aac => "AAC (LC)",
            Self::WavPcm => "PCM WAV",
            Self::RawPcm => "PCM",
        }
    }
}

impl PlaybackEngine {
    pub fn new(volume_percent: u16) -> Self {
        Self {
            loaded: None,
            cursor_samples: 0,
            chunk_samples: STREAM_CHUNK_FRAMES * DEFAULT_CHANNELS,
            volume_percent: u32::from(volume_percent).min(100),
            #[cfg(any(target_os = "trueos", target_os = "zkvm"))]
            stream: None,
        }
    }

    pub fn is_loaded(&self) -> bool {
        self.loaded.is_some()
    }

    pub fn load_path(&mut self, path: &str) -> Result<LoadedTrack, io::Error> {
        let path = path.trim();
        if path.is_empty() {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "missing path"));
        }

        let bytes = read_audio_file(path)?;
        let decoded = decode_audio(path, &bytes)?;
        if decoded.samples.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "audio file is empty",
            ));
        }

        self.close_stream();
        self.cursor_samples = 0;
        let track = decoded.track();
        self.loaded = Some(decoded);
        Ok(track)
    }

    pub fn play(&mut self) -> Result<(), io::Error> {
        if self.loaded.is_none() {
            return Err(io::Error::new(io::ErrorKind::NotFound, "no loaded track"));
        }
        self.play_impl()
    }

    pub fn pause(&mut self) -> Result<(), io::Error> {
        self.pause_impl()
    }

    pub fn feed(&mut self) -> Result<bool, io::Error> {
        self.feed_impl()
    }

    pub fn position_secs(&self) -> u64 {
        let frames = self.cursor_samples / DEFAULT_CHANNELS;
        (frames as u64) / u64::from(DEFAULT_RATE_HZ)
    }

    pub fn close_stream(&mut self) {
        self.close_stream_impl();
    }

    pub fn clear(&mut self) {
        self.close_stream();
        self.loaded = None;
        self.cursor_samples = 0;
    }

    #[cfg(any(target_os = "trueos", target_os = "zkvm"))]
    fn play_impl(&mut self) -> Result<(), io::Error> {
        if let Some(stream) = self.stream {
            stream.resume().map_err(audio_error)?;
            return Ok(());
        }

        let loaded = self.loaded.as_ref().expect("checked loaded");
        if self.cursor_samples >= loaded.samples.len() {
            self.cursor_samples = 0;
        }

        let stream =
            trueos::audio::Stream::open_playback(trueos::audio::PlaybackParams::s16le_stereo_48k())
                .map_err(audio_error)?;
        let applied = stream
            .set_volume_percent(self.volume_percent)
            .map_err(audio_error)?;
        self.volume_percent = applied.min(100);

        self.pump_stream(stream, STREAM_TARGET_QUEUE_FRAMES)?;
        stream.start().map_err(audio_error)?;
        self.stream = Some(stream);
        Ok(())
    }

    #[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
    fn play_impl(&mut self) -> Result<(), io::Error> {
        Ok(())
    }

    #[cfg(any(target_os = "trueos", target_os = "zkvm"))]
    fn pause_impl(&mut self) -> Result<(), io::Error> {
        if let Some(stream) = self.stream {
            stream.pause().map_err(audio_error)?;
        }
        Ok(())
    }

    #[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
    fn pause_impl(&mut self) -> Result<(), io::Error> {
        Ok(())
    }

    #[cfg(any(target_os = "trueos", target_os = "zkvm"))]
    fn feed_impl(&mut self) -> Result<bool, io::Error> {
        let Some(loaded) = self.loaded.as_ref() else {
            return Ok(false);
        };

        if self.cursor_samples >= loaded.samples.len() {
            self.close_stream();
            return Ok(true);
        }

        let Some(stream) = self.stream else {
            self.play_impl()?;
            return Ok(false);
        };

        self.pump_stream(stream, STREAM_TARGET_QUEUE_FRAMES)?;
        Ok(false)
    }

    #[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
    fn feed_impl(&mut self) -> Result<bool, io::Error> {
        Ok(false)
    }

    #[cfg(any(target_os = "trueos", target_os = "zkvm"))]
    fn pump_stream(
        &mut self,
        stream: trueos::audio::Stream,
        target_queued_frames: usize,
    ) -> Result<(), io::Error> {
        for _ in 0..STREAM_MAX_PUMP_CHUNKS {
            let Some(loaded) = self.loaded.as_ref() else {
                return Ok(());
            };
            if self.cursor_samples >= loaded.samples.len() {
                return Ok(());
            }
            if let Ok(queued) = stream.queued_frames() {
                if queued >= target_queued_frames {
                    return Ok(());
                }
            }

            let end = self
                .cursor_samples
                .saturating_add(self.chunk_samples)
                .min(loaded.samples.len());
            if self.write_stream_chunk(stream, end)? == 0 {
                return Ok(());
            }
        }
        Ok(())
    }

    #[cfg(any(target_os = "trueos", target_os = "zkvm"))]
    fn write_stream_chunk(
        &mut self,
        stream: trueos::audio::Stream,
        end: usize,
    ) -> Result<usize, io::Error> {
        let Some(loaded) = self.loaded.as_ref() else {
            return Ok(0);
        };
        if self.cursor_samples >= end {
            return Ok(0);
        }

        match stream.write_interleaved_i16(&loaded.samples[self.cursor_samples..end]) {
            Ok(frames) => {
                let written = frames.saturating_mul(DEFAULT_CHANNELS);
                self.cursor_samples = self
                    .cursor_samples
                    .saturating_add(written.min(end - self.cursor_samples));
                Ok(written)
            }
            Err(trueos::audio::ERR_BUSY) => Ok(0),
            Err(err) => Err(audio_error(err)),
        }
    }

    #[cfg(any(target_os = "trueos", target_os = "zkvm"))]
    fn close_stream_impl(&mut self) {
        if let Some(stream) = self.stream.take() {
            let _ = stream.drop_stream();
            let _ = stream.close();
        }
    }

    #[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
    fn close_stream_impl(&mut self) {}
}

impl Drop for PlaybackEngine {
    fn drop(&mut self) {
        self.close_stream();
    }
}

impl DecodedAudio {
    fn track(&self) -> LoadedTrack {
        LoadedTrack {
            path: self.path.clone(),
            file_name: file_name(&self.path),
            codec: self.codec.label().to_string(),
            frames: self.frames,
            duration_secs: duration_secs(self.frames),
            size_label: size_label(self.bytes_len),
        }
    }
}

fn decode_audio(path: &str, bytes: &[u8]) -> Result<DecodedAudio, io::Error> {
    if bytes.get(0..4) == Some(b"RIFF") {
        return decode_wav_pcm_s16_stereo_48k(path, bytes);
    }
    if bytes.get(4..8) == Some(b"ftyp") {
        return decode_m4a(path, bytes);
    }
    if path.ends_with(".pcm") || path.ends_with(".raw") {
        return decode_raw_pcm_s16_stereo_48k(path, bytes, AudioCodec::RawPcm);
    }
    Err(io::Error::new(
        io::ErrorKind::InvalidData,
        "unsupported audio container",
    ))
}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
fn decode_m4a(path: &str, bytes: &[u8]) -> Result<DecodedAudio, io::Error> {
    let decoded = crate::audio::m4a::decode_m4a_to_pcm_48k_stereo_s16(bytes).map_err(|err| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("m4a decode failed: {err:?}"),
        )
    })?;
    Ok(DecodedAudio {
        path: path.to_string(),
        frames: decoded.frames,
        samples: decoded.samples,
        codec: AudioCodec::Aac,
        bytes_len: bytes.len(),
    })
}

#[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
fn decode_m4a(_path: &str, _bytes: &[u8]) -> Result<DecodedAudio, io::Error> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "m4a decode is only enabled for TRUEOS blueprint builds",
    ))
}

fn decode_raw_pcm_s16_stereo_48k(
    path: &str,
    bytes: &[u8],
    codec: AudioCodec,
) -> Result<DecodedAudio, io::Error> {
    if bytes.is_empty() || bytes.len() % FRAME_BYTES != 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "raw PCM must be s16le/stereo/48k",
        ));
    }

    let mut samples = Vec::with_capacity(bytes.len() / SAMPLE_BYTES);
    for pair in bytes.chunks_exact(SAMPLE_BYTES) {
        samples.push(i16::from_le_bytes([pair[0], pair[1]]));
    }
    let frames = samples.len() / DEFAULT_CHANNELS;
    Ok(DecodedAudio {
        path: path.to_string(),
        samples,
        frames,
        codec,
        bytes_len: bytes.len(),
    })
}

fn decode_wav_pcm_s16_stereo_48k(path: &str, bytes: &[u8]) -> Result<DecodedAudio, io::Error> {
    let (data_off, data_len) = wav_pcm_s16_stereo_48k_data_range(bytes)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "unsupported WAV format"))?;
    if data_len == 0 || data_len % FRAME_BYTES != 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid WAV data length",
        ));
    }

    let data = bytes
        .get(data_off..data_off + data_len)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "invalid WAV data range"))?;
    decode_raw_pcm_s16_stereo_48k(path, data, AudioCodec::WavPcm)
}

fn wav_pcm_s16_stereo_48k_data_range(bytes: &[u8]) -> Option<(usize, usize)> {
    if bytes.len() < 44 || bytes.get(0..4)? != b"RIFF" || bytes.get(8..12)? != b"WAVE" {
        return None;
    }

    let mut fmt_ok = false;
    let mut data_range = None;
    let mut off = 12usize;

    while off + 8 <= bytes.len() {
        let chunk_id = bytes.get(off..off + 4)?;
        let chunk_len = usize::try_from(le_u32(bytes, off + 4)?).ok()?;
        let chunk_data_off = off + 8;
        let chunk_end = chunk_data_off.checked_add(chunk_len)?;
        if chunk_end > bytes.len() {
            return None;
        }

        if chunk_id == b"fmt " {
            let format_tag = le_u16(bytes, chunk_data_off)?;
            let channels = le_u16(bytes, chunk_data_off + 2)?;
            let sample_rate = le_u32(bytes, chunk_data_off + 4)?;
            let block_align = le_u16(bytes, chunk_data_off + 12)?;
            let bits_per_sample = le_u16(bytes, chunk_data_off + 14)?;
            fmt_ok = format_tag == 1
                && usize::from(channels) == DEFAULT_CHANNELS
                && sample_rate == DEFAULT_RATE_HZ
                && bits_per_sample == 16
                && usize::from(block_align) == FRAME_BYTES;
        } else if chunk_id == b"data" {
            data_range = Some((chunk_data_off, chunk_len));
        }

        let padded_len = (chunk_len + 1) & !1;
        off = chunk_data_off.checked_add(padded_len)?;
    }

    if fmt_ok { data_range } else { None }
}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
fn read_audio_file(path: &str) -> Result<Vec<u8>, io::Error> {
    let bytes = trueos::vfs::read_file(path.as_bytes()).map_err(vfs_error)?;
    validate_audio_file_size(path, bytes)
}

#[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
fn read_audio_file(path: &str) -> Result<Vec<u8>, io::Error> {
    let bytes = std::fs::read(path)?;
    validate_audio_file_size(path, bytes)
}

fn validate_audio_file_size(path: &str, bytes: Vec<u8>) -> Result<Vec<u8>, io::Error> {
    if bytes.len() > MAX_AUDIO_FILE_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "{path} is too large for blueprint playback preload: {} bytes",
                bytes.len()
            ),
        ));
    }
    Ok(bytes)
}

fn le_u16(bytes: &[u8], off: usize) -> Option<u16> {
    let b = bytes.get(off..off + 2)?;
    Some(u16::from_le_bytes([b[0], b[1]]))
}

fn le_u32(bytes: &[u8], off: usize) -> Option<u32> {
    let b = bytes.get(off..off + 4)?;
    Some(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

fn duration_secs(frames: usize) -> u64 {
    (frames as u64).div_ceil(u64::from(DEFAULT_RATE_HZ))
}

fn file_name(path: &str) -> String {
    path.rsplit('/')
        .next()
        .filter(|name| !name.is_empty())
        .unwrap_or(path)
        .to_string()
}

fn size_label(bytes: usize) -> String {
    if bytes >= 1024 * 1024 {
        format!("{:.2} MB", bytes as f64 / (1024.0 * 1024.0))
    } else if bytes >= 1024 {
        format!("{:.1} KiB", bytes as f64 / 1024.0)
    } else {
        format!("{bytes} B")
    }
}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
fn audio_error(err: i32) -> io::Error {
    io::Error::from_raw_os_error(-err)
}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
fn vfs_error(err: i32) -> io::Error {
    io::Error::from_raw_os_error(-err)
}
