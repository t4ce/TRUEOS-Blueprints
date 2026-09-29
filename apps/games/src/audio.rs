//! Procedural Games soundscape, adapted from the supplied Garden Collective
//! studio's F warm-horizon palette. All clocks advance in rendered audio frames,
//! so pause and stream backpressure also pause the composition.
use trueos::audio::{self, PlaybackParams, Stream};
use trueos::logl::{self, level};

const RATE: u64 = 48_000;
const RATE_F: f32 = RATE as f32;
const BLOCK_FRAMES: usize = 960;
const QUEUE_FRAMES: usize = BLOCK_FRAMES * 4;
const RETRY_MS: u64 = 5_000;
const TABLE_SIZE: usize = 1024;
const TAU: f32 = core::f32::consts::TAU;
const F_ROOT: i32 = 41;
const CHORDS: [[i32; 5]; 4] = [
    [0, 7, 12, 16, 19],
    [5, 12, 16, 19, 23],
    [2, 9, 12, 16, 21],
    [7, 14, 17, 21, 24],
];
const BELLS: [i32; 5] = [19, 21, 24, 26, 28];

pub struct Audio {
    soundscape: Option<Soundscape>,
    next_retry: u64,
    error_logged: bool,
}

impl Audio {
    pub const fn new() -> Self {
        Self {
            soundscape: None,
            next_retry: 0,
            error_logged: false,
        }
    }

    pub fn update(&mut self, playing: bool, now_ms: u64) {
        if self.soundscape.is_none() && playing && now_ms >= self.next_retry {
            match Soundscape::open(now_ms as u32) {
                Ok(soundscape) => {
                    self.soundscape = Some(soundscape);
                    self.error_logged = false;
                }
                Err(code) => {
                    if !self.error_logged {
                        logl::log(
                            level::INFO,
                            format_args!("Games: audio unavailable ({code}); retrying"),
                        );
                        self.error_logged = true;
                    }
                    self.next_retry = now_ms.saturating_add(RETRY_MS);
                }
            }
        }
        if let Some(soundscape) = self.soundscape.as_mut() {
            if let Err(code) = soundscape.update(playing) {
                logl::log(
                    level::ERROR,
                    format_args!("Games: audio stream failed ({code})"),
                );
                self.soundscape = None;
                self.next_retry = now_ms.saturating_add(RETRY_MS);
            }
        }
    }
}

struct Soundscape {
    stream: Stream,
    samples: [i16; BLOCK_FRAMES * 2],
    cursor: usize,
    active: bool,
    sine: [f32; TABLE_SIZE],
    frame: u64,
    left_phase: f32,
    right_phase: f32,
    pad_phase: [f32; 5],
    pad_triangle_phase: [f32; 5],
    pad_breath_phase: [f32; 5],
    pad_drift_phase: [f32; 5],
    pad_hz: [f32; 5],
    pad_target: [f32; 5],
    percent_gain: [f32; 101],
    chord_index: usize,
    next_chord: u64,
    ocean_low: [f32; 2],
    ocean_dc: [f32; 2],
    random: u32,
    chime: Chime,
    next_chime: u64,
    chime_count: u64,
}

#[derive(Default)]
struct Chime {
    age: u64,
    hz: f32,
    phase: f32,
    partial_phase: f32,
    gain: f32,
    envelope: f32,
    pan: f32,
    active: bool,
}

impl Soundscape {
    fn open(seed: u32) -> Result<Self, i32> {
        let stream = Stream::open_playback(PlaybackParams::s16le_stereo_48k())?;
        let mut sine = [0.; TABLE_SIZE];
        for (i, sample) in sine.iter_mut().enumerate() {
            *sample = libm::sinf(TAU * i as f32 / TABLE_SIZE as f32);
        }
        let mut pad_hz = [0.; 5];
        for (i, hz) in pad_hz.iter_mut().enumerate() {
            *hz = midi_hz(F_ROOT + CHORDS[0][i]);
        }
        let mut percent_gain = [0.; 101];
        for (percent, gain) in percent_gain.iter_mut().enumerate() {
            *gain = libm::powf(percent as f32 / 100., 1.25);
        }
        let mut soundscape = Self {
            stream,
            samples: [0; BLOCK_FRAMES * 2],
            cursor: BLOCK_FRAMES * 2,
            active: true,
            sine,
            frame: 0,
            left_phase: 0.,
            right_phase: 0.,
            pad_phase: [0.; 5],
            pad_triangle_phase: [0.; 5],
            pad_breath_phase: [0.; 5],
            pad_drift_phase: [0.; 5],
            pad_hz,
            pad_target: pad_hz,
            percent_gain,
            chord_index: 0,
            next_chord: (47.5 * RATE_F) as u64,
            ocean_low: [0.; 2],
            ocean_dc: [0.; 2],
            random: (seed ^ 0xA853_A923).max(1),
            chime: Chime::default(),
            next_chime: (2.2 * RATE_F) as u64,
            chime_count: 0,
        };
        soundscape.feed()?;
        stream.start()?;
        Ok(soundscape)
    }

