use glam::Vec3;

use crate::hair_rope::MAX_SEGMENTS;

const MAX_POINTS: usize = MAX_SEGMENTS + 2;
const MAX_NEIGHBORS: usize = 16;
const ROOT_X_RANGE: f32 = 0.024;
const ROOT_Z_RANGE: f32 = 0.055;
const ROOT_Y_RANGE: f32 = 0.13;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum HairLook {
    #[default]
    Locks,
    Ribbon,
    SoftMerge,
    StrongMerge,
}

impl HairLook {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Locks => "Locks",
            Self::Ribbon => "Flat ribbons",
            Self::SoftMerge => "Soft depth merge",
            Self::StrongMerge => "Strong depth merge",
        }
    }

    pub(crate) fn cycle(self, backward: bool) -> Self {
        let looks = [
            Self::Locks,
            Self::Ribbon,
            Self::SoftMerge,
            Self::StrongMerge,
        ];
        let step = if backward { looks.len() - 1 } else { 1 };
        looks[(self as usize + step) % looks.len()]
    }

    fn merge(self) -> Option<MergeProfile> {
        match self {
            Self::Locks | Self::Ribbon => None,
            Self::SoftMerge => Some(MergeProfile {
                lateral_range: 0.011,
                depth_range: 0.020,
                tangent_cosine: 0.939_692_6,    // 20°
                depth_axis_cosine: 0.866_025_4, // 30°
                maximum_shift: 0.005,
                strength: 0.7,
            }),
            Self::StrongMerge => Some(MergeProfile {
                lateral_range: 0.016,
                depth_range: 0.032,
                tangent_cosine: 0.819_152_06,    // 35°
                depth_axis_cosine: 0.707_106_77, // 45°
                maximum_shift: 0.010,
                strength: 1.0,
            }),
        }
    }
}

#[derive(Clone, Copy)]
struct MergeProfile {
    lateral_range: f32,
    depth_range: f32,
    tangent_cosine: f32,
    depth_axis_cosine: f32,
    maximum_shift: f32,
    strength: f32,
}

#[derive(Clone, Copy)]
struct GuideCurve {
    points: [Vec3; MAX_POINTS],
    count: usize,
}

pub(crate) struct DepthBlend {
    neighbors: Vec<Vec<usize>>,
    curves: Vec<GuideCurve>,
}

impl DepthBlend {
    pub(crate) fn new(roots: &[Vec3]) -> Self {
        let neighbors = roots
            .iter()
            .enumerate()
            .map(|(index, &root)| {
                let mut candidates: Vec<_> = roots
                    .iter()
                    .enumerate()
                    .filter_map(|(other, &position)| {
                        let offset = (position - root).abs();
                        (other != index
                            && offset.x < ROOT_X_RANGE
                            && offset.y < ROOT_Y_RANGE
                            && offset.z < ROOT_Z_RANGE)
                            .then(|| (other, offset.dot(Vec3::new(2.0, 0.15, 1.0))))
                    })
                    .collect();
                candidates.sort_by(|(_, a), (_, b)| a.total_cmp(b));
                candidates
                    .into_iter()
                    .take(MAX_NEIGHBORS)
                    .map(|(index, _)| index)
                    .collect()
            })
            .collect();
        Self {
            neighbors,
            curves: Vec::with_capacity(roots.len()),
        }
    }

    pub(crate) fn clear(&mut self) {
        self.curves.clear();
    }

    pub(crate) fn push(&mut self, points: &[Vec3]) {
        let mut curve = GuideCurve {
            points: [Vec3::ZERO; MAX_POINTS],
            count: points.len(),
        };
        curve.points[..curve.count].copy_from_slice(points);
        self.curves.push(curve);
    }

    pub(crate) fn for_each_curve(&self, look: HairLook, mut emit: impl FnMut(usize, &mut [Vec3])) {
        let merge = look.merge();
        for (index, curve) in self.curves.iter().enumerate() {
            let mut scratch = curve.points;
            let points = &mut scratch[..curve.count];
            if let Some(profile) = merge
                && curve.count >= 4
            {
                let end = curve.count - 1;
                for (knot, point) in points.iter_mut().enumerate().take(end).skip(2) {
                    let progress = (knot - 1) as f32 / (end - 1) as f32;
                    let tangent =
                        (curve.points[knot + 1] - curve.points[knot - 1]).normalize_or_zero();
                    point.z += self.depth_shift(index, *point, tangent, progress, profile);
                }
            }
            emit(index, points);
        }
    }

