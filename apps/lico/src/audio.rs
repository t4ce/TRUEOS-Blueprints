use crate::examples::PATCHES;
use glicol::Engine;
use glicol_parser::HighlightSpan;
use std::error::Error;
use std::io::{self, Cursor};
use std::time::Duration;
use tokio::sync::mpsc::{Receiver, Sender};

const BLOCK: usize = 128;
const SAMPLE_RATE: u32 = 48_000;
const BATCH_FRAMES: usize = 4 * BLOCK;
const BATCH_BYTES: usize = 4 * BATCH_FRAMES;
const STREAM_LIMIT: usize = SAMPLE_RATE as usize * 4 * 120;
pub const MONITOR_FRAMES: usize = BATCH_FRAMES;
pub const DEFAULT_BPM: u16 = 120;
const SAMPLES: [(&str, &[u8]); 4] = [
    (r"\808bd", include_bytes!("../assets/808bd.wav")),
    (r"\808sd", include_bytes!("../assets/808sd.wav")),
    (r"\808ch", include_bytes!("../assets/808ch.wav")),
    (r"\blip", include_bytes!("../assets/blip.wav")),
];

pub fn sample_names() -> impl Iterator<Item = &'static str> {
    SAMPLES.iter().map(|(name, _)| *name)
}

pub enum Command {
    Run(String),
    ChangeTempo(i16),
    Stop,
}

pub enum Status {
    Ready(u32),
    Tempo(u16),
    PatchParsed(String, Vec<HighlightSpan>),
    Playing(String),
    Stopped,
    PatchError(String),
    AudioError(String),
}

fn engine() -> Result<Engine<BLOCK>, hound::Error> {
    let mut engine = Engine::new();
    engine.set_sr(SAMPLE_RATE as usize);
    for (name, wav) in SAMPLES {
        let mut reader = hound::WavReader::new(Cursor::new(wav))?;
        let spec = reader.spec();
        let channels = usize::from(spec.channels);
        let frames = reader.duration() as usize;
        let mut planar = vec![0.0_f32; frames * channels];
        for (index, sample) in reader.samples::<i16>().enumerate() {
            let offset = (index % channels) * frames + index / channels;
            planar[offset] = f32::from(sample?) / 32768.0;
        }
        engine.add_sample(name, planar.leak(), channels, spec.sample_rate as usize);
    }
    Ok(engine)
}

fn render(engine: &mut Engine<BLOCK>, pcm: &mut [u8], waveform: &mut [f32; MONITOR_FRAMES]) {
    for (bytes, wave) in pcm
        .chunks_exact_mut(BLOCK * 4)
        .zip(waveform.chunks_exact_mut(BLOCK))
    {
        let stereo = engine.next_block(Vec::new());
        wave.copy_from_slice(&stereo[0][..]);
        for (i, frame) in bytes.chunks_exact_mut(4).enumerate() {
            for (channel, sample) in frame.chunks_exact_mut(2).enumerate() {
                let value = (stereo[channel][i] * 32768.0).round() as i16;
                sample.copy_from_slice(&value.to_le_bytes());
            }
        }
    }
}

