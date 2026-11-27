use glam::Vec3;

use crate::autobs::{BASE_YAW, camera_back};

pub(super) const SWEEP_NS: u64 = 8_000_000_000;
const END_HOLD_NS: u64 = 1_000_000_000;
const SWAP_INTERVAL_NS: u64 = 15_000_000_000;
const SWAP_DURATION_NS: u64 = 3_000_000_000;
const LIMIT_DEGREES: i16 = 55;
pub(super) const ANGLE_COUNT: usize = (2 * LIMIT_DEGREES + 1) as usize;
const ELEVATION: f32 = 28.0_f32.to_radians();
const EDGE: usize = 64;
const BMP_HEADER_BYTES: usize = 54;
pub(super) const SKIN_SRGB: [u8; 3] = [218, 171, 147];
const AMBIENT: f32 = 0.14;
const RAMP_SIZE: usize = 256;

pub(super) fn angle_index(angle: i16) -> usize {
    (angle + LIMIT_DEGREES) as usize
}

pub(super) fn angle(elapsed_ns: u64) -> i16 {
    if elapsed_ns < SWEEP_NS {
        return eased_angle(
            -LIMIT_DEGREES,
            elapsed_ns.saturating_sub(END_HOLD_NS),
            SWEEP_NS - 2 * END_HOLD_NS,
        );
    }
    let playback_ns = elapsed_ns - SWEEP_NS;
    let cycle = playback_ns / SWAP_INTERVAL_NS;
    if cycle == 0 {
        return LIMIT_DEGREES;
    }
    let from = if cycle.is_multiple_of(2) {
        -LIMIT_DEGREES
    } else {
        LIMIT_DEGREES
    };
    eased_angle(from, playback_ns % SWAP_INTERVAL_NS, SWAP_DURATION_NS)
}

fn eased_angle(from: i16, elapsed_ns: u64, duration_ns: u64) -> i16 {
    let t = elapsed_ns.min(duration_ns) as f32 / duration_ns as f32;
    let eased = t * t * (3.0 - 2.0 * t);
    ((1.0 - 2.0 * eased) * f32::from(from)).round() as i16
}

pub(super) fn presentation_markers() -> impl Iterator<Item = u64> {
    [0, SWEEP_NS / 2, SWEEP_NS - END_HOLD_NS / 2]
        .into_iter()
        .chain((1..).flat_map(|cycle| {
            let start = SWEEP_NS + cycle * SWAP_INTERVAL_NS;
            [
                start,
                start + SWAP_DURATION_NS / 2,
                start + SWAP_DURATION_NS,
            ]
        }))
}

pub(super) struct LightPlane {
    center: Vec3,
    side: Vec3,
}

impl LightPlane {
    pub(super) fn new() -> Self {
        let back = camera_back(BASE_YAW);
        let (rise, run) = ELEVATION.sin_cos();
        Self {
            center: back * run + Vec3::Y * rise,
            side: Vec3::Y.cross(back),
        }
    }

    pub(super) fn coordinates(&self, normal: Vec3) -> [f32; 2] {
        // The light moves in this plane. Its dot product with the normal only
        // needs these two projections, with no angular seams or UV unwrap.
        [normal.dot(self.center), normal.dot(self.side)].map(|value| {
            let texel = (value.clamp(-1.0, 1.0) + 1.0) * 0.5 * (EDGE - 1) as f32;
            (texel + 0.5) / EDGE as f32
        })
    }

    #[cfg(target_os = "trueos")]
    pub(super) fn diffuse_direction(&self, angle: i16) -> Vec3 {
        let (side, center) = f32::from(angle).to_radians().sin_cos();
        self.center * center + self.side * side
    }
}

pub(super) struct DiffuseLookup {
    bmp: Vec<u8>,
    ramp_bgr: [[u8; 3]; RAMP_SIZE],
}

fn diffuse_ramp<const N: usize>(rgb: [u8; 3]) -> [[u8; 3]; N] {
    let linear = rgb.map(|channel| ((f32::from(channel) / 255.0 + 0.055) / 1.055).powf(2.4));
    std::array::from_fn(|level| {
        let intensity = AMBIENT + (1.0 - AMBIENT) * level as f32 / (N - 1) as f32;
        linear.map(|channel| {
            let encoded = 1.055 * (channel * intensity).powf(1.0 / 2.4) - 0.055;
            (encoded * 255.0).round() as u8
        })
    })
}

// Include the unlit half of the dot product so clamping happens after the
// GPU interpolates the normals' projections across the triangle.
#[cfg(target_os = "trueos")]
pub(super) const DIFFUSE_WIDTH: usize = 257;

