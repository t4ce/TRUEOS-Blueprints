use anyhow::{Context, Result, ensure};
use glam::{Mat3, Vec2, Vec3};

use super::sky_light::{encode_rgb, linear_rgb};

const CURVE: &[u8; 72 * 12] = include_bytes!("../../assets/skirt-heart-curve.bin");
const SIDES: usize = 12;
const RADIUS: f32 = 0.0011;
const GOLD: [u8; 3] = [202, 161, 81];
const ROUGHNESS: f32 = 0.18;
const PEAK: f32 = 1.8;
pub(super) const GOLD_RAMP_WIDTH: usize = 96;

struct Mount {
    triangle: [usize; 3],
    weights: Vec3,
}

impl Mount {
    fn at(positions: &[[f32; 3]], indices: &[u32], center: Vec2) -> Result<Self> {
        indices
            .as_chunks::<3>()
            .0
            .iter()
            .filter_map(|triangle| {
                let triangle = triangle.map(|index| index as usize);
                let [a, b, c] = triangle.map(|index| Vec3::from_array(positions[index]));
                let (ab, ac) = ((b - a).truncate(), (c - a).truncate());
                let area = ab.perp_dot(ac);
                if area.abs() < 1.0e-8 {
                    return None;
                }
                let offset = center - a.truncate();
                let v = offset.perp_dot(ac) / area;
                let w = ab.perp_dot(offset) / area;
                let weights = Vec3::new(1.0 - v - w, v, w);
                (weights.min_element() >= -1.0e-5).then_some((
                    weights.dot(Vec3::new(a.z, b.z, c.z)),
                    Self { triangle, weights },
                ))
            })
            // -Z is the front of the authored skirt.
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .map(|(_, mount)| mount)
            .context("heart center is outside the skirt")
    }

    fn frame(&self, positions: &[[f32; 3]]) -> Result<(Vec3, Mat3)> {
        let [a, b, c] = self
            .triangle
            .map(|index| Vec3::from_array(positions[index]));
        let x = (b - a)
            .try_normalize()
            .context("collapsed skirt heart mount")?;
        let z = x
            .cross(c - a)
            .try_normalize()
            .context("collapsed skirt heart mount")?;
        let origin = a * self.weights.x + b * self.weights.y + c * self.weights.z;
        Ok((origin, Mat3::from_cols(x, z.cross(x), z)))
    }
}

pub(super) struct SkirtHeart {
    mount: Mount,
    vertices: Vec<(Vec3, Vec3)>,
    indices: Vec<u32>,
}

impl SkirtHeart {
    pub(super) fn new(positions: &[[f32; 3]], indices: &[u32]) -> Result<Self> {
        let centers: Vec<_> = CURVE
            .as_chunks::<12>()
            .0
            .iter()
            .map(|sample| {
                Vec3::from_array(std::array::from_fn(|axis| {
                    f32::from_le_bytes(sample[axis * 4..axis * 4 + 4].try_into().unwrap())
                }))
            })
            .collect();
        ensure!(
            centers.iter().all(|point| point.is_finite()),
            "nonfinite skirt heart curve"
        );
        let rings = centers.len();
        let centroid = centers.iter().copied().sum::<Vec3>() / rings as f32;
        let mount = Mount::at(positions, indices, centroid.truncate())?;
        let (origin, basis) = mount.frame(positions)?;
        let inverse = basis.transpose();
        let cross_section: [_; SIDES] = std::array::from_fn(|side| {
            (std::f32::consts::TAU * side as f32 / SIDES as f32).sin_cos()
        });
        let mut vertices = Vec::with_capacity(rings * SIDES);
        let mut indices = Vec::with_capacity(rings * SIDES * 6);
        for (ring, &center) in centers.iter().enumerate() {
            let next = (ring + 1) % rings;
            let previous = (ring + rings - 1) % rings;
            let tangent = (centers[next] - centers[previous])
                .try_normalize()
                .context("degenerate skirt heart curve tangent")?;
            let side = tangent
                .cross(Vec3::NEG_Z)
                .try_normalize()
                .context("invalid skirt heart cross-section")?;
            let front = side.cross(tangent);
            for (corner, &(sin, cos)) in cross_section.iter().enumerate() {
                let normal = front * cos + side * sin;
                let position = center + RADIUS * normal;
                vertices.push((inverse * (position - origin), inverse * normal));

                let right = (corner + 1) % SIDES;
                let [a, b, c, d] = [
                    ring * SIDES + corner,
                    ring * SIDES + right,
                    next * SIDES + corner,
                    next * SIDES + right,
                ]
                .map(|index| index as u32);
                indices.extend([a, b, c, c, b, d]);
            }
        }
        Ok(Self {
            mount,
            vertices,
            indices,
        })
    }

    pub(super) fn vertex_count(&self) -> usize {
        self.vertices.len()
    }

    pub(super) fn indices(&self) -> &[u32] {
        &self.indices
    }

    pub(super) fn vertices<'a>(
        &'a self,
        skirt: &'a [[f32; 3]],
    ) -> Result<impl ExactSizeIterator<Item = (Vec3, Vec3)> + 'a> {
        let (origin, basis) = self.mount.frame(skirt)?;
        Ok(self
            .vertices
            .iter()
            .map(move |&(position, normal)| (origin + basis * position, basis * normal)))
    }
}

pub(super) fn gold_ramp() -> [[u8; 4]; GOLD_RAMP_WIDTH] {
    let color = linear_rgb(GOLD);
    std::array::from_fn(|i| {
        let energy = PEAK * i as f32 / (GOLD_RAMP_WIDTH - 1) as f32;
        let [r, g, b] = encode_rgb(color * energy);
        [r, g, b, 255]
    })
}

pub(super) fn gold_column(normal: Vec3, key: Vec3, half: Vec3, view: Vec3) -> f32 {
    let facing = normal.dot(view).clamp(0.0, 1.0);
    let direct = normal.dot(key).max(0.0);
    let highlight = normal
        .dot(half)
        .clamp(0.0, 1.0)
        .powf(2.0 / (ROUGHNESS * ROUGHNESS));
    let rim = 0.23 * (1.0 - facing).powi(3);
    let energy = 0.35 + 0.40 * direct + rim + 1.40 * highlight * direct;
    0.5 + (GOLD_RAMP_WIDTH - 1) as f32 * (energy / PEAK).clamp(0.0, 1.0)
}
