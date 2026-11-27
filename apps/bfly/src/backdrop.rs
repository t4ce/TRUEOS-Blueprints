//! Clock-driven display color through neighboring green-to-purple HSL swatches.

const HUES: usize = 41;
const LIGHTNESSES: usize = 22;
const SWATCHES: usize = HUES * LIGHTNESSES;
const INTRO_STEP_NS: u64 = 4_000_000_000;
const INTRO_LEVELS_PER_STEP: u8 = 6;
const WEIGHT: u64 = 1 << 16;
const SWEEP_NS: u64 = sweep_duration_ns();

pub struct Sample {
    pub rgb: [u8; 3],
    pub precise_rgb: [f32; 3],
}

impl Sample {
    fn from_ratio(numerators: [u64; 3], scale: u64) -> Self {
        Self {
            rgb: numerators.map(|value| ((value + scale / 2) / scale) as u8),
            precise_rgb: numerators.map(|value| value as f32 / scale as f32),
        }
    }
}

pub struct Backdrop {
    initial: [u8; 3],
    entry_rgb: [u8; 3],
    started_ns: u64,
    last_ns: u64,
    intro_steps: u64,
    segment_started_ns: u64,
    swatch: usize,
}

impl Backdrop {
    pub fn new(initial: [u8; 3], now_ns: u64) -> Self {
        let swatch = nearest_swatch(initial);
        let entry_rgb = swatch_sample(swatch, swatch, 0).rgb;
        let max_delta = initial[0]
            .abs_diff(entry_rgb[0])
            .max(initial[1].abs_diff(entry_rgb[1]))
            .max(initial[2].abs_diff(entry_rgb[2]));
        let intro_steps = u64::from(max_delta.div_ceil(INTRO_LEVELS_PER_STEP));
        Self {
            initial,
            entry_rgb,
            started_ns: now_ns,
            last_ns: now_ns,
            intro_steps,
            segment_started_ns: intro_steps * INTRO_STEP_NS,
            swatch,
        }
    }

    /// Sample the transition from the current display color to the entry swatch.
    /// Once settled, returns that swatch and true without advancing the sweep.
    pub fn sample_entry_at(&self, now_ns: u64) -> ([u8; 3], bool) {
        match self.entry_sample(now_ns.saturating_sub(self.started_ns)) {
            Some(sample) => (sample.rgb, false),
            None => (self.entry_rgb, true),
        }
    }

    fn entry_sample(&self, elapsed_ns: u64) -> Option<Sample> {
        let intro_ns = self.intro_steps * INTRO_STEP_NS;
        if elapsed_ns >= intro_ns {
            return None;
        }
        let step = elapsed_ns / INTRO_STEP_NS;
        let step_elapsed_ns = elapsed_ns % INTRO_STEP_NS;
        let weight = step * WEIGHT + eased_weight(step_elapsed_ns, INTRO_STEP_NS);
        let total = self.intro_steps * WEIGHT;
        let numerators = core::array::from_fn(|channel| {
            let a = u64::from(self.initial[channel]);
            let b = u64::from(self.entry_rgb[channel]);
            a * (total - weight) + b * weight
        });
        Some(Sample::from_ratio(numerators, total))
    }

    pub fn sample_at(&mut self, now_ns: u64) -> Sample {
        self.last_ns = self.last_ns.max(now_ns);
        let elapsed_ns = self.last_ns - self.started_ns;
        if let Some(sample) = self.entry_sample(elapsed_ns) {
            return sample;
        }
        let elapsed_since_segment = elapsed_ns - self.segment_started_ns;
        let mut segment_elapsed_ns = elapsed_since_segment % SWEEP_NS;
        // A full sweep ends at the same swatch regardless of its start.
        self.segment_started_ns += elapsed_since_segment - segment_elapsed_ns;
        loop {
            let duration_ns = step_duration_ns(self.swatch);
            let next = next_swatch(self.swatch);
            if segment_elapsed_ns < duration_ns {
                return swatch_sample(
                    self.swatch,
                    next,
                    eased_weight(segment_elapsed_ns, duration_ns),
                );
            }
            segment_elapsed_ns -= duration_ns;
            self.segment_started_ns += duration_ns;
            self.swatch = next;
        }
    }

