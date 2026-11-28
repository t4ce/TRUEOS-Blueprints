use glam::{Quat, Vec2, Vec3};

use crate::raster::{Texture, Vertex};

// The full width is at the two edges, with two shoulders on each face.
const SECTION: [Vec2; 6] = [
    Vec2::new(1.0, 0.0),
    Vec2::new(0.44, 0.90),
    Vec2::new(-0.44, 0.90),
    Vec2::new(-1.0, 0.0),
    Vec2::new(-0.44, -0.90),
    Vec2::new(0.44, -0.90),
];
const SIDES: u32 = SECTION.len() as u32;
const MIN_LINK_LENGTH: f32 = 1e-5;
const MAX_CURVE_OFFSET: f32 = 0.007;

#[derive(Clone, Copy)]
pub(crate) struct LockProfile {
    pub(crate) half_width: f32,
    pub(crate) half_depth: f32,
    pub(crate) color_shift: f32,
}

pub(crate) fn lock_texture(stops: &[(f32, [u8; 3]); 5]) -> Texture {
    const WIDTH: u32 = 17;
    const HEIGHT: u32 = 129;
    let mut pixels = Vec::with_capacity((WIDTH * HEIGHT) as usize);
    for row in 0..HEIGHT {
        let progress = row as f32 / (HEIGHT - 1) as f32;
        let color = lock_color(stops, progress);
        for column in 0..WIDTH {
            let across = 2.0 * column as f32 / (WIDTH - 1) as f32 - 1.0;
            let shade = 1.0 - 0.07 * across * across;
            let [red, green, blue] = color.map(|channel| (channel * shade).round() as u8);
            pixels.push([red, green, blue, 255]);
        }
    }
    Texture {
        width: WIDTH,
        height: HEIGHT,
        pixels,
    }
}

fn lock_color(stops: &[(f32, [u8; 3]); 5], progress: f32) -> [f32; 3] {
    let upper = stops
        .partition_point(|&(position, _)| position <= progress)
        .clamp(1, stops.len() - 1);
    let (start, lower_color) = stops[upper - 1];
    let (end, upper_color) = stops[upper];
    let blend = smooth_ramp((progress - start) / (end - start));
    std::array::from_fn(|channel| {
        let lower = f32::from(lower_color[channel]);
        let upper = f32::from(upper_color[channel]);
        lower + (upper - lower) * blend
    })
}

fn smooth_ramp(position: f32) -> f32 {
    let position = position.clamp(0.0, 1.0);
    position * position * (3.0 - 2.0 * position)
}

#[derive(Default)]
pub(crate) struct HairHull {
    pub(crate) vertices: Vec<Vertex>,
    pub(crate) triangles: Vec<[u32; 3]>,
}

impl HairHull {
    pub(crate) fn clear(&mut self) {
        self.vertices.clear();
        self.triangles.clear();
    }

    pub(crate) fn append_lock(&mut self, points: &[Vec3], root_normal: Vec3, profile: LockProfile) {
        let total_length: f32 = points
            .windows(2)
            .map(|pair| pair[0].distance(pair[1]))
            .filter(|length| *length >= MIN_LINK_LENGTH)
            .sum();
        if total_length < MIN_LINK_LENGTH || profile.half_width <= 0.0 || profile.half_depth <= 0.0
        {
            return;
        }

        let first_ring = self.vertices.len() as u32;
        let mut frame = None;
        let mut distance = 0.0;
        for (link, pair) in points.windows(2).enumerate() {
            let [start, end] = [pair[0], pair[1]];
            let chord = end - start;
            let length = chord.length();
            if length < MIN_LINK_LENGTH {
                continue;
            }
            let direction = chord / length;
            let start_tangent = guide_tangent(points, link, direction);
            let end_tangent = guide_tangent(points, link + 1, direction);
            let curve = Frame::transport(frame, root_normal, start_tangent);
            self.append_ring(first_ring, start, curve, profile, distance / total_length);

            // Sample the middle of a cubic through the two guide points. Limit
            // the offset near contacts so this small render-only curve stays
            // close to the simulated guide.
            let offset = (start_tangent - end_tangent) * (0.125 * length);
            let midpoint = start.lerp(end, 0.5)
                + offset.clamp_length_max(MAX_CURVE_OFFSET.min(0.125 * length));
            let midpoint_tangent = (1.5 * direction - 0.25 * (start_tangent + end_tangent))
                .try_normalize()
                .unwrap_or(direction);
            let curve = Frame::transport(Some(curve), root_normal, midpoint_tangent);
            self.append_ring(
                first_ring,
                midpoint,
                curve,
                profile,
                (distance + 0.5 * length) / total_length,
            );
            frame = Some(curve);
            distance += length;
        }

        let tip = self.vertices.len() as u32;
        let end = *points.last().unwrap();
        self.vertices.push(Vertex {
            position: end,
            normal: frame.unwrap().tangent,
            uv: Vec2::new(0.5, 1.0),
        });
        let last_ring = tip - SIDES;
        for side in 0..SIDES {
            self.triangles
                .push([last_ring + side, last_ring + (side + 1) % SIDES, tip]);
        }
    }

