use std::f32::consts::TAU;

const HARMONIC_WEIGHTS: [f32; 14] = [
    1.0, 0.79, 0.64, 0.43, 0.26, 0.17, 0.088, 0.035, 0.031, 0.039, 0.032, 0.024, 0.015, 0.009,
];
const UNISON_PARTIALS: usize = 3;
const MAX_PARTIALS: usize = HARMONIC_WEIGHTS.len() + UNISON_PARTIALS;

pub(super) struct PianoVoice {
    tone: PianoTone,
    fading_strike: Option<FadingStrike>,
}

impl PianoVoice {
    pub(super) fn new(note: u8, velocity: u8, frequency: f32, sample_rate: f32) -> Self {
        Self {
            tone: PianoTone::new(note, velocity, frequency, sample_rate),
            fading_strike: None,
        }
    }

    pub(super) fn restrike(
        &mut self,
        note: u8,
        velocity: u8,
        frequency: f32,
        sample_rate: f32,
        previous_level: f32,
    ) {
        let tone = PianoTone::new(note, velocity, frequency, sample_rate);
        let previous = std::mem::replace(&mut self.tone, tone);
        // A hammer starts a fresh strike. Let the old string state crossfade
        // for a few milliseconds to avoid a discontinuity on repeated keys.
        self.fading_strike = Some(FadingStrike {
            tone: previous,
            level: previous_level,
            mix: 1.0,
            step: 1.0 / (sample_rate * 0.003).max(1.0),
        });
    }

    pub(super) fn sample(&mut self, level: f32) -> f32 {
        let current = self.tone.sample() * level;
        if let Some(previous) = &mut self.fading_strike {
            let sample = previous.tone.sample() * previous.level * previous.mix
                + current * (1.0 - previous.mix);
            previous.mix -= previous.step;
            if previous.mix <= 0.0 {
                self.fading_strike = None;
            }
            sample
        } else {
            current
        }
    }
}

struct FadingStrike {
    tone: PianoTone,
    level: f32,
    mix: f32,
    step: f32,
}

struct PianoTone {
    partials: [Partial; MAX_PARTIALS],
    partial_count: usize,
    hammer: Hammer,
    bloom: f32,
    bloom_decay: f32,
    remaining_frames: usize,
}

impl PianoTone {
    fn new(note: u8, velocity: u8, frequency: f32, sample_rate: f32) -> Self {
        let velocity = f32::from(velocity) / 127.0;
        let note = f32::from(note);
        let bass = ((55.0 - note) / 34.0).clamp(0.0, 1.0);
        let treble = ((note - 55.0) / 53.0).clamp(0.0, 1.0);
        let brightness = 0.54 + 0.38 * velocity + 0.12 * bass - 0.10 * treble;
        // The middle keys retain the hammer's upper harmonics. On shorter
        // treble strings, the pitch dominates and the top modes fall away.
        let middle_rolloff = 0.13 * (1.0 - (note - 60.0).abs() / 13.0).max(0.0);
        let treble_rolloff = 1.15 * ((note - 69.0) / 15.0).clamp(0.0, 2.1);
        // At low velocity, the felt makes a gentler strike and excites fewer
        // upper partials. Preserve more of the upper spectrum on bass keys.
        let touch_rolloff = 2.0 * ((note - 45.0) / 15.0).clamp(0.0, 1.0) * (1.0 - velocity).powi(2);
        let harmonic_falloff =
            brightness.powf(0.60) * (-(middle_rolloff + treble_rolloff + touch_rolloff)).exp2();
        let stiffness = 0.00022 + 0.00022 * bass * bass + 0.0018 * treble.powi(3);
        let decay_time = (7.1 * (frequency / 110.0).powf(-0.44)).clamp(0.55, 12.0);
        let tail_scale = 3.0 - 1.65 * ((note - 80.0) / 28.0).clamp(0.0, 1.0);
        let hammer_mix = 0.07 + 0.12 * velocity;
        let detune = (1.1 + 1.7 * treble) / 1200.0;
        let unison_high = 2.0f32.powf(detune * 0.76);
        let unison_low = 2.0f32.powf(-detune * 0.24);

        let mut partials = [Partial::default(); MAX_PARTIALS];
        let mut partial_count = 0;
        let mut total_weight = 0.0;
        let mut longest_decay = decay_time;
        let mut harmonic_gain = 1.0;
        for (index, weight) in HARMONIC_WEIGHTS.into_iter().enumerate() {
            let harmonic = (index + 1) as f32;
            let stretch = ((1.0 + stiffness * harmonic * harmonic) / (1.0 + stiffness)).sqrt();
            let pitch = frequency * harmonic * stretch;
            if pitch >= sample_rate * 0.45 {
                break;
            }
            let order = harmonic - 1.0;
            let weight = weight * harmonic_gain;
            harmonic_gain *= harmonic_falloff;
            let partial_decay = decay_time / (1.0 + 0.42 * order + 0.052 * order * order);
            let damping = (-6.907_755 / (partial_decay * sample_rate)).exp();
            if note >= 48.0 && index < UNISON_PARTIALS && pitch * unison_high < sample_rate * 0.45 {
                // A weaker, more slowly damped unison leaves a second tail
                // after the quick initial decay of the main string motion.
                let tail_decay = partial_decay * tail_scale;
                let tail_damping = (-6.907_755 / (tail_decay * sample_rate)).exp();
                longest_decay = longest_decay.max(tail_decay);
                partials[partial_count] = Partial::new(
                    pitch * unison_high,
                    weight * 0.24,
                    tail_damping,
                    sample_rate,
                );
                partial_count += 1;
                partials[partial_count] =
                    Partial::new(pitch * unison_low, weight * 0.76, damping, sample_rate);
            } else {
                partials[partial_count] = Partial::new(pitch, weight, damping, sample_rate);
            }
            partial_count += 1;
            total_weight += weight;
        }
        // Normalize the sum before velocity gain. The normalization keeps
        // register and timbre changes from adding an independent gain boost.
        if total_weight > 0.0 {
            for partial in &mut partials[..partial_count] {
                partial.real *= (1.0 - hammer_mix) / total_weight;
            }
        }
        Self {
            partials,
            partial_count,
            hammer: Hammer::new(note as u8, velocity, treble, hammer_mix, sample_rate),
            bloom: 0.32,
            bloom_decay: (-1.0 / (0.11 * sample_rate)).exp(),
            remaining_frames: (2.0 * longest_decay * sample_rate) as usize,
        }
    }