    fn update(&mut self, playing: bool) -> Result<(), i32> {
        if playing != self.active {
            self.stream.set_paused(!playing)?;
            self.active = playing;
        }
        if playing {
            self.feed()?;
        }
        Ok(())
    }

    fn feed(&mut self) -> Result<(), i32> {
        for _ in 0..4 {
            if self.stream.queued_frames()? >= QUEUE_FRAMES {
                break;
            }
            if self.cursor == self.samples.len() {
                self.fill_block();
            }
            match self
                .stream
                .write_interleaved_i16(&self.samples[self.cursor..])
            {
                Ok(0) | Err(audio::ERR_BUSY) => break,
                Ok(frames) => {
                    let written = frames.checked_mul(2).ok_or(audio::ERR_IO)?;
                    if written > self.samples.len() - self.cursor {
                        return Err(audio::ERR_IO);
                    }
                    self.cursor += written;
                }
                Err(code) => return Err(code),
            }
        }
        Ok(())
    }

    fn fill_block(&mut self) {
        for i in 0..BLOCK_FRAMES {
            let [left, right] = self.next_frame();
            self.samples[2 * i] = pcm(left);
            self.samples[2 * i + 1] = pcm(right);
        }
        self.cursor = 0;
    }

    fn next_frame(&mut self) -> [f32; 2] {
        let seconds = self.frame as f32 / RATE_F;
        let carrier = 75. + 50. * triangle_cycle(seconds, 30.);
        let beat = match (self.frame / (180 * RATE)) % 3 {
            0 => 2.,
            1 => 6.,
            _ => 10.,
        };
        let tone_gain = 0.22; // 100% studio tone setting, before master and peak scale.
        let mut out = [
            sine_at(&self.sine, &mut self.left_phase, carrier - beat * 0.5) * tone_gain,
            sine_at(&self.sine, &mut self.right_phase, carrier + beat * 0.5) * tone_gain,
        ];

        if self.frame >= self.next_chord {
            self.chord_index = (self.chord_index + 1) % CHORDS.len();
            self.next_chord += (47.5 * RATE_F) as u64; // 15% drift and evolution.
            for i in 0..5 {
                self.pad_target[i] = midi_hz(F_ROOT + CHORDS[self.chord_index][i]);
            }
        }
        let pad_percent = 70. + 15. * triangle_cycle(seconds, 60.);
        let pad_level = 1.08 * percent_gain(&self.percent_gain, pad_percent);
        // Warmth 95% softens the pad's upper partials; motion 15% keeps it slow.
        let warmth = 0.95;
        let pad_top = 1. - 0.5 * warmth;
        for i in 0..5 {
            self.pad_hz[i] += (self.pad_target[i] - self.pad_hz[i]) / (8. * RATE_F);
            let drift = sine_at(
                &self.sine,
                &mut self.pad_drift_phase[i],
                0.02 + i as f32 * 0.004,
            ) * 0.0015;
            let hz = self.pad_hz[i] * (1. + drift);
            let sine = sine_at(&self.sine, &mut self.pad_phase[i], hz);
            let tri = triangle_at(&mut self.pad_triangle_phase[i], hz);
            let breath = sine_at(
                &self.sine,
                &mut self.pad_breath_phase[i],
                0.016 + i as f32 * 0.003,
            );
            let voice = (sine * 0.109 + tri * 0.027 * pad_top) * (0.85 + breath * 0.02) * pad_level;
            let pan = [-0.7, -0.35, 0., 0.35, 0.7][i];
            out[0] += voice * (1. - pan) * 0.5;
            out[1] += voice * (1. + pan) * 0.5;
        }

        let ocean_time = seconds % 20.;
        let ocean_percent = if ocean_time < 10. {
            25. + 40. * ocean_time / 10.
        } else if ocean_time < 15. {
            65.
        } else {
            65. - 40. * (ocean_time - 15.) / 5.
        };
        let ocean_level = 1.2 * percent_gain(&self.percent_gain, ocean_percent);
        // Two independent, filtered noise channels form the ocean hush.
        let lowpass = 0.059; // Approximately 474 Hz at 48 kHz (warmth 95%).
        let highpass = 0.0084; // Approximately 65 Hz.
        for channel in 0..2 {
            let white = self.random_unit();
            self.ocean_low[channel] += lowpass * (white - self.ocean_low[channel]);
            self.ocean_dc[channel] += highpass * (self.ocean_low[channel] - self.ocean_dc[channel]);
            out[channel] += (self.ocean_low[channel] - self.ocean_dc[channel]) * 0.63 * ocean_level;
        }

        if !self.chime.active && self.frame >= self.next_chime {
            self.start_chime();
        }
        if self.chime.active {
            let age = self.chime.age as f32 / RATE_F;
            let envelope = if age < 0.09 {
                age / 0.09
            } else {
                self.chime.envelope *= 0.999_978_1;
                self.chime.envelope
            };
            let bell = (sine_at(&self.sine, &mut self.chime.phase, self.chime.hz)
                + 0.15
                    * sine_at(
                        &self.sine,
                        &mut self.chime.partial_phase,
                        self.chime.hz * 2.,
                    ))
                * envelope
                * self.chime.gain;
            out[0] += bell * (1. - self.chime.pan) * 0.5;
            out[1] += bell * (1. + self.chime.pan) * 0.5;
            self.chime.age += 1;
            if self.chime.age >= 7 * RATE {
                self.chime.active = false;
                // A just-finished chime gets one more note one third of the time.
                let repeat = self.random_unit() < -1. / 3.;
                let delay = if repeat {
                    RATE
                } else {
                    (RATE_F * (8.4 + self.random_01() * 8.4)) as u64
                };
                self.next_chime = self.frame + delay;
            }
        }
        self.frame += 1;
        // Studio master at 100%, then its fixed 0.25 peak scale. PCM is bounded.
        [out[0] * 0.205, out[1] * 0.205]
    }

