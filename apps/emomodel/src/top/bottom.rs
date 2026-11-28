use super::smooth;
use glam::Vec3;

/// Signed distances from the waist and leg edges. Positive is inside the fabric.
pub(super) fn edges(Vec3 { x, y, z }: Vec3, left_leg: f32, right_leg: f32, waist: f32) -> [f32; 2] {
    let across = x.abs();
    let back = smooth((z + 0.036) / 0.080);

    // Leave a narrow band over the hips and deepen the back panel over the seat.
    let front_arch = smooth((across - 0.012) / 0.112);
    let back_arch = smooth((across - 0.026) / 0.098);
    let crotch = mix(0.782, 0.770, back);
    let brief_opening = mix(crotch, 0.886, mix(front_arch, back_arch, back));
    let hip = smooth((across - 0.015) / 0.105);
    let low_waist = 0.885 + 0.017 * hip + 0.006 * back;
    let high_waist = 1.040 + 0.006 * hip;
    let waist_edge = mix(low_waist, high_waist, waist.clamp(0.0, 1.0));

    // Left is negative X. Blend only across the joined crotch so the leg
    // openings can move independently without introducing a center seam.
    let right = smooth((x + 0.014) / 0.028);
    let length = mix(left_leg.clamp(0.0, 1.0), right_leg.clamp(0.0, 1.0), right);
    let knee_opening = 0.501 + 0.004 * (back - 0.5);
    let leg_edge = mix(brief_opening, knee_opening, length);

    [waist_edge - y, y - leg_edge]
}

fn mix(start: f32, end: f32, amount: f32) -> f32 {
    start + (end - start) * amount
}

#[cfg(test)]
mod tests {
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
}