    fn append_ring(
        &mut self,
        first_ring: u32,
        center: Vec3,
        frame: Frame,
        profile: LockProfile,
        progress: f32,
    ) {
        let taper = (1.0 - progress).max(0.0);
        let half_width = profile.half_width * taper.sqrt() * (1.0 + 0.30 * progress);
        let half_depth = profile.half_depth * taper * (1.0 - 0.55 * progress);
        let color_progress =
            (progress + 4.0 * progress * taper * profile.color_shift).clamp(0.0, 1.0);
        let depth_axis = frame.tangent.cross(frame.width_axis);
        let ring = self.vertices.len() as u32;
        for point in SECTION {
            let side = frame.width_axis * point.x;
            let depth = depth_axis * point.y;
            self.vertices.push(Vertex {
                position: center + half_width * side + half_depth * depth,
                normal: (half_depth * side + half_width * depth).normalize(),
                uv: Vec2::new(0.5 * (point.x + 1.0), color_progress),
            });
        }

        if ring == first_ring {
            for side in 1..SIDES - 1 {
                self.triangles.push([ring, ring + side + 1, ring + side]);
            }
        } else {
            let previous = ring - SIDES;
            for side in 0..SIDES {
                let next_side = (side + 1) % SIDES;
                self.triangles
                    .push([previous + side, previous + next_side, ring + side]);
                self.triangles
                    .push([previous + next_side, ring + next_side, ring + side]);
            }
        }
    }
}

#[derive(Clone, Copy)]
struct Frame {
    tangent: Vec3,
    width_axis: Vec3,
}

impl Frame {
    fn transport(previous: Option<Self>, root_normal: Vec3, tangent: Vec3) -> Self {
        let width_axis = previous.map_or_else(
            || {
                root_normal
                    .cross(tangent)
                    .try_normalize()
                    .unwrap_or_else(|| tangent.any_orthonormal_vector())
            },
            |frame| Quat::from_rotation_arc(frame.tangent, tangent) * frame.width_axis,
        );
        // The projection keeps the section perpendicular after many short arcs.
        let width_axis = (width_axis - tangent * width_axis.dot(tangent)).normalize();
        Self {
            tangent,
            width_axis,
        }
    }
}

fn guide_tangent(points: &[Vec3], index: usize, fallback: Vec3) -> Vec3 {
    let point = points[index];
    let incoming = index
        .checked_sub(1)
        .map_or(Vec3::ZERO, |previous| point - points[previous]);
    let outgoing = points
        .get(index + 1)
        .map_or(Vec3::ZERO, |&next| next - point);
    let incoming = usable_direction(incoming);
    let outgoing = usable_direction(outgoing);
    (incoming + outgoing).try_normalize().unwrap_or(fallback)
}

