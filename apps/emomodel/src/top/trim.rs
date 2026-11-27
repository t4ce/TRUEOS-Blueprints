use std::f32::consts::TAU;

use super::{COLLAR_ROW, COLLAR_TRIM_START, OPENING_ROW, OPENING_TRIM_END, Surface};

pub(crate) const STITCH_PERIOD_METERS: f32 = 0.008;
const STITCH_FILL: f32 = 0.70;
const MAX_SLOPE: f32 = 0.09;

pub(crate) struct Sample {
    pub(crate) weights: [f32; Surface::ALL.len()],
    pub(crate) slope: f32,
}

pub(crate) fn sample(
    row: f32,
    along_meters: f32,
    row_footprint: f32,
    along_footprint_meters: f32,
) -> Sample {
    let collar = row >= (OPENING_TRIM_END + COLLAR_TRIM_START) as f32 * 0.5;
    let (across, direction) = if collar {
        (COLLAR_ROW as f32 - row, -1.0)
    } else {
        (row - OPENING_ROW as f32, 1.0)
    };
    let width = Surface::TRIM.len() as f32;
    let footprint = row_footprint.max(0.18);
    let low = across - 0.5 * footprint;
    let high = across + 0.5 * footprint;
    let overlap = |start, end| ((high.min(end) - low.max(start)) / footprint).clamp(0.0, 1.0);
    let mut weights = [0.0; Surface::ALL.len()];
    weights[Surface::Skin as usize] = if high <= 0.0 {
        1.0
    } else {
        overlap(f32::NEG_INFINITY, 0.0)
    };
    weights[Surface::Fabric as usize] = if low >= width {
        1.0
    } else {
        overlap(width, f32::INFINITY)
    };

    let mut slope = 0.0;
    if low < width && high > 0.0 {
        let mut stitch_phase = 0.0;
        for (index, surface) in Surface::TRIM.into_iter().enumerate() {
            let weight = overlap(index as f32, index as f32 + 1.0);
            if surface == Surface::Thread {
                if weight > 0.0 {
                    let ink = stitch_coverage(along_meters, stitch_phase, along_footprint_meters);
                    weights[Surface::Thread as usize] += weight * ink;
                    weights[Surface::Band as usize] += weight * (1.0 - ink);
                }
                stitch_phase += 0.5;
            } else {
                weights[surface as usize] += weight;
            }
        }

        // The rounded ribbon rises and falls as sin² across its width. Average
        // the derivative over the visible ribbon portion of a texel.
        let start = low.max(0.0);
        let end = high.min(width);
        let frequency = TAU / width;
        slope = direction * MAX_SLOPE * ((start * frequency).cos() - (end * frequency).cos())
            / ((end - start) * frequency);
    }
    Sample { weights, slope }
}

fn stitch_coverage(along_meters: f32, phase: f32, footprint_meters: f32) -> f32 {
    let center = (along_meters / STITCH_PERIOD_METERS + phase + 0.5).rem_euclid(1.0);
    let footprint = (footprint_meters / STITCH_PERIOD_METERS).max(0.075);
    // Integrate the painted interval within each cycle so distant stitches
    // settle into an even shade instead of flickering between dashes and gaps.
    let integral = |point: f32| {
        let cycles = point.floor();
        let position = point - cycles;
        cycles * STITCH_FILL + (position - 0.5 * (1.0 - STITCH_FILL)).clamp(0.0, STITCH_FILL)
    };
    ((integral(center + 0.5 * footprint) - integral(center - 0.5 * footprint)) / footprint)
        .clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_seams_stagger_their_stitches_and_fill_gaps_with_band() {
        for (edge, direction) in [(OPENING_ROW, 1.0), (COLLAR_ROW, -1.0)] {
            for (along, expected) in [(0.0, [1.0, 0.0]), (STITCH_PERIOD_METERS * 0.5, [0.0, 1.0])] {
                for (across, thread) in [3.5, 6.5].into_iter().zip(expected) {
                    let Sample { weights, .. } =
                        sample(edge as f32 + direction * across, along, 0.3, 0.0);
                    assert!((weights[Surface::Thread as usize] - thread).abs() < 0.001);
                    assert!((weights[Surface::Band as usize] - (1.0 - thread)).abs() < 0.001);
                }
            }
        }
    }

    #[test]
    fn relief_is_confined_to_the_band_and_reverses_with_the_collar() {
        for (edge, direction) in [(OPENING_ROW, 1.0), (COLLAR_ROW, -1.0)] {
            let across = |offset| sample(edge as f32 + direction * offset, 0.0, 0.3, 0.0);
            assert_eq!(across(-1.0).slope, 0.0);
            assert!((across(2.5).slope - direction * MAX_SLOPE).abs() < 0.001);
            assert!((across(7.5).slope + direction * MAX_SLOPE).abs() < 0.001);
            assert_eq!(across(11.0).slope, 0.0);
            let boundary = across(0.0).weights;
            assert!((boundary[Surface::Skin as usize] - 0.5).abs() < 0.001);
            assert!((boundary[Surface::Outline as usize] - 0.5).abs() < 0.001);
        }
    }
}
