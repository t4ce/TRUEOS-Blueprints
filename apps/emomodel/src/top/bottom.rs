use super::{
    BottomStyle, JOIN_GAP_METERS, PANT_TRIM_FADE_METERS, SOCK_CONTOUR_METERS, shirt_hem_height,
    smooth, sock_height,
};
use glam::Vec3;

const LOWEST_BRIEF_CUFF: f32 = 0.770;
const HIGHEST_BRIEF_CUFF: f32 = 0.886;
const LOWEST_KNEE_CUFF: f32 = 0.499;
const HIGHEST_KNEE_CUFF: f32 = 0.503;
const SPACE_CUFF_MARGIN: f32 = 0.20;
const SHIRT_WAIST_JOIN_METERS: f32 = 0.004;
const SHIRT_WAIST_CLEARANCE_METERS: f32 = 0.008;

/// Signed distances from the waist and leg edges. Positive is inside the fabric.
pub(super) fn edges(Vec3 { x, y, z }: Vec3, left_leg: f32, right_leg: f32, waist: f32) -> [f32; 2] {
    let across = x.abs();
    let back = smooth((z + 0.036) / 0.080);

    // Leave a narrow band over the hips and deepen the back panel over the seat.
    let front_arch = smooth((across - 0.012) / 0.112);
    let back_arch = smooth((across - 0.026) / 0.098);
    let crotch = mix(0.782, LOWEST_BRIEF_CUFF, back);
    let brief_opening = mix(crotch, HIGHEST_BRIEF_CUFF, mix(front_arch, back_arch, back));
    let waist_edge = waist_height(across, back, waist);

    // Left is negative X. Blend only across the joined crotch so the leg
    // openings can move independently without introducing a center seam.
    let right = smooth((x + 0.014) / 0.028);
    let length = mix(left_leg.clamp(0.0, 1.0), right_leg.clamp(0.0, 1.0), right);
    let knee_opening = mix(LOWEST_KNEE_CUFF, HIGHEST_KNEE_CUFF, back);
    let leg_edge = cuff_height(brief_opening, knee_opening, length);

    [waist_edge - y, y - leg_edge]
}

fn waist_height(across: f32, back: f32, waist: f32) -> f32 {
    let hip = smooth((across - 0.015) / 0.105);
    let low_waist = 0.885 + 0.017 * hip + 0.006 * back;
    let high_waist = 1.040 + 0.006 * hip;
    mix(low_waist, high_waist, waist.clamp(0.0, 1.0))
}

/// Fade the whole waistband together as the closest part of the hem approaches.
pub(super) fn shirt_waist_join_strength(hem: f32, waist: f32) -> f32 {
    let gap = shirt_waist_min_gap(hem, waist);
    let fade = SHIRT_WAIST_CLEARANCE_METERS - SHIRT_WAIST_JOIN_METERS;
    smooth((SHIRT_WAIST_CLEARANCE_METERS - gap) / fade)
}

/// Lower bound on the shirt hem height minus the pants waist height.
fn shirt_waist_min_gap(hem: f32, waist: f32) -> f32 {
    const INTERVALS: usize = 64;
    const INTERVAL_WIDTH: f32 = 0.14 / INTERVALS as f32;
    let hem = hem.clamp(0.0, 1.0);
    // The gap tightens toward the back and both curves reach their depth
    // limits by z = .12. Both heights rise with |x| and flatten by .14 m.
    (0..INTERVALS)
        .map(|interval| {
            let low_x = interval as f32 * INTERVAL_WIDTH;
            let high_x = (interval + 1) as f32 * INTERVAL_WIDTH;
            // Use each interval's lowest hem and highest waist to bound its gap.
            shirt_hem_height(Vec3::new(low_x, 0.0, 0.12), hem) - waist_height(high_x, 1.0, waist)
        })
        .fold(f32::INFINITY, f32::min)
}

pub(crate) fn space_sock_length(preferred: f32, bottoms: BottomStyle) -> f32 {
    // Bound both pants cuffs, including the blend between opposing leg lengths.
    let left = bottoms.left_leg.clamp(0.0, 1.0);
    let right = bottoms.right_leg.clamp(0.0, 1.0);
    let lowest_cuff = cuff_height(LOWEST_BRIEF_CUFF, LOWEST_KNEE_CUFF, left.max(right));
    let highest_cuff = cuff_height(HIGHEST_BRIEF_CUFF, HIGHEST_KNEE_CUFF, left.min(right));
    let gap_limit = lowest_cuff - SOCK_CONTOUR_METERS;
    let overlap_limit = highest_cuff + SOCK_CONTOUR_METERS;

    // Pad the range of partial contact on both sides and reserve the pants trim fade.
    let margin = (overlap_limit - gap_limit) * SPACE_CUFF_MARGIN;
    let max_gap_height = gap_limit - margin - JOIN_GAP_METERS - PANT_TRIM_FADE_METERS;
    let min_overlap_height = overlap_limit + margin;
    let preferred_step = (preferred.clamp(0.0, 1.0) * 10.0).round() as i32;
    (0_i32..=10)
        .filter(|&step| {
            let height = sock_height(step as f32 / 10.0);
            height <= max_gap_height || height >= min_overlap_height
        })
        .min_by_key(|step| step.abs_diff(preferred_step))
        .unwrap_or(0) as f32
        / 10.0
}

pub(super) fn cuff_height(brief: f32, knee: f32, length: f32) -> f32 {
    // Ease out the high side of the brief arch in the first step. Keeping its
    // lowest point as the anchor lets every part of the cuff move downward.
    let rounding = 0.8 * smooth(length / 0.1);
    mix(mix(brief, LOWEST_BRIEF_CUFF, rounding), knee, length)
}

