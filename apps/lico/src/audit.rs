use crate::audio::{BLOCK, SAMPLE_RATE, engine};
use rustfft::{Fft, FftPlanner, num_complex::Complex32};
use serde_json::{Value, json};
use std::sync::Arc;
use std::time::{Duration, Instant};

const FFT_SIZE: usize = 2048;
const MAX_GRID_STEPS: usize = 64;
const RENDER_BUDGET: Duration = Duration::from_secs(5);

pub struct Window {
    start: usize,
    end: usize,
    step: usize,
    beats_per_step: f64,
}

impl Window {
    pub fn new(start_seconds: f64, seconds: f64, bpm: u16) -> Result<Self, String> {
        if !start_seconds.is_finite()
            || start_seconds < 0.0
            || !(0.25..=16.0).contains(&seconds)
            || start_seconds + seconds > 120.0
        {
            return Err("audit needs a 0.25–16 second window within the first 120 seconds".into());
        }
        let frames_per_second = f64::from(SAMPLE_RATE);
        let start = (start_seconds * frames_per_second).round() as usize;
        let end = ((start_seconds + seconds) * frames_per_second).round() as usize;
        let frames_per_beat = 60.0 * frames_per_second / f64::from(bpm);
        let mut beats_per_step = 0.25;
        let step = loop {
            let frames = (frames_per_beat * beats_per_step).round() as usize;
            if (end - start).div_ceil(frames) <= MAX_GRID_STEPS {
                break frames;
            }
            beats_per_step *= 2.0;
        };
        Ok(Self {
            start,
            end,
            step,
            beats_per_step,
        })
    }

    fn grid_steps(&self) -> usize {
        (self.end - self.start).div_ceil(self.step)
    }
}

pub fn compare(
    candidate: &str,
    baseline: &str,
    baseline_source: &str,
    bpm: u16,
    window: Window,
    focus_chain: Option<&str>,
) -> Result<Value, String> {
    let deadline = Instant::now() + RENDER_BUDGET;
    let candidate_report = render(candidate, bpm, &window, focus_chain, deadline)?;
    let baseline_report = if candidate == baseline {
        candidate_report.clone()
    } else {
        match render(baseline, bpm, &window, focus_chain, deadline) {
            Ok(report) => report,
            Err(error) => json!({"error": error}),
        }
    };
    let change = |section: &str, field: &str| {
        let candidate = candidate_report[section][field].as_f64()?;
        let baseline = baseline_report[section][field].as_f64()?;
        Some(round_tenth(candidate - baseline))
    };
    Ok(json!({
        "bpm": bpm,
        "start_seconds": window.start as f64 / f64::from(SAMPLE_RATE),
        "seconds": (window.end - window.start) as f64 / f64::from(SAMPLE_RATE),
        "grid": {"beats_per_step": window.beats_per_step, "steps": window.grid_steps()},
        "external_midi_events": 0,
        "focus_chain": focus_chain,
        "baseline_source": baseline_source,
        "baseline": baseline_report,
        "candidate": candidate_report,
        "candidate_minus_baseline": {
            "mix_peak_db": change("mix", "peak_dbfs"),
            "mix_rms_db": change("mix", "rms_dbfs"),
            "focus_rms_db": change("focus", "rms_dbfs"),
            "spectrum_energy_percentage_points": {
                "below_150_hz": change("spectrum_energy_percent", "below_150_hz"),
                "150_to_2000_hz": change("spectrum_energy_percent", "150_to_2000_hz"),
                "above_2000_hz": change("spectrum_energy_percent", "above_2000_hz")
            }
        }
    }))
}

fn render(
    code: &str,
    bpm: u16,
    window: &Window,
    focus_chain: Option<&str>,
    deadline: Instant,
) -> Result<Value, String> {
    let mut engine = engine().map_err(|error| error.to_string())?;
    engine.set_bpm(f32::from(bpm));
    engine
        .update_with_code(code)
        .map_err(|error| error.to_string())?;
    let focus = focus_chain.and_then(|chain| {
        // Glicol stores the displayed drum alias under its original name.
        let chain = if chain == "~🥁" { "~drum" } else { chain };
        engine.index_info.get(chain)?.last().copied()
    });
    let mut focus_levels = focus.map(|_| Levels::new(window.grid_steps()));
    let mut mix = Levels::new(window.grid_steps());
    let mut spectrum = Spectrum::new();

    for (block, first_frame) in (0..window.end).step_by(BLOCK).enumerate() {
        if block % 64 == 0 && Instant::now() >= deadline {
            return Err(
                "audit reached its render time limit; try an earlier or shorter window".into(),
            );
        }
        let stereo = engine.next_block(Vec::new());
        let start = first_frame.max(window.start);
        let end = (first_frame + BLOCK).min(window.end);
        for frame in start..end {
            let step = (frame - window.start) / window.step;
            let offset = frame - first_frame;
            let left = mix.add(stereo[0][offset], step);
            let right = mix.add(stereo[1][offset], step);
            spectrum.push(Complex32::new(left, right));
        }
        if let (Some(node), Some(levels)) = (focus, &mut focus_levels)
            && engine.context.processor.processed_last_block(node)
        {
            let channels = &engine.context.graph[node].buffers;
            for frame in start..end {
                let step = (frame - window.start) / window.step;
                for channel in channels {
                    levels.add(channel[frame - first_frame], step);
                }
            }
        }
    }
    Ok(json!({
        "mix": mix.report(),
        "spectrum_energy_percent": spectrum.report(),
        "focus": focus_levels.map(|levels| levels.report()),
        "has_midi_nodes": engine.has_midi_input()
    }))
}