fn usable_direction(offset: Vec3) -> Vec3 {
    let length = offset.length();
    if length >= MIN_LINK_LENGTH {
        offset / length
    } else {
        Vec3::ZERO
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BASE_PROFILE: LockProfile = LockProfile {
        half_width: 0.012,
        half_depth: 0.006,
        color_shift: 0.0,
    };

    #[test]
    fn lock_reuses_one_tip_and_keeps_outward_winding() {
        let mut hull = HairHull::default();
        let tip = Vec3::new(0.04, -0.30, 0.01);
        let points = [Vec3::ZERO, Vec3::new(0.01, -0.14, 0.02), tip];
        hull.append_lock(&points, Vec3::Z, BASE_PROFILE);

        assert_eq!(hull.vertices.len(), 4 * SIDES as usize + 1);
        assert_eq!(hull.triangles.len(), 46);
        assert_eq!(
            hull.vertices
                .iter()
                .filter(|vertex| vertex.position == tip)
                .count(),
            1
        );
        assert!(hull.vertices.iter().all(|vertex| {
            vertex.position.is_finite() && (vertex.normal.length_squared() - 1.0).abs() < 1e-4
        }));
        for triangle in &hull.triangles[4..hull.triangles.len() - 6] {
            let [a, b, c] = triangle.map(|index| &hull.vertices[index as usize]);
            let face = (b.position - a.position).cross(c.position - a.position);
            assert!(face.dot(a.normal + b.normal + c.normal) > 0.0);
        }

        let capacity = hull.vertices.capacity();
        hull.clear();
        assert!(hull.vertices.is_empty() && hull.triangles.is_empty());
        assert_eq!(hull.vertices.capacity(), capacity);
    }

    #[test]
    fn coincident_control_points_keep_the_end_and_a_finite_frame() {
        let mut hull = HairHull::default();
        let tip = Vec3::NEG_Y * 0.4;
        hull.append_lock(&[Vec3::ZERO; 3], Vec3::NEG_Y, BASE_PROFILE);
        assert!(hull.vertices.is_empty());
        hull.append_lock(
            &[Vec3::ZERO, Vec3::ZERO, tip, tip],
            Vec3::NEG_Y,
            BASE_PROFILE,
        );
        assert_eq!(hull.vertices.len(), 2 * SIDES as usize + 1);
        assert_eq!(hull.vertices.last().unwrap().position, tip);
        assert!(hull.vertices.iter().all(|vertex| vertex.normal.is_finite()));
    }

    #[test]
    fn width_and_depth_scale_their_own_cross_sections() {
        let tip = Vec3::NEG_Y * 0.4;
        let points = [Vec3::ZERO, tip];
        let mut locks = HairHull::default();
        locks.append_lock(&points, Vec3::Z, BASE_PROFILE);
        for (width, depth) in [(0.7, 1.0), (1.0, 0.7), (0.7, 0.7)] {
            let mut scaled = HairHull::default();
            scaled.append_lock(
                &points,
                Vec3::Z,
                LockProfile {
                    half_width: BASE_PROFILE.half_width * width,
                    half_depth: BASE_PROFILE.half_depth * depth,
                    ..BASE_PROFILE
                },
            );
            assert_eq!(locks.triangles, scaled.triangles);
            for (before, after) in locks.vertices.iter().zip(&scaled.vertices) {
                assert!(
                    after
                        .position
                        .distance(before.position * Vec3::new(width, 1.0, depth))
                        < 1e-6
                );
                assert_eq!(before.uv, after.uv);
            }
            assert_eq!(scaled.vertices.last().unwrap().position, tip);
        }
    }

    #[test]
    fn lock_ramp_blends_the_palette_smoothly_and_darkens_the_edges() {
        let stops = [
            (0.0, [40, 32, 55]),
            (0.25, [72, 64, 87]),
            (0.5, [96, 84, 117]),
            (0.75, [80, 72, 96]),
            (1.0, [48, 40, 63]),
        ];
        assert_eq!(lock_color(&stops, 0.0625), [45.0, 37.0, 60.0]);
        let texture = lock_texture(&stops);
        for (progress, [red, green, blue]) in stops {
            let row = (progress * (texture.height - 1) as f32) as usize;
            let start = row * texture.width as usize;
            let center = texture.pixels[start + texture.width as usize / 2];
            assert_eq!(center, [red, green, blue, 255]);
            assert!(texture.pixels[start][0] < center[0]);
            assert_eq!(
                texture.pixels[start],
                texture.pixels[start + texture.width as usize - 1]
            );
        }
        assert!(texture.pixels.iter().all(|pixel| pixel[3] == 255));
    }

    #[test]
    fn color_shift_preserves_the_hull_and_anchors_both_ends_of_the_ramp() {
        let points = [Vec3::ZERO, Vec3::NEG_Y * 0.4];
        let mut original = HairHull::default();
        original.append_lock(&points, Vec3::Z, BASE_PROFILE);
        let mut tinted = HairHull::default();
        tinted.append_lock(
            &points,
            Vec3::Z,
            LockProfile {
                color_shift: 0.08,
                ..BASE_PROFILE
            },
        );

        assert_eq!(original.triangles, tinted.triangles);
        assert_eq!(original.vertices.len(), tinted.vertices.len());
        for (before, after) in original.vertices.iter().zip(&tinted.vertices) {
            assert_eq!(before.position, after.position);
            assert_eq!(before.normal, after.normal);
            assert_eq!(before.uv.x, after.uv.x);
        }
        assert_eq!(tinted.vertices[0].uv.y, 0.0);
        assert!((tinted.vertices[SIDES as usize].uv.y - 0.58).abs() < 1e-6);
        assert_eq!(tinted.vertices.last().unwrap().uv.y, 1.0);
    }
}