    fn depth_shift(
        &self,
        index: usize,
        point: Vec3,
        tangent: Vec3,
        progress: f32,
        profile: MergeProfile,
    ) -> f32 {
        let mut offset_sum = 0.0;
        let mut weight_sum = 1.0; // include this guide so weak matches cannot pull it far
        for &neighbor in &self.neighbors[index] {
            let Some(curve) = self.curves.get(neighbor) else {
                continue;
            };
            if curve.count < 3 {
                continue;
            }
            let (neighbor_point, neighbor_tangent) = sample_guide(curve, progress);
            let offset = neighbor_point - point;
            let depth_alignment = offset.z.abs() / offset.x.hypot(offset.z).max(1e-6);
            let axis_weight = smoothstep(profile.depth_axis_cosine, 1.0, depth_alignment);
            let tangent_weight =
                smoothstep(profile.tangent_cosine, 1.0, tangent.dot(neighbor_tangent));
            // Small lower-lock offsets in Y are fine. Only Z receives a correction.
            let lateral_distance = offset.x.hypot(0.35 * offset.y);
            let lateral_weight = 1.0 - smoothstep(0.0, profile.lateral_range, lateral_distance);
            let depth_weight = 1.0 - smoothstep(0.005, profile.depth_range, offset.z.abs());
            let weight = axis_weight * tangent_weight * lateral_weight * depth_weight;
            offset_sum += weight * offset.z;
            weight_sum += weight;
        }
        let envelope = smoothstep(0.05, 0.32, progress) * (1.0 - smoothstep(0.82, 1.0, progress));
        (profile.strength * offset_sum / weight_sum)
            .clamp(-profile.maximum_shift, profile.maximum_shift)
            * envelope
    }
}

fn sample_guide(curve: &GuideCurve, progress: f32) -> (Vec3, Vec3) {
    let end = curve.count - 1;
    let position = 1.0 + progress * (end - 1) as f32;
    let from = (position as usize).min(end - 1);
    let point = curve.points[from].lerp(curve.points[from + 1], position - from as f32);
    let tangent =
        (curve.points[from + 1] - curve.points[from.saturating_sub(1).max(1)]).normalize_or_zero();
    (point, tangent)
}

fn smoothstep(start: f32, end: f32, value: f32) -> f32 {
    let t = ((value - start) / (end - start)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn matching_paths(a: &[Vec3; 7], b: &[Vec3; 7], look: HairLook) -> [[Vec3; 7]; 2] {
        let mut blend = DepthBlend::new(&[a[0], b[0]]);
        blend.push(a);
        blend.push(b);
        let mut curves = [[Vec3::ZERO; 7]; 2];
        blend.for_each_curve(look, |index, points| curves[index].copy_from_slice(points));
        curves
    }

    fn vertical_guide() -> [Vec3; 7] {
        std::array::from_fn(|index| Vec3::NEG_Y * 0.060 * index as f32)
    }

    #[test]
    fn depth_neighbors_blend_only_in_z_between_the_anchors_and_tips() {
        let a = vertical_guide();
        let b = a.map(|point| point + Vec3::Z * 0.014);
        assert_eq!(matching_paths(&a, &b, HairLook::Ribbon), [a, b]);
        let soft = matching_paths(&a, &b, HairLook::SoftMerge);
        let strong = matching_paths(&a, &b, HairLook::StrongMerge);
        assert!(soft[0][3].z > a[3].z);
        assert!(strong[0][3].z > soft[0][3].z);
        assert!(strong[1][3].z < soft[1][3].z);
        assert!(strong[0][3].z <= 0.01);
        for (before, after) in [a, b].iter().zip(strong.iter()) {
            assert_eq!(before[..2], after[..2]);
            assert_eq!(before[6], after[6]);
            for (before, after) in before.iter().zip(after) {
                assert_eq!(before.truncate(), after.truncate());
            }
        }
    }

    #[test]
    fn sideways_crossing_and_distant_guides_are_ignored() {
        let a = vertical_guide();
        let sideways = a.map(|point| point + Vec3::new(0.015, 0.0, 0.006));
        let far = a.map(|point| point + Vec3::Z * 0.038);
        let mut crossing = a;
        for (point, z) in crossing
            .iter_mut()
            .zip([0.012, 0.012, -0.055, 0.008, 0.065, 0.065, 0.065])
        {
            point.z = z;
        }
        assert_eq!(matching_paths(&a, &sideways, HairLook::StrongMerge)[0], a);
        assert_eq!(matching_paths(&a, &far, HairLook::StrongMerge)[0], a);
        assert_eq!(
            matching_paths(&a, &crossing, HairLook::StrongMerge)[0][3],
            a[3]
        );
    }
}