#[derive(Default)]
struct Energy {
    sum: f64,
    samples: usize,
}

impl Energy {
    fn add(&mut self, power: f64) {
        self.sum += power;
        self.samples += 1;
    }

    fn rms_dbfs(&self) -> Option<f64> {
        power_dbfs(self.sum / self.samples.max(1) as f64)
    }
}

struct Levels {
    total: Energy,
    steps: Vec<Energy>,
    peak: f64,
    full_scale: usize,
    non_finite: usize,
}

impl Levels {
    fn new(steps: usize) -> Self {
        Self {
            total: Energy::default(),
            steps: (0..steps).map(|_| Energy::default()).collect(),
            peak: 0.0,
            full_scale: 0,
            non_finite: 0,
        }
    }

    fn add(&mut self, sample: f32, step: usize) -> f32 {
        self.non_finite += usize::from(!sample.is_finite());
        let sample = if sample.is_finite() { sample } else { 0.0 };
        let magnitude = f64::from(sample.abs());
        self.peak = self.peak.max(magnitude);
        self.full_scale += usize::from(magnitude >= 1.0);
        let power = magnitude * magnitude;
        self.total.add(power);
        self.steps[step].add(power);
        sample
    }

    fn report(&self) -> Value {
        json!({
            "peak_dbfs": power_dbfs(self.peak * self.peak),
            "rms_dbfs": self.total.rms_dbfs(),
            "samples_at_or_above_full_scale": self.full_scale,
            "non_finite_samples": self.non_finite,
            "rms_by_grid_step_dbfs": self.steps.iter().map(Energy::rms_dbfs).collect::<Vec<_>>()
        })
    }
}

fn power_dbfs(power: f64) -> Option<f64> {
    (power > 0.0).then(|| round_tenth(10.0 * power.log10()))
}

fn round_tenth(value: f64) -> f64 {
    (value * 10.0).round() / 10.0
}

struct Spectrum {
    fft: Arc<dyn Fft<f32>>,
    scratch: Vec<Complex32>,
    block: Vec<Complex32>,
    window: Vec<f32>,
    bands: [f64; 3],
}

impl Spectrum {
    fn new() -> Self {
        let fft = FftPlanner::new().plan_fft_forward(FFT_SIZE);
        Self {
            scratch: vec![Complex32::default(); fft.get_inplace_scratch_len()],
            fft,
            block: Vec::with_capacity(FFT_SIZE),
            window: (0..FFT_SIZE)
                .map(|i| 0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / FFT_SIZE as f32).cos())
                .collect(),
            bands: [0.0; 3],
        }
    }

    fn push(&mut self, sample: Complex32) {
        self.block.push(sample * self.window[self.block.len()]);
        if self.block.len() == FFT_SIZE {
            self.accumulate();
        }
    }

    fn accumulate(&mut self) {
        self.fft
            .process_with_scratch(&mut self.block, &mut self.scratch);
        // Packing left and right into one FFT preserves their total power
        // when both frequency signs are counted.
        for (bin, value) in self.block.iter().enumerate() {
            let hz = bin.min(FFT_SIZE - bin) * SAMPLE_RATE as usize / FFT_SIZE;
            let band = if hz < 150 {
                0
            } else if hz < 2000 {
                1
            } else {
                2
            };
            self.bands[band] += f64::from(value.re).powi(2) + f64::from(value.im).powi(2);
        }
        self.block.clear();
    }

    fn report(mut self) -> Option<Value> {
        if !self.block.is_empty() {
            self.block.resize(FFT_SIZE, Complex32::default());
            self.accumulate();
        }
        let total: f64 = self.bands.iter().sum();
        (total.is_finite() && total > 0.0).then(|| {
            let [low, mid, high] = self.bands.map(|energy| round_tenth(100.0 * energy / total));
            json!({"below_150_hz": low, "150_to_2000_hz": mid, "above_2000_hz": high})
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compares_unclipped_mix_levels_and_the_selected_chain() {
        let report = compare(
            "~🥁: constsig 1.25; out: ~🥁",
            "out: constsig 0.25",
            "playing",
            120,
            Window::new(0.5, 0.25, 120).unwrap(),
            Some("~🥁"),
        )
        .unwrap();
        assert_eq!(report["start_seconds"], 0.5);
        assert_eq!(report["grid"]["beats_per_step"], 0.25);
        assert_eq!(report["baseline"]["mix"]["rms_dbfs"], -12.0);
        assert_eq!(report["candidate_minus_baseline"]["mix_rms_db"], 13.9);
        assert_eq!(report["baseline"]["focus"], Value::Null);
        assert_eq!(
            report["candidate"]["mix"]["samples_at_or_above_full_scale"],
            SAMPLE_RATE / 2
        );
        assert_eq!(
            report["candidate"]["focus"]["rms_by_grid_step_dbfs"],
            json!([1.9, 1.9])
        );
    }

    #[test]
    fn stereo_phase_cancellation_does_not_hide_bass_energy() {
        let mut spectrum = Spectrum::new();
        for frame in 0..FFT_SIZE {
            let sample = (std::f32::consts::TAU * 4.0 * frame as f32 / FFT_SIZE as f32).sin();
            spectrum.push(Complex32::new(sample, -sample));
        }
        assert_eq!(spectrum.report().unwrap()["below_150_hz"], 100.0);
    }
}
