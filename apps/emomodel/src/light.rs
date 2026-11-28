pub(super) const SKIN_SRGB: [u8; 3] = [218, 171, 147];

#[cfg(target_os = "trueos")]
use glam::Vec3;

#[cfg(target_os = "trueos")]
pub(super) const AMBIENT: f32 = 0.14;

#[cfg(target_os = "trueos")]
pub(super) fn diffuse_energy(normal: Vec3, key: Vec3) -> f32 {
    AMBIENT + (1.0 - AMBIENT) * normal.dot(key).clamp(0.0, 1.0)
}

// Include the unlit half so the GPU interpolates its normal projections before
// the lookup clamps the key's diffuse contribution.
#[cfg(target_os = "trueos")]
pub(super) const DIFFUSE_WIDTH: usize = 257;

#[cfg(target_os = "trueos")]
pub(super) fn diffuse_lookup_rgba(rgb: [u8; 3]) -> Vec<u8> {
    diffuse_lookup_with_peak(rgb, 1.0)
}

#[cfg(target_os = "trueos")]
pub(super) fn diffuse_column(projection: f32, peak: f32) -> f32 {
    let normalized = (projection + 1.0) / (peak + 1.0);
    normalized.clamp(0.0, 1.0) * (DIFFUSE_WIDTH - 1) as f32 + 0.5
}

#[cfg(target_os = "trueos")]
pub(super) fn diffuse_lookup_with_peak(rgb: [u8; 3], peak: f32) -> Vec<u8> {
    let linear = rgb.map(|channel| ((f32::from(channel) / 255.0 + 0.055) / 1.055).powf(2.4));
    (0usize..DIFFUSE_WIDTH)
        .flat_map(|column| {
            let direct = (column as f32 / (DIFFUSE_WIDTH - 1) as f32 * (peak + 1.0) - 1.0).max(0.0);
            let energy = AMBIENT + (1.0 - AMBIENT) * direct;
            let [r, g, b] = linear.map(|channel| {
                let srgb = 1.055 * (channel * energy).powf(1.0 / 2.4) - 0.055;
                (srgb * 255.0).round() as u8
            });
            [r, g, b, 255]
        })
        .collect()
}