    #[cfg(test)]
    fn rgb_at(&mut self, now_ns: u64) -> [u8; 3] {
        self.sample_at(now_ns).rgb
    }
}

fn grid_point(swatch: usize) -> (usize, usize) {
    if swatch < HUES {
        return (swatch, 0);
    }
    let offset = swatch - HUES;
    let row_width = HUES - 1;
    let row = 1 + offset / row_width;
    if row < LIGHTNESSES {
        let column = offset % row_width;
        return (
            if row % 2 == 0 {
                column + 1
            } else {
                row_width - column
            },
            row,
        );
    }
    // Leave column zero free on the downward sweep, then take it back up.
    (0, SWATCHES - swatch)
}

fn next_swatch(swatch: usize) -> usize {
    (swatch + 1) % SWATCHES
}

const fn step_duration_ns(swatch: usize) -> u64 {
    3_000_000_000 + ((swatch as u64 * 31 + 17) % 9) * 250_000_000
}

const fn sweep_duration_ns() -> u64 {
    let mut swatch = 0;
    let mut duration_ns = 0;
    while swatch < SWATCHES {
        duration_ns += step_duration_ns(swatch);
        swatch += 1;
    }
    duration_ns
}

fn eased_weight(elapsed_ns: u64, duration_ns: u64) -> u64 {
    let t = elapsed_ns * WEIGHT / duration_ns;
    let scale = WEIGHT * WEIGHT;
    (t * t * (3 * WEIGHT - 2 * t) + scale / 2) / scale
}

fn swatch_sample(start: usize, end: usize, weight: u64) -> Sample {
    let (start_column, start_row) = grid_point(start);
    let (end_column, end_row) = grid_point(end);
    let mix = |a: u64, b: u64| a * (WEIGHT - weight) + b * weight;
    let hue = mix(120 + 4 * start_column as u64, 120 + 4 * end_column as u64);
    // Lightness goes from 21/84 to 63/84, evenly spaced through 22 rows.
    let lightness = mix(21 + 2 * start_row as u64, 21 + 2 * end_row as u64);
    let light_scale = 84 * WEIGHT;
    let chroma = 2 * lightness.min(light_scale - lightness);
    let base = lightness - chroma / 2;
    let sector_scale = 60 * WEIGHT;
    let sector = hue / sector_scale;
    let position = hue % sector_scale;
    let rising = chroma * position / sector_scale;
    let rgb = match sector {
        2 => [0, chroma, rising],
        3 => [0, chroma - rising, chroma],
        _ => [rising, 0, chroma],
    };
    Sample::from_ratio(rgb.map(|component| (component + base) * 255), light_scale)
}

fn nearest_swatch(initial: [u8; 3]) -> usize {
    let achromatic = initial[0] == initial[1] && initial[1] == initial[2];
    let mut nearest = 0;
    let mut best_distance = u32::MAX;
    for swatch in 0..SWATCHES {
        if achromatic && grid_point(swatch).0 != 0 {
            continue;
        }
        let candidate = swatch_sample(swatch, swatch, 0).rgb;
        let distance = (0..3)
            .map(|channel| u32::from(initial[channel].abs_diff(candidate[channel])).pow(2))
            .sum();
        if distance < best_distance {
            nearest = swatch;
            best_distance = distance;
        }
    }
    nearest
}

#[cfg(test)]
mod tests {
    use super::*;

    fn swatch_color(start: usize, end: usize, weight: u64) -> [u8; 3] {
        swatch_sample(start, end, weight).rgb
    }