#[cfg(target_os = "trueos")]
pub(super) fn diffuse_lookup_rgba(rgb: [u8; 3]) -> Vec<u8> {
    let ramp = diffuse_ramp::<{ DIFFUSE_WIDTH / 2 + 1 }>(rgb);
    std::iter::repeat_n(ramp[0], DIFFUSE_WIDTH / 2)
        .chain(ramp)
        .flat_map(|[r, g, b]| [r, g, b, 255])
        .collect()
}

impl DiffuseLookup {
    pub(super) fn new() -> Self {
        let mut bmp = vec![0; BMP_HEADER_BYTES + EDGE * EDGE * 3];
        let file_bytes = bmp.len() as u32;
        bmp[..2].copy_from_slice(b"BM");
        for (offset, value) in [
            (2, file_bytes),
            (10, BMP_HEADER_BYTES as u32),
            (14, 40),
            (18, EDGE as u32),
            (22, EDGE as u32),
            (34, (EDGE * EDGE * 3) as u32),
        ] {
            bmp[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        }
        bmp[26..30].copy_from_slice(&[1, 0, 24, 0]);
        let ramp_bgr = diffuse_ramp(SKIN_SRGB).map(|[r, g, b]| [b, g, r]);
        Self { bmp, ramp_bgr }
    }

    pub(super) fn update(&mut self, angle_degrees: i16) -> &[u8] {
        let (side, center) = f32::from(angle_degrees).to_radians().sin_cos();
        let projection = |texel: usize| 2.0 * texel as f32 / (EDGE - 1) as f32 - 1.0;
        let (rows, _) = self.bmp[BMP_HEADER_BYTES..].as_chunks_mut::<{ EDGE * 3 }>();
        for (row, bytes) in rows.iter_mut().enumerate() {
            // Positive-height BMP stores rows bottom first. The decoded texture uses top first.
            let side_component = projection(EDGE - 1 - row) * side;
            let (pixels, _) = bytes.as_chunks_mut::<3>();
            for (column, pixel) in pixels.iter_mut().enumerate() {
                let diffuse = (projection(column) * center + side_component).clamp(0.0, 1.0);
                pixel.copy_from_slice(&self.ramp_bgr[(diffuse * 255.0).round() as usize]);
            }
        }
        &self.bmp
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn projected_normals_follow_the_sweep_without_a_seam() {
        let plane = LightPlane::new();
        let mut lookup = DiffuseLookup::new();
        for elapsed in [0, SWEEP_NS / 2, SWEEP_NS] {
            let angle = angle(elapsed);
            let (side, center) = f32::from(angle).to_radians().sin_cos();
            let towards = plane.center * center + plane.side * side;
            let bmp = lookup.update(angle);
            let red = |normal: Vec3| {
                let [x, y] = plane
                    .coordinates(normal)
                    .map(|uv| (uv * EDGE as f32) as usize);
                bmp[BMP_HEADER_BYTES + ((EDGE - 1 - y) * EDGE + x) * 3 + 2]
            };
            assert!(red(towards) >= 206);
            assert!(red(-towards) < 90);
            let facing_right = red(plane.side);
            let facing_left = red(-plane.side);
            assert_eq!(facing_right.cmp(&facing_left), angle.cmp(&0));
        }
    }

    #[test]
    fn light_alternates_every_fifteen_seconds_with_a_three_second_swap() {
        let first = SWEEP_NS + SWAP_INTERVAL_NS;
        assert_eq!(angle_index(-LIMIT_DEGREES), 0);
        assert_eq!(angle_index(LIMIT_DEGREES), ANGLE_COUNT - 1);
        assert_eq!(angle(first - 1), LIMIT_DEGREES);
        let mut markers = presentation_markers().skip(3);
        for cycle in 0..4 {
            let start = first + cycle * SWAP_INTERVAL_NS;
            let from = if cycle % 2 == 0 {
                LIMIT_DEGREES
            } else {
                -LIMIT_DEGREES
            };
            assert_eq!(angle(start), from);
            assert_eq!(angle(start + SWAP_DURATION_NS / 2), 0);
            assert_eq!(angle(start + SWAP_DURATION_NS), -from);
            assert_eq!(angle(start + SWAP_INTERVAL_NS - 1), -from);
            assert_eq!(markers.next(), Some(start));
            assert_eq!(markers.next(), Some(start + SWAP_DURATION_NS / 2));
            assert_eq!(markers.next(), Some(start + SWAP_DURATION_NS));
        }
    }
}