pub fn render_wav(code: &str, path: &str, seconds: u32) -> Result<(), Box<dyn Error>> {
    let mut engine = engine()?;
    engine.update_with_code(code)?;
    let spec = hound::WavSpec {
        channels: 2,
        sample_rate: SAMPLE_RATE,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut wav = hound::WavWriter::create(path, spec)?;
    let mut pcm = [0; BATCH_BYTES];
    let mut waveform = [0.; MONITOR_FRAMES];
    let mut remaining = u64::from(seconds) * u64::from(SAMPLE_RATE) * 2;
    while remaining > 0 {
        render(&mut engine, &mut pcm, &mut waveform);
        let samples = remaining.min((BATCH_BYTES / 2) as u64) as usize;
        for bytes in pcm[..samples * 2].chunks_exact(2) {
            wav.write_sample(i16::from_le_bytes([bytes[0], bytes[1]]))?;
        }
        remaining -= samples as u64;
    }
    wav.finalize()?;
    Ok(())
}

pub fn self_check() -> Result<(), Box<dyn Error>> {
    let mut engine = engine()?;
    let mut pcm = vec![0; BATCH_BYTES];
    let mut waveform = [0.; MONITOR_FRAMES];
    for &(name, code) in PATCHES {
        engine
            .update_with_code(code)
            .map_err(|error| io::Error::other(format!("{name}: {error}")))?;
        let batches = (8 * SAMPLE_RATE as usize).div_ceil(BATCH_FRAMES);
        let mut energy = 0.0;
        let mut peak = 0.0_f64;
        for _ in 0..batches {
            render(&mut engine, &mut pcm, &mut waveform);
            for bytes in pcm.chunks_exact(2) {
                let sample = f64::from(i16::from_le_bytes([bytes[0], bytes[1]])) / 32768.0;
                energy += sample * sample;
                peak = peak.max(sample.abs());
            }
        }
        if peak >= f64::from(i16::MAX) / 32768.0 {
            return Err(format!("{name} reached full scale").into());
        }
        let rms_dbfs = 10.0 * (energy / (batches * BATCH_FRAMES * 2) as f64).log10();
        if rms_dbfs < -36.0 {
            return Err(format!("{name} rendered at only {rms_dbfs:.1} dBFS RMS").into());
        }
        let peak_dbfs = 20.0 * peak.log10();
        println!(
            "lico: {name} at {SAMPLE_RATE} Hz, {rms_dbfs:.1} dBFS RMS, {peak_dbfs:.1} dBFS peak"
        );
    }
    Ok(())
}

pub async fn serve(
    mut commands: Receiver<Command>,
    status: Sender<Status>,
    monitor: Sender<[f32; MONITOR_FRAMES]>,
) -> Result<(), Box<dyn Error>> {
    let mut engine = engine()?;
    let mut playing = false;
    let mut bpm = DEFAULT_BPM;
    let mut stream = None;
    // The kernel validates append buffers against the VM heap.
    let mut pcm = vec![0; BATCH_BYTES];
    let mut waveform = [0.; MONITOR_FRAMES];
    let mut ticks = tokio::time::interval(Duration::from_nanos(
        1_000_000_000 * BATCH_FRAMES as u64 / u64::from(SAMPLE_RATE),
    ));
    ticks.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let _ = status.try_send(Status::Ready(SAMPLE_RATE));

    loop {
        tokio::select! {
            command = commands.recv() => match command {
                Some(Command::Run(code)) => {
                    match engine.update_with_code_and_spans(&code) {
                        Ok(spans) => {
                            let _ = status.try_send(Status::PatchParsed(code.clone(), spans));
                            if !playing {
                                match Stream::open() {
                                    Ok(output) => {
                                        stream = Some(output);
                                        playing = true;
                                        ticks.reset();
                                    }
                                    Err(error) => {
                                        let _ = status.try_send(Status::AudioError(error.to_string()));
                                        continue;
                                    }
                                }
                            }
                            let _ = status.try_send(Status::Playing(code));
                        }
                        Err(error) => { let _ = status.try_send(Status::PatchError(error.to_string())); }
                    }
                }
                Some(Command::ChangeTempo(delta)) => {
                    bpm = bpm.saturating_add_signed(delta).clamp(20, 400);
                    engine.set_bpm(f32::from(bpm));
                    let _ = status.try_send(Status::Tempo(bpm));
                }
                Some(Command::Stop) => {
                    playing = false;
                    stream = None;
                    let samples = std::mem::take(&mut engine.samples_dict);
                    engine = Engine::new();
                    engine.samples_dict = samples;
                    engine.set_sr(SAMPLE_RATE as usize);
                    engine.set_bpm(f32::from(bpm));
                    let _ = status.try_send(Status::Stopped);
                }
                None => return Ok(()),
            },
            _ = ticks.tick(), if playing => {
                if let Err(error) = tick(&mut stream, &mut engine, &mut pcm, &mut waveform) {
                    stream = None;
                    playing = false;
                    let _ = status.try_send(Status::AudioError(error.to_string()));
                } else {
                    let _ = monitor.try_send(waveform);
                }
                tokio::task::yield_now().await;
            }
        }
    }
}

fn tick(
    stream: &mut Option<Stream>,
    engine: &mut Engine<BLOCK>,
    pcm: &mut [u8],
    waveform: &mut [f32; MONITOR_FRAMES],
) -> io::Result<()> {
    if stream.is_none() {
        match Stream::open() {
            Ok(output) => *stream = Some(output),
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => return Ok(()),
            Err(error) => return Err(error),
        }
    }
    let output = stream.as_mut().expect("stream opened");
    render(engine, pcm, waveform);
    output.append(pcm)?;
    if output.written == STREAM_LIMIT {
        output.finish()?;
        *stream = None;
    }
    Ok(())
}

struct Stream {
    token: Option<u64>,
    written: usize,
}

impl Stream {
    fn open() -> io::Result<Self> {
        match audio_call(0, 0, &[]) {
            id if id > 0 => Ok(Self {
                token: Some(id as u64),
                written: 0,
            }),
            -16 => Err(io::Error::new(
                io::ErrorKind::WouldBlock,
                "kernel audio sink is busy",
            )),
            status => Err(io::Error::other(format!(
                "kernel audio stream open failed ({status})"
            ))),
        }
    }

    fn append(&mut self, pcm: &[u8]) -> io::Result<()> {
        self.call(1, pcm)?;
        self.written += pcm.len();
        Ok(())
    }

    fn finish(&mut self) -> io::Result<()> {
        self.call(2, &[])?;
        self.token = None;
        Ok(())
    }

    fn call(&self, action: u32, pcm: &[u8]) -> io::Result<()> {
        let status = audio_call(action, self.token.expect("open audio stream"), pcm);
        if status == 0 {
            Ok(())
        } else {
            Err(io::Error::other(format!(
                "kernel audio stream action {action} failed ({status})"
            )))
        }
    }
}

impl Drop for Stream {
    fn drop(&mut self) {
        if let Some(token) = self.token {
            audio_call(3, token, &[]);
        }
    }
}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
fn audio_call(action: u32, token: u64, pcm: &[u8]) -> i64 {
    let ptr = if pcm.is_empty() {
        std::ptr::null()
    } else {
        pcm.as_ptr()
    };
    // PCM is read synchronously by the kernel and stays alive for the call.
    unsafe { v::bp_abi::trueos_cabi_http_audio_stream_v1(action, token, ptr, pcm.len()) }
}

#[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
fn audio_call(action: u32, _token: u64, _pcm: &[u8]) -> i64 {
    // Desktop preview exercises the TUI and DSP without a kernel audio sink.
    i64::from(action == 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offbeat_sample_starts_in_the_second_slot() {
        let mut engine = engine().unwrap();
        engine
            .update_with_code(r"out: speed 8 >> seq _ 60 >> sp \808ch;")
            .unwrap();
        let mut first_hit = None;
        for block in 0..12_000_usize.div_ceil(BLOCK) {
            let stereo = engine.next_block(Vec::new());
            if let Some(frame) = stereo[0].iter().position(|sample| *sample != 0.0) {
                first_hit = Some(block * BLOCK + frame);
                break;
            }
        }
        assert_eq!(first_hit, Some(6_000));
    }

    #[test]
    fn looping_points_interpolate_between_adjacent_values() {
        let mut engine = engine().unwrap();
        engine
            .update_with_code("out: [0=>0.1 1/4=>0.5 1/2=>0.2 1=>0.1]!;")
            .unwrap();
        let mut output = Vec::new();
        for _ in 0..96_001_usize.div_ceil(BLOCK) {
            output.extend_from_slice(&engine.next_block(Vec::new())[0][..]);
        }
        for (frame, expected) in [
            (0, 0.1),
            (12_000, 0.3),
            (24_000, 0.5),
            (36_000, 0.35),
            (96_000, 0.1),
        ] {
            assert!(
                (output[frame] - expected).abs() < 0.0001,
                "frame {frame}: {}",
                output[frame]
            );
        }
    }
}
