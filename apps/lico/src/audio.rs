use crate::examples::PATCHES;
use crate::midi::{Demo as MidiDemo, Input as MidiInput, Source as MidiSource};
use glicol::{Engine, MidiEvent, SequencePlayhead};
use glicol_parser::SourceMap;
use std::error::Error;
use std::io::{self, Cursor};
use std::sync::LazyLock;
use std::time::Duration;
use tokio::sync::{
    mpsc::{Receiver, Sender},
    oneshot, watch,
};

pub(crate) const BLOCK: usize = 128;
pub const SAMPLE_RATE: u32 = 48_000;
const BATCH_FRAMES: usize = 4 * BLOCK;
const BATCH_BYTES: usize = 4 * BATCH_FRAMES;
const STREAM_LIMIT: usize = SAMPLE_RATE as usize * 4 * 120;
pub const MONITOR_FRAMES: usize = BATCH_FRAMES;
pub const DEFAULT_BPM: u16 = 120;
const SAMPLES: &[(&str, &[u8])] = &[
    (r"\808bd", include_bytes!("../assets/808bd.wav")),
    (r"\808sd", include_bytes!("../assets/808sd.wav")),
    (r"\808ch", include_bytes!("../assets/808ch.wav")),
    (r"\blip", include_bytes!("../assets/blip.wav")),
    (
        r"\kit_kick",
        include_bytes!("../assets/acoustic-kit/kick.wav"),
    ),
    (
        r"\kit_snare",
        include_bytes!("../assets/acoustic-kit/snare.wav"),
    ),
    (
        r"\kit_tom_high",
        include_bytes!("../assets/acoustic-kit/tom_high.wav"),
    ),
    (
        r"\kit_tom_mid",
        include_bytes!("../assets/acoustic-kit/tom_mid.wav"),
    ),
    (
        r"\kit_tom_floor",
        include_bytes!("../assets/acoustic-kit/tom_floor.wav"),
    ),
    (
        r"\kit_hat_closed",
        include_bytes!("../assets/acoustic-kit/hat_closed.wav"),
    ),
    (
        r"\kit_hat_pedal",
        include_bytes!("../assets/acoustic-kit/hat_pedal.wav"),
    ),
    (
        r"\kit_hat_open",
        include_bytes!("../assets/acoustic-kit/hat_open.wav"),
    ),
    (
        r"\kit_crash",
        include_bytes!("../assets/acoustic-kit/crash.wav"),
    ),
    (
        r"\kit_ride",
        include_bytes!("../assets/acoustic-kit/ride.wav"),
    ),
];

pub fn sample_names() -> impl Iterator<Item = &'static str> {
    SAMPLES.iter().map(|(name, _)| *name)
}

pub enum Command {
    Run(String),
    Restart {
        code: String,
        finished: oneshot::Sender<Result<Option<String>, String>>,
    },
    ChangeTempo(i16),
    SelectMidiInput(MidiSource),
    TestMidi,
    Stop,
}

pub enum Status {
    Ready {
        rate: u32,
        midi_source: MidiSource,
    },
    Tempo(u16),
    PatchParsed {
        run: u64,
        code: String,
        source: SourceMap,
    },
    Playing(String),
    Midi(String),
    MidiInput {
        source: MidiSource,
        error: Option<String>,
    },
    Stopped,
    PatchError(String),
    AudioError(String),
}

#[derive(Clone)]
pub struct MonitorFrame {
    pub run: u64,
    pub waveform: [f32; MONITOR_FRAMES],
    pub sequences: Vec<Option<SequencePlayhead>>,
    pub playing: bool,
    pub beat_phase: f32,
}

impl Default for MonitorFrame {
    fn default() -> Self {
        Self {
            run: 0,
            waveform: [0.0; MONITOR_FRAMES],
            sequences: Vec::new(),
            playing: false,
            beat_phase: 0.0,
        }
    }
}

impl MonitorFrame {
    fn stop(&mut self) {
        self.playing = false;
        self.waveform.fill(0.0);
        self.sequences.clear();
        self.beat_phase = 0.0;
    }
}

struct DecodedSample {
    name: &'static str,
    planar: Vec<f32>,
    channels: usize,
    sample_rate: usize,
}

