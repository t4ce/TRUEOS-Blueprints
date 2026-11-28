use glam::Vec3;

use super::smooth;

pub(super) const RELIEF_WIDTH_METERS: f32 = 0.004;
const INSTEP_HEIGHT_METERS: f32 = 0.058;
const TOE_RISE_METERS: f32 = 0.015;
const HEEL_RISE_METERS: f32 = 0.019;
const SIDE_RISE_METERS: f32 = 0.006;
pub(super) const MAX_HEIGHT_METERS: f32 =
    INSTEP_HEIGHT_METERS + HEEL_RISE_METERS + SIDE_RISE_METERS;

/// Positive below the opening of the flat. The bind feet point toward -Z.
pub(super) fn edge(Vec3 { x, y, z }: Vec3) -> f32 {
    let across = (x.abs() - 0.070).abs();
    // Sweep the center of the opening toward the toes and leave the sides higher.
    let toe_arc = (across / 0.034).min(1.0);
    let toe_start = 0.073 - 0.014 * toe_arc * toe_arc;
    let toe = smooth((-z - toe_start) / 0.026);
    let heel = smooth((z - 0.025) / 0.042);
    let side = smooth((across - 0.014) / 0.021);
    INSTEP_HEIGHT_METERS + TOE_RISE_METERS * toe + HEEL_RISE_METERS * heel + SIDE_RISE_METERS * side
        - y
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_toe_opening_curves_forward_between_the_covered_sides() {
        for side in [-1.0, 1.0] {
            let at = |x: f32, y, z| edge(Vec3::new(side * x, y, z));
            assert!(at(0.06954, 0.06437, -0.07606) < 0.0);
            assert!(at(0.100, 0.064, -0.080) > 0.0);
            assert!(at(0.070, 0.064, -0.100) > 0.0);
            assert!(at(0.10343, 0.06149, -0.04950) > 0.0);
            assert!(at(0.03900, 0.06294, 0.03790) > 0.0);
        }
    }
}