    fn sample(&mut self) -> f32 {
        if self.remaining_frames == 0 {
            return 0.0;
        }
        self.remaining_frames -= 1;
        if self.remaining_frames % 128 == 0 {
            // Drop inaudible modes as they decay. Quiet held notes then use
            // fewer oscillators and never spend time on subnormal floats.
            let mut active = 0;
            for i in 0..self.partial_count {
                let partial = self.partials[i];
                if partial.real.abs() + partial.imaginary.abs() > 0.000_001 {
                    self.partials[active] = partial;
                    active += 1;
                }
            }
            self.partial_count = active;
            if self.bloom < 0.000_001 {
                self.bloom = 0.0;
            }
        }
        let strings = self.partials[..self.partial_count]
            .iter_mut()
            .map(Partial::sample)
            .sum::<f32>();
        let sample = strings * (0.68 + self.bloom) + self.hammer.sample();
        self.bloom *= self.bloom_decay;
        sample
    }
}

#[derive(Clone, Copy, Default)]
struct Partial {
    real: f32,
    imaginary: f32,
    real_step: f32,
    imaginary_step: f32,
}

impl Partial {
    fn new(frequency: f32, weight: f32, damping: f32, sample_rate: f32) -> Self {
        let (sin_step, cos_step) = (TAU * frequency / sample_rate).sin_cos();
        Self {
            real: weight,
            imaginary: 0.0,
            real_step: cos_step * damping,
            imaginary_step: sin_step * damping,
        }
    }

    fn sample(&mut self) -> f32 {
        let sample = self.imaginary;
        // Rotate and decay the complex oscillator together. Trigonometry
        // and envelope setup run once per note event.
        self.imaginary = self.real * self.imaginary_step + sample * self.real_step;
        self.real = self.real * self.real_step - sample * self.imaginary_step;
        sample
    }
}

struct Hammer {
    noise_state: u32,
    level: f32,
    decay: f32,
    fast: f32,
    slow: f32,
    fast_step: f32,
    slow_step: f32,
}

impl Hammer {
    fn new(note: u8, velocity: f32, treble: f32, level: f32, sample_rate: f32) -> Self {
        let noise_state = u32::from(note).wrapping_mul(0x9e37_79b9) ^ 0xc482_f2c9;
        let upper = 1_500.0 + 3_900.0 * velocity + 1_300.0 * treble;
        Self {
            noise_state,
            level,
            decay: (-1.0 / (0.004 * sample_rate)).exp(),
            fast: 0.0,
            slow: 0.0,
            fast_step: 1.0 - (-TAU * upper / sample_rate).exp(),
            slow_step: 1.0 - (-TAU * 620.0 / sample_rate).exp(),
        }
    }

    fn sample(&mut self) -> f32 {
        if self.level < 0.000_02 {
            return 0.0;
        }
        self.noise_state ^= self.noise_state << 13;
        self.noise_state ^= self.noise_state >> 17;
        self.noise_state ^= self.noise_state << 5;
        let noise = self.noise_state as i32 as f32 / i32::MAX as f32;
        self.fast += self.fast_step * (noise - self.fast);
        self.slow += self.slow_step * (noise - self.slow);
        let sample = 0.5 * self.level * (self.fast - self.slow);
        self.level *= self.decay;
        sample
    }
}