    #[test]
    fn sweep_visits_every_adjacent_swatch_once() {
        let mut seen = [false; SWATCHES];
        let mut colors = [[0; 3]; SWATCHES];
        for swatch in 0..SWATCHES {
            let (column, row) = grid_point(swatch);
            let (next_column, next_row) = grid_point(next_swatch(swatch));
            assert!(column < HUES && row < LIGHTNESSES);
            assert_eq!(column.abs_diff(next_column) + row.abs_diff(next_row), 1);
            let key = row * HUES + column;
            assert!(!seen[key]);
            seen[key] = true;
            let color = swatch_color(swatch, swatch, 0);
            assert!(!colors[..swatch].contains(&color));
            colors[swatch] = color;
        }
        assert!(seen.into_iter().all(|visited| visited));
    }

    #[test]
    fn sweep_respects_hue_lightness_and_step_duration() {
        assert_eq!(swatch_color(0, 0, 0), [0, 128, 0]);
        assert_eq!(swatch_color(40, 40, 0), [85, 0, 128]);
        assert_eq!(
            swatch_color(
                SWATCHES - (LIGHTNESSES - 1),
                SWATCHES - (LIGHTNESSES - 1),
                0
            ),
            [128, 255, 128]
        );
        for swatch in 0..SWATCHES {
            let duration = step_duration_ns(swatch);
            assert!((3_000_000_000..=5_000_000_000).contains(&duration));
            let rgb = swatch_color(swatch, next_swatch(swatch), WEIGHT / 2);
            let min = *rgb.iter().min().unwrap();
            let max = *rgb.iter().max().unwrap();
            assert!(min == 0 || max == 255);
            assert!((127..=383).contains(&(u16::from(min) + u16::from(max))));
        }
    }

    #[test]
    fn intro_starts_at_the_current_rgb_and_drifts_into_green() {
        let initial = [255; 3];
        let mut backdrop = Backdrop::new(initial, 0);
        assert_eq!(backdrop.rgb_at(0), initial);
        assert_eq!(grid_point(backdrop.swatch).0, 0);
        let mut previous = initial;
        for step in 1..=backdrop.intro_steps {
            let rgb = backdrop.rgb_at(step * INTRO_STEP_NS);
            assert!(
                (0..3).all(
                    |channel| rgb[channel].abs_diff(previous[channel]) <= INTRO_LEVELS_PER_STEP
                )
            );
            previous = rgb;
        }
        assert_eq!(previous, backdrop.entry_rgb);
    }

    #[test]
    fn entry_only_starts_at_the_programmed_color_and_holds_the_nearest_swatch() {
        let initial = [22, 96, 180];
        let started = 9_000;
        let mut backdrop = Backdrop::new(initial, started);
        let swatch = nearest_swatch(initial);
        let target = swatch_color(swatch, swatch, 0);
        let settled = started + backdrop.intro_steps * INTRO_STEP_NS;
        assert_eq!(backdrop.sample_entry_at(started), (initial, false));
        for step in 0..backdrop.intro_steps {
            let now = started + step * INTRO_STEP_NS;
            assert_eq!(backdrop.sample_entry_at(now), (backdrop.rgb_at(now), false));
        }
        assert_eq!(backdrop.sample_entry_at(settled), (target, true));
        assert_eq!(
            backdrop.sample_entry_at(settled + SWEEP_NS / 2),
            (target, true)
        );
        assert_eq!(
            Backdrop::new(target, started).sample_entry_at(started),
            (target, true)
        );
    }

    #[test]
    fn elapsed_time_supports_clock_jumps_and_preserves_the_whole_cycle() {
        let ns = 13_000;
        let initial = swatch_color(219, 219, 0);
        let mut backdrop = Backdrop::new(initial, ns);
        assert_eq!(backdrop.rgb_at(ns), initial);
        let duration = step_duration_ns(219);
        let middle = backdrop.rgb_at(ns + duration / 2);
        assert_eq!(backdrop.rgb_at(ns + duration / 2 - 1), middle);
        assert_eq!(backdrop.rgb_at(ns + duration), swatch_color(220, 220, 0));
        assert_eq!(backdrop.rgb_at(ns + SWEEP_NS * 76_543), initial);
        assert_eq!(backdrop.rgb_at(u64::MAX), backdrop.rgb_at(u64::MAX));
    }
}
