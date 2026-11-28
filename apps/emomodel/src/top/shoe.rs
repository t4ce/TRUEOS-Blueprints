use glam::Vec3;

use super::smooth;

pub(super) const RELIEF_WIDTH_METERS: f32 = 0.004;
const INSTEP_HEIGHT_METERS: f32 = 0.055;
const CAP_RISE_METERS: f32 = 0.017;
const SIDE_RISE_METERS: f32 = 0.004;
pub(super) const MAX_HEIGHT_METERS: f32 = INSTEP_HEIGHT_METERS + CAP_RISE_METERS + SIDE_RISE_METERS;

/// Positive below the opening of the flat. The bind feet point toward -Z.
pub(super) fn edge(Vec3 { x, y, z }: Vec3) -> f32 {
    let across = (x.abs() - 0.070).abs();
    let toe = smooth((-z - 0.055) / 0.030);
    let heel = smooth((z - 0.025) / 0.042);
    let side = smooth((across - 0.014) / 0.021);
    INSTEP_HEIGHT_METERS + CAP_RISE_METERS * (toe + heel) + SIDE_RISE_METERS * side - y
}
