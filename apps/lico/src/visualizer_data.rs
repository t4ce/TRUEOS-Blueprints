use crate::audio::{MONITOR_FRAMES, MonitorFrame, SAMPLE_RATE};
use rustfft::{Fft, FftPlanner, num_complex::Complex32};
use std::sync::Arc;

const WAVEFORM_POINTS: usize = 64;
const SPECTRUM_BANDS: usize = 40;
const TRACKS: usize = 8;
const SPECTRUM: usize = WAVEFORM_POINTS;
const HITS: usize = SPECTRUM + SPECTRUM_BANDS;
const PHASES: usize = HITS + TRACKS;
const FEATURES: usize = PHASES + TRACKS;

pub struct Analyzer {
    fft: Arc<dyn Fft<f32>>,
    bins: [Complex32; MONITOR_FRAMES],
    scratch: Vec<Complex32>,
    window: [f32; MONITOR_FRAMES],
    band_edges: [usize; SPECTRUM_BANDS + 1],
    analysis: [f32; 128],
    run: u64,
    previous_rms: f32,
}

impl Analyzer {
    pub fn new() -> Self {
        let fft = FftPlanner::new().plan_fft_forward(MONITOR_FRAMES);
        Self {
            scratch: vec![Complex32::default(); fft.get_inplace_scratch_len()],
            fft,
            bins: [Complex32::default(); MONITOR_FRAMES],
            window: std::array::from_fn(|i| {
                let phase = std::f32::consts::TAU * i as f32 / (MONITOR_FRAMES - 1) as f32;
                0.5 * (1.0 - phase.cos())
            }),
            band_edges: std::array::from_fn(|i| {
                (MONITOR_FRAMES as f32 / 2.0).powf(i as f32 / SPECTRUM_BANDS as f32) as usize
            }),
            analysis: [0.0; 128],
            run: 0,
            previous_rms: 0.0,
        }
    }

    pub fn sample(&mut self, frame: &MonitorFrame) -> &[f32; 128] {
        if !frame.playing || self.run != frame.run {
            self.analysis.fill(0.0);
            self.previous_rms = 0.0;
            self.run = frame.run;
        }
        self.analysis[HITS..PHASES].fill(0.0);
        self.analysis[PHASES..FEATURES].fill(-1.0);
        if !frame.playing {
            return &self.analysis;
        }

        let waveform = frame.waveform.map(|sample| {
            if sample.is_finite() {
                sample.clamp(-1.0, 1.0)
            } else {
                0.0
            }
        });
        let samples_per_point = MONITOR_FRAMES / WAVEFORM_POINTS;
        for (slot, chunk) in self.analysis[..SPECTRUM]
            .iter_mut()
            .zip(waveform.chunks_exact(samples_per_point))
        {
            *slot = chunk.iter().sum::<f32>() / samples_per_point as f32;
        }
        for ((bin, &sample), &weight) in self.bins.iter_mut().zip(&waveform).zip(&self.window) {
            *bin = Complex32::new(sample * weight, 0.0);
        }
        self.fft
            .process_with_scratch(&mut self.bins, &mut self.scratch);
        for band in 0..SPECTRUM_BANDS {
            let start = self.band_edges[band];
            let end = self.band_edges[band + 1]
                .max(start + 1)
                .min(MONITOR_FRAMES / 2);
            let level = self.level(start, end);
            let previous = &mut self.analysis[SPECTRUM + band];
            *previous = level.max(*previous * 0.80);
        }

        let rms = (waveform.iter().map(|sample| sample * sample).sum::<f32>()
            / MONITOR_FRAMES as f32)
            .sqrt();
        let onset = ((rms - self.previous_rms) * 7.0).clamp(0.0, 1.0);
        self.previous_rms = rms;
        let mut track_count = 0;
        for (track, playhead) in frame.sequences.iter().flatten().take(TRACKS).enumerate() {
            self.analysis[PHASES + track] =
                ((playhead.bar_frame as f64 / playhead.bar_length) as f32).clamp(0.0, 1.0);
            if let Some(age) = playhead.frames_since_trigger {
                self.analysis[HITS + track] = (-(age as f32) / (0.09 * SAMPLE_RATE as f32)).exp2();
            }
            track_count += 1;
        }
        let bass = self.level(1, 4);
        let mids = self.level(4, 26);
        let treble = self.level(26, MONITOR_FRAMES / 2);
        let onset = onset.max(self.analysis[FEATURES + 6] * 0.66);
        self.analysis[FEATURES..].copy_from_slice(&[
            (rms * 3.0).sqrt().min(1.0),
            bass,
            mids,
            treble,
            frame.beat_phase,
            1.0,
            onset,
            track_count as f32 / TRACKS as f32,
        ]);
        &self.analysis
    }

    fn level(&self, start: usize, end: usize) -> f32 {
        let peak = self.bins[start..end]
            .iter()
            .map(Complex32::norm_sqr)
            .fold(0.0, f32::max);
        let magnitude = peak.sqrt() * 4.0 / MONITOR_FRAMES as f32;
        (1.0 + 0.25 * magnitude.max(0.0001).log10()).clamp(0.0, 1.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use glicol::SequencePlayhead;

    #[test]
    fn sends_real_signal_and_live_track_positions_then_clears_on_stop() {
        let mut analyzer = Analyzer::new();
        let mut frame = MonitorFrame {
            run: 1,
            playing: true,
            beat_phase: 0.6,
            waveform: std::array::from_fn(|sample| {
                0.3 * (sample as f32 * std::f32::consts::TAU * 9.0 / MONITOR_FRAMES as f32).sin()
            }),
            sequences: vec![
                None,
                Some(SequencePlayhead {
                    bar_frame: 12_000,
                    bar_length: 48_000.0,
                    frames_since_trigger: Some(0),
                }),
                Some(SequencePlayhead {
                    bar_frame: 30_000,
                    bar_length: 60_000.0,
                    frames_since_trigger: None,
                }),
            ],
        };
        let result = analyzer.sample(&frame);
        assert!(result[..SPECTRUM].iter().any(|&value| value > 0.2));
        assert!(result[SPECTRUM..HITS].iter().any(|&value| value > 0.7));
        assert_eq!(&result[HITS..HITS + 2], &[1.0, 0.0]);
        assert_eq!(&result[PHASES..PHASES + 3], &[0.25, 0.5, -1.0]);
        assert_eq!(&result[124..126], &[0.6, 1.0]);
        assert!(result.iter().all(|v| (-1.0..=1.0).contains(v)));

        frame.playing = false;
        let result = analyzer.sample(&frame);
        assert_eq!(&result[..PHASES], &[0.0; PHASES]);
        assert_eq!(&result[PHASES..FEATURES], &[-1.0; TRACKS]);
        assert_eq!(&result[FEATURES..], &[0.0; 8]);
    }
}