impl DecodedSample {
    fn read(name: &'static str, wav: &[u8]) -> Result<Self, hound::Error> {
        let mut reader = hound::WavReader::new(Cursor::new(wav))?;
        let spec = reader.spec();
        let channels = usize::from(spec.channels);
        let frames = reader.duration() as usize;
        let mut planar = vec![0.0_f32; frames * channels];
        for (index, sample) in reader.samples::<i16>().enumerate() {
            let offset = (index % channels) * frames + index / channels;
            planar[offset] = f32::from(sample?) / 32768.0;
        }
        Ok(Self {
            name,
            planar,
            channels,
            sample_rate: spec.sample_rate as usize,
        })
    }
}

pub(crate) fn engine() -> io::Result<Engine<BLOCK>> {
    static DECODED_SAMPLES: LazyLock<Result<Vec<DecodedSample>, hound::Error>> =
        LazyLock::new(|| {
            SAMPLES
                .iter()
                .map(|&(name, wav)| DecodedSample::read(name, wav))
                .collect()
        });

    let samples = DECODED_SAMPLES
        .as_ref()
        .map_err(|error| io::Error::other(error.to_string()))?;
    let mut engine = Engine::new();
    engine.set_sr(SAMPLE_RATE as usize);
    for sample in samples {
        engine.add_sample(
            sample.name,
            &sample.planar,
            sample.channels,
            sample.sample_rate,
        );
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

fn feed_midi_demo(engine: &mut Engine<BLOCK>, demo: &mut Option<MidiDemo>) {
    if let Some(source) = demo
        && !source.advance(SAMPLE_RATE, BATCH_FRAMES, |event| engine.send_midi(event))
    {
        *demo = None;
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
        let mut midi_demo = engine.has_midi_input().then(MidiDemo::default);
        let batches = (8 * SAMPLE_RATE as usize).div_ceil(BATCH_FRAMES);
        let mut energy = 0.0;
        let mut peak = 0.0_f64;
        for _ in 0..batches {
            feed_midi_demo(&mut engine, &mut midi_demo);
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
    monitor: watch::Sender<MonitorFrame>,
    mut midi_input: MidiInput,
) -> Result<(), Box<dyn Error>> {
    let mut engine = engine()?;
    let mut playing = None;
    let mut run = 0_u64;
    let mut sequences = Vec::new();
    let mut bpm = DEFAULT_BPM;
    let mut beat_phase = 0.0_f64;
    let mut stream = None;
    let mut midi_demo = None;
    let mut midi_playing = false;
    // The kernel validates append buffers against the VM heap.
    let mut pcm = vec![0; BATCH_BYTES];
    let mut waveform = [0.; MONITOR_FRAMES];
    let mut ticks = tokio::time::interval(Duration::from_nanos(
        1_000_000_000 * BATCH_FRAMES as u64 / u64::from(SAMPLE_RATE),
    ));
    ticks.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let _ = status.try_send(Status::Ready {
        rate: SAMPLE_RATE,
        midi_source: midi_input.source(),
    });

    loop {
        tokio::select! {
            command = commands.recv() => match command {
                Some(command @ (Command::Run(_) | Command::Restart { .. })) => {
                    let (code, finished) = match command {
                        Command::Run(code) => (code, None),
                        Command::Restart { code, finished } => (code, Some(finished)),
                        _ => unreachable!(),
                    };
                    let restarting = finished.is_some();
                    let complete = |result| {
                        if let Some(finished) = finished {
                            let _ = finished.send(result);
                        }
                    };
                    // Build the replacement graph while the current stream is still active.
                    let mut replacement = restarting.then(|| {
                        let mut fresh = Engine::new();
                        fresh.samples_dict.clone_from(&engine.samples_dict);
                        fresh.set_sr(SAMPLE_RATE as usize);
                        fresh.set_bpm(f32::from(bpm));
                        fresh
                    });
                    let target = replacement.as_mut().unwrap_or(&mut engine);
                    let source = match target.update_with_code_and_source(&code) {
                        Ok(source) => source,
                        Err(error) => {
                            let message = error.to_string();
                            let _ = status.try_send(Status::PatchError(message.clone()));
                            complete(Err(message));
                            continue;
                        }
                    };
                    if let Some(fresh) = replacement {
                        engine = fresh;
                        stream = None;
                        midi_demo = None;
                        midi_input.reset();
                    } else if midi_demo.take().is_some() {
                        engine.release_midi_notes();
                    }
                    run = run.wrapping_add(1);
                    sequences.clear();
                    sequences.extend(source.sequences.iter().map(|seq| {
                        engine.sequence_node(&seq.chain, seq.node)
                    }));
                    let _ = status.try_send(Status::PatchParsed {
                        run,
                        code: code.clone(),
                        source,
                    });
                    if playing.is_none() || restarting {
                        match Stream::open() {
                            Ok(output) => stream = Some(output),
                            // The previous stream's tail can still be draining at rollover.
                            Err(error) if playing.is_some() && error.kind() == io::ErrorKind::WouldBlock => {},
                            Err(error) => {
                                playing = None;
                                midi_playing = false;
                                monitor.send_modify(MonitorFrame::stop);
                                let message = error.to_string();
                                let _ = status.try_send(Status::AudioError(message.clone()));
                                complete(Err(message));
                                continue;
                            }
                        }
                        beat_phase = 0.0;
                        ticks.reset();
                    }
                    midi_playing = engine.has_midi_input();
                    if !midi_playing {
                        midi_input.reset();
                    } else if midi_input.source() != MidiSource::Off {
                        let label = midi_input.source().label();
                        let _ = status.try_send(Status::Midi(format!("MIDI input · {label}")));
                    }
                    let previous = playing.replace(code.clone());
                    let _ = status.try_send(Status::Playing(code));
                    complete(Ok(previous));
                }
                Some(Command::ChangeTempo(delta)) => {
                    bpm = bpm.saturating_add_signed(delta).clamp(20, 400);
                    engine.set_bpm(f32::from(bpm));
                    let _ = status.try_send(Status::Tempo(bpm));
                }
                Some(Command::SelectMidiInput(source)) => {
                    let result = if source == midi_input.source() {
                        Ok(())
                    } else {
                        MidiInput::open(source).map(|input| {
                            midi_input = input;
                            midi_demo = None;
                            engine.release_midi_notes();
                        })
                    };
                    let _ = status.try_send(Status::MidiInput {
                        source: midi_input.source(),
                        error: result.err().map(|error| error.to_string()),
                    });
                }
                Some(Command::TestMidi) => {
                    let message = if midi_playing {
                        engine.release_midi_notes();
                        midi_demo = Some(MidiDemo::default());
                        "MIDI test phrase · channel 1"
                    } else {
                        "Run a patch with midisynth first. The test uses channel 1."
                    };
                    let _ = status.try_send(Status::Midi(message.into()));
                }
                Some(Command::Stop) => {
                    playing = None;
                    stream = None;
                    midi_demo = None;
                    midi_playing = false;
                    midi_input.reset();
                    sequences.clear();
                    let samples = std::mem::take(&mut engine.samples_dict);
                    engine = Engine::new();
                    engine.samples_dict = samples;
                    engine.set_sr(SAMPLE_RATE as usize);
                    engine.set_bpm(f32::from(bpm));
                    monitor.send_modify(MonitorFrame::stop);
                    let _ = status.try_send(Status::Stopped);
                }
                None => return Ok(()),
            },
            _ = ticks.tick(), if playing.is_some() || midi_input.needs_idle_poll() => {
                let received = midi_input.poll(midi_playing, |event| {
                    if midi_demo.is_some() {
                        // A physical key press takes over from the test phrase.
                        if !matches!(event, MidiEvent::NoteOn { .. }) {
                            return;
                        }
                        midi_demo = None;
                        engine.release_midi_notes();
                    }
                    engine.send_midi(event);
                });
                if let Err(error) = received {
                    if midi_demo.is_none() {
                        engine.release_midi_notes();
                    }
                    midi_input = MidiInput::Disabled;
                    let _ = status.try_send(Status::MidiInput {
                        source: midi_input.source(),
                        error: Some(format!("MIDI input stopped: {error}")),
                    });
                }
                if playing.is_none() {
                    continue;
                }
                match tick(&mut stream, &mut engine, &mut pcm, &mut waveform, &mut midi_demo) {
                    Ok(true) => {
                        beat_phase = (beat_phase
                            + BATCH_FRAMES as f64 * f64::from(bpm) / (60.0 * f64::from(SAMPLE_RATE)))
                            .fract();
                        monitor.send_modify(|frame| {
                            frame.run = run;
                            frame.waveform = waveform;
                            frame.playing = true;
                            frame.beat_phase = beat_phase as f32;
                            frame.sequences.clear();
                            frame.sequences.extend(sequences.iter().map(|node| {
                                node.and_then(|node| engine.sequence_playhead(node))
                            }));
                        });
                    }
                    Ok(false) => {},
                    Err(error) => {
                        stream = None;
                        playing = None;
                        midi_demo = None;
                        midi_playing = false;
                        midi_input.reset();
                        engine.release_midi_notes();
                        monitor.send_modify(MonitorFrame::stop);
                        let _ = status.try_send(Status::AudioError(error.to_string()));
                    }
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
    midi_demo: &mut Option<MidiDemo>,
) -> io::Result<bool> {
    if stream.is_none() {
        match Stream::open() {
            Ok(output) => *stream = Some(output),
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => return Ok(false),
            Err(error) => return Err(error),
        }
    }
    let output = stream.as_mut().expect("stream opened");
    feed_midi_demo(engine, midi_demo);
    render(engine, pcm, waveform);
    output.append(pcm)?;
    if output.written == STREAM_LIMIT {
        output.finish()?;
        *stream = None;
    }
    Ok(true)
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

    #[tokio::test]
    #[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
    async fn restart_keeps_the_live_patch_on_error_and_resets_the_beat_on_success() {
        let (commands, rx) = tokio::sync::mpsc::channel(8);
        let (status, _updates) = tokio::sync::mpsc::channel(16);
        let (monitor, mut frames) = watch::channel(MonitorFrame::default());
        tokio::time::timeout(Duration::from_secs(2), async {
            let (result, ()) =
                tokio::join!(serve(rx, status, monitor, MidiInput::Disabled), async {
                    let restart = async |code: &str| {
                        let (finished, result) = oneshot::channel();
                        commands
                            .send(Command::Restart {
                                code: code.into(),
                                finished,
                            })
                            .await
                            .unwrap();
                        result.await.unwrap()
                    };
                    commands.send(Command::ChangeTempo(30)).await.unwrap();
                    commands
                        .send(Command::Run("out: constsig 0.125".into()))
                        .await
                        .unwrap();
                    while frames.borrow().beat_phase < 0.1 {
                        frames.changed().await.unwrap();
                    }
                    let previous_run = frames.borrow().run;

                    assert!(restart("out: ~missing").await.is_err());
                    frames.changed().await.unwrap();
                    assert_eq!(frames.borrow().run, previous_run);
                    assert_eq!(frames.borrow().waveform, [0.125; MONITOR_FRAMES]);

                    assert_eq!(
                        restart("out: constsig 0.25").await.unwrap().as_deref(),
                        Some("out: constsig 0.125")
                    );
                    while frames.borrow().run == previous_run {
                        frames.changed().await.unwrap();
                    }
                    let frame = frames.borrow();
                    assert!(frame.playing);
                    assert_eq!(frame.waveform, [0.25; MONITOR_FRAMES]);
                    let first_beat = BATCH_FRAMES as f32 * 150.0 / (60.0 * SAMPLE_RATE as f32);
                    assert!((frame.beat_phase - first_beat).abs() < f32::EPSILON);
                    drop(frame);

                    commands.send(Command::Stop).await.unwrap();
                    assert!(restart("out: constsig 0.25").await.unwrap().is_none());
                    drop(commands);
                });
            result.unwrap();
        })
        .await
        .expect("the audio worker should acknowledge both restarts and advance playback");
    }

    #[tokio::test]
    #[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
    async fn wifi_piano_packets_play_release_and_switch_off() {
        use std::net::{Ipv4Addr, UdpSocket};

        async fn wait_for_sound(frames: &mut watch::Receiver<MonitorFrame>, sounding: bool) {
            loop {
                frames.changed().await.unwrap();
                let waveform = &frames.borrow().waveform;
                if if sounding {
                    waveform.iter().any(|sample| sample.abs() > 0.01)
                } else {
                    *waveform == [0.0; MONITOR_FRAMES]
                } {
                    break;
                }
            }
        }

        let socket = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        socket.set_nonblocking(true).unwrap();
        let address = socket.local_addr().unwrap();
        let sender = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        sender.connect(address).unwrap();

        let (commands, rx) = tokio::sync::mpsc::channel(8);
        let (status, mut updates) = tokio::sync::mpsc::channel(8);
        let (monitor, mut frames) = watch::channel(MonitorFrame::default());
        let input = MidiInput::Wifi {
            socket,
            piano: crate::piano_udp::Piano::default(),
        };
        tokio::time::timeout(Duration::from_secs(2), async {
            let (result, ()) = tokio::join!(serve(rx, status, monitor, input), async move {
                let patch = include_str!("../assets/examples/midi.glicol");
                commands.send(Command::Run(patch.into())).await.unwrap();
                while !matches!(updates.recv().await.unwrap(), Status::Playing(_)) {}
                frames.changed().await.unwrap();
                assert_eq!(frames.borrow().waveform, [0.0; MONITOR_FRAMES]);

                sender.send(b"piano seq=1 mask=0x1000000").unwrap();
                wait_for_sound(&mut frames, true).await;

                sender.send(b"piano seq=2 mask=0").unwrap();
                wait_for_sound(&mut frames, false).await;

                sender.send(b"piano seq=3 mask=0x1000000").unwrap();
                wait_for_sound(&mut frames, true).await;
                commands
                    .send(Command::SelectMidiInput(MidiSource::Off))
                    .await
                    .unwrap();
                while !matches!(
                    updates.recv().await.unwrap(),
                    Status::MidiInput {
                        source: MidiSource::Off,
                        error: None,
                    }
                ) {}
                let _rebound = UdpSocket::bind(address).unwrap();
                wait_for_sound(&mut frames, false).await;
                drop(commands);
            });
            result.unwrap();
        })
        .await
        .expect("Wi-Fi MIDI should start and release a live note");
    }

    #[test]
    #[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
    fn raw_midi_demo_renders_and_releases_through_the_live_path() {
        let mut engine = engine().unwrap();
        engine
            .update_with_code(include_str!("../assets/examples/midi.glicol"))
            .unwrap();
        let mut pcm = [0; BATCH_BYTES];
        let mut waveform = [0.0; MONITOR_FRAMES];
        let mut stream = None;
        let mut midi_demo = None;

        tick(
            &mut stream,
            &mut engine,
            &mut pcm,
            &mut waveform,
            &mut midi_demo,
        )
        .unwrap();
        assert!(pcm.iter().all(|byte| *byte == 0));
        midi_demo = Some(MidiDemo::default());
        tick(
            &mut stream,
            &mut engine,
            &mut pcm,
            &mut waveform,
            &mut midi_demo,
        )
        .unwrap();
        assert!(pcm.iter().any(|byte| *byte != 0));
        for _ in 0..(2 * SAMPLE_RATE as usize).div_ceil(BATCH_FRAMES) {
            tick(
                &mut stream,
                &mut engine,
                &mut pcm,
                &mut waveform,
                &mut midi_demo,
            )
            .unwrap();
        }
        assert!(midi_demo.is_none());
        assert!(pcm.iter().all(|byte| *byte == 0));
    }

    #[test]
    fn synthesized_piano_decays_and_releases_without_samples() {
        let mut engine = Engine::<BLOCK>::new();
        engine.set_sr(SAMPLE_RATE as usize);
        engine
            .update_with_code(include_str!("../assets/examples/piano.glicol"))
            .unwrap();
        assert!(engine.has_midi_input());

        let rms = |engine: &mut Engine<BLOCK>, blocks: usize| {
            let energy: f32 = (0..blocks)
                .map(|_| {
                    engine.next_block(Vec::new())[0]
                        .iter()
                        .map(|sample| sample * sample)
                        .sum::<f32>()
                })
                .sum();
            (energy / (BLOCK * blocks) as f32).sqrt()
        };
        assert_eq!(rms(&mut engine, 1), 0.0);
        engine.send_midi(MidiEvent::NoteOn {
            channel: 4,
            note: 69,
            velocity: 110,
        });
        let attack = rms(&mut engine, 20);
        assert!(attack > 0.01);
        rms(&mut engine, 375);
        let held = rms(&mut engine, 20);
        assert!(held > 0.0001 && held < attack * 0.8);
        engine.send_midi(MidiEvent::NoteOff {
            channel: 4,
            note: 69,
        });
        assert!(rms(&mut engine, 1) > 0.0);
        rms(&mut engine, 75);
        assert_eq!(rms(&mut engine, 1), 0.0);
    }

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
