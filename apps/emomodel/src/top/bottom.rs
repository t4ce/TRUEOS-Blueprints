use super::{
    BottomStyle, JOIN_GAP_METERS, PANT_TRIM_FADE_METERS, SOCK_CONTOUR_METERS, smooth, sock_height,
};
use glam::Vec3;

const LOWEST_BRIEF_CUFF: f32 = 0.770;
const HIGHEST_BRIEF_CUFF: f32 = 0.886;
const LOWEST_KNEE_CUFF: f32 = 0.499;
const HIGHEST_KNEE_CUFF: f32 = 0.503;
const SPACE_CUFF_MARGIN: f32 = 0.20;

/// Signed distances from the waist and leg edges. Positive is inside the fabric.
pub(super) fn edges(Vec3 { x, y, z }: Vec3, left_leg: f32, right_leg: f32, waist: f32) -> [f32; 2] {
    let across = x.abs();
    let back = smooth((z + 0.036) / 0.080);

    // Leave a narrow band over the hips and deepen the back panel over the seat.
    let front_arch = smooth((across - 0.012) / 0.112);
    let back_arch = smooth((across - 0.026) / 0.098);
    let crotch = mix(0.782, LOWEST_BRIEF_CUFF, back);
    let brief_opening = mix(crotch, HIGHEST_BRIEF_CUFF, mix(front_arch, back_arch, back));
    let hip = smooth((across - 0.015) / 0.105);
    let low_waist = 0.885 + 0.017 * hip + 0.006 * back;
    let high_waist = 1.040 + 0.006 * hip;
    let waist_edge = mix(low_waist, high_waist, waist.clamp(0.0, 1.0));

    // Left is negative X. Blend only across the joined crotch so the leg
    // openings can move independently without introducing a center seam.
    let right = smooth((x + 0.014) / 0.028);
    let length = mix(left_leg.clamp(0.0, 1.0), right_leg.clamp(0.0, 1.0), right);
    let knee_opening = mix(LOWEST_KNEE_CUFF, HIGHEST_KNEE_CUFF, back);
    let leg_edge = mix(brief_opening, knee_opening, length);

    [waist_edge - y, y - leg_edge]
}

pub(crate) fn space_sock_length(preferred: f32, bottoms: BottomStyle) -> f32 {
    // Bound both pants cuffs, including the blend between opposing leg lengths.
    let left = bottoms.left_leg.clamp(0.0, 1.0);
    let right = bottoms.right_leg.clamp(0.0, 1.0);
    let lowest_cuff = mix(LOWEST_BRIEF_CUFF, LOWEST_KNEE_CUFF, left.max(right));
    let highest_cuff = mix(HIGHEST_BRIEF_CUFF, HIGHEST_KNEE_CUFF, left.min(right));
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