fn mix(start: f32, end: f32, amount: f32) -> f32 {
    start + (end - start) * amount
}

#[cfg(test)]
mod tests {
    use super::super::LowerFit;
    use super::*;

    fn distance(point: Vec3, left_leg: f32, right_leg: f32, waist: f32) -> f32 {
        let [waist, leg] = edges(point, left_leg, right_leg, waist);
        waist.min(leg)
    }

    #[test]
    fn first_leg_step_rounds_the_brief_opening_without_raising_it() {
        let height = |x, length| -edges(Vec3::new(x, 0.0, 0.044), length, length, 0.0)[1];
        let span = |length| height(0.13, length) - height(0.0, length);
        assert_eq!(height(0.0, 0.0), LOWEST_BRIEF_CUFF);
        assert_eq!(height(0.13, 0.0), HIGHEST_BRIEF_CUFF);
        assert!((span(0.1) / span(0.0) - 0.18).abs() < 0.001);

        for step in 1..=100 {
            let before = (step - 1) as f32 / 100.0;
            let after = step as f32 / 100.0;
            for x in [0.0, 0.04, 0.08, 0.13] {
                assert!(height(x, after) < height(x, before));
            }
        }
    }

    #[test]
    fn waist_and_leg_adjustments_expand_only_their_own_edges() {
        let hip = Vec3::new(-0.12, 0.892, -0.012);
        let belly = Vec3::new(-0.04, 0.99, -0.088);
        let left_thigh = Vec3::new(-0.07, 0.62, -0.061);
        let right_thigh = Vec3::new(0.07, 0.62, -0.061);

        assert!(distance(hip, 0.0, 0.0, 0.0) > 0.0);
        assert!(distance(belly, 1.0, 1.0, 0.0) < 0.0);
        assert!(distance(belly, 0.0, 0.0, 1.0) > 0.0);
        assert!(distance(left_thigh, 1.0, 0.0, 0.0) > 0.0);
        assert!(distance(right_thigh, 1.0, 0.0, 1.0) < 0.0);
        assert!(distance(right_thigh, 0.0, 1.0, 0.0) > 0.0);

        for p in [hip, belly, left_thigh, right_thigh] {
            assert_eq!(distance(p, 2.0, -1.0, 2.0), distance(p, 1.0, 0.0, 1.0));
            assert!(distance(p, 1.0, 1.0, 1.0) >= distance(p, 0.0, 0.0, 0.0));
        }
    }

    #[test]
    fn shirt_waist_bound_is_below_the_rendered_gap() {
        for waist_step in 0..=10 {
            let waist = waist_step as f32 / 10.0;
            for hem_step in 0..=10 {
                let hem = hem_step as f32 / 10.0;
                let min_gap = shirt_waist_min_gap(hem, waist);
                for x in [-0.16, -0.095, -0.045, 0.0, 0.045, 0.095, 0.16] {
                    for z in [-0.14, -0.036, 0.02, 0.044, 0.14] {
                        let point = Vec3::new(x, 0.0, z);
                        let gap = shirt_hem_height(point, hem) - edges(point, 0.0, 0.0, waist)[0];
                        assert!(
                            gap >= min_gap - 1e-6,
                            "hem={hem}, waist={waist}, point={point}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn shirt_waist_join_fades_between_four_and_eight_millimeters() {
        assert!(shirt_waist_min_gap(0.8, 0.0) < -0.01);
        assert_eq!(shirt_waist_join_strength(0.8, 0.0), 1.0);

        let strength = |hem| shirt_waist_join_strength(hem, 0.5);
        assert_eq!(strength(0.265), 0.0);
        assert_eq!(strength(0.30), 1.0);
        assert!(0.0 < strength(0.275));
        assert!(strength(0.275) < strength(0.282));
        assert!(strength(0.282) < strength(0.289));
        assert!(strength(0.289) < 1.0);

        let clear_gap = shirt_waist_min_gap(0.4, 0.3);
        assert!(clear_gap > 0.008 && clear_gap < 0.01);
        assert_eq!(shirt_waist_join_strength(0.4, 0.3), 0.0);
    }

    #[test]
    fn space_socks_pick_a_clear_gap_or_full_overlap_for_both_legs() {
        for left in 0..=10 {
            for right in 0..=10 {
                let pants = BottomStyle {
                    left_leg: left as f32 / 10.0,
                    right_leg: right as f32 / 10.0,
                    waist: 0.0,
                };
                for roll in 0..=10 {
                    let socks = Some(space_sock_length(roll as f32 / 10.0, pants));
                    let mut clear_gap = true;
                    let mut full_overlap = true;
                    for x in [-0.14, -0.09, -0.04, -0.01, 0.0, 0.01, 0.04, 0.09, 0.14] {
                        for z in [-0.1, -0.036, 0.02, 0.044, 0.1] {
                            let gap = LowerFit::at(Vec3::new(x, 0.0, z), pants, socks).gap();
                            clear_gap &= gap >= JOIN_GAP_METERS + PANT_TRIM_FADE_METERS;
                            full_overlap &= gap <= 0.0;
                        }
                    }
                    assert!(
                        clear_gap || full_overlap,
                        "left={left}, right={right}, roll={roll}"
                    );
                }
            }
        }

        let mut pants = BottomStyle::default();
        assert_eq!(space_sock_length(1.0, pants), 0.9);
        pants.left_leg = 0.5;
        pants.right_leg = 0.5;
        assert_eq!(space_sock_length(1.0, pants), 1.0);
    }
}