    fn start_chime(&mut self) {
        let note = BELLS[(self.random_01() * BELLS.len() as f32) as usize];
        let tier = [0.6, 0.7, 0.8][((self.chime_count / 3) % 3) as usize];
        self.chime_count += 1;
        self.chime = Chime {
            age: 0,
            hz: midi_hz(F_ROOT + note),
            phase: 0.,
            partial_phase: 0.,
            gain: 0.09 * 1.05 * self.percent_gain[75] * tier,
            envelope: 1.,
            pan: self.random_unit() * 0.65,
            active: true,
        };
    }

    fn random_01(&mut self) -> f32 {
        self.random ^= self.random << 13;
        self.random ^= self.random >> 17;
        self.random ^= self.random << 5;
        ((self.random >> 8) as f32) * (1. / 16_777_216.)
    }

    fn random_unit(&mut self) -> f32 {
        self.random_01() * 2. - 1.
    }
}

impl Drop for Soundscape {
    fn drop(&mut self) {
        let _ = self.stream.drop_stream();
        let _ = self.stream.close();
    }
}

fn percent_gain(table: &[f32; 101], percent: f32) -> f32 {
    let lower = (percent as usize).min(99);
    table[lower] + (table[lower + 1] - table[lower]) * (percent - lower as f32)
}

fn midi_hz(note: i32) -> f32 {
    440. * libm::powf(2., (note as f32 - 69.) / 12.)
}

fn triangle_cycle(seconds: f32, period: f32) -> f32 {
    let fraction = (seconds / period) % 1.;
    1. - (2. * fraction - 1.).abs()
}

fn sine_at(table: &[f32; TABLE_SIZE], phase: &mut f32, hz: f32) -> f32 {
    let position = *phase * TABLE_SIZE as f32;
    let index = position as usize % TABLE_SIZE;
    let next = (index + 1) % TABLE_SIZE;
    let fraction = position - position as usize as f32;
    let value = table[index] + (table[next] - table[index]) * fraction;
    *phase += hz / RATE_F;
    if *phase >= 1. {
        *phase -= 1.;
    }
    value
}

fn triangle_at(phase: &mut f32, hz: f32) -> f32 {
    let value = 1. - 4. * (*phase - 0.5).abs();
    *phase += hz / RATE_F;
    if *phase >= 1. {
        *phase -= 1.;
    }
    value
}

fn pcm(sample: f32) -> i16 {
    (sample.clamp(-0.94, 0.94) * i16::MAX as f32) as i16
}
