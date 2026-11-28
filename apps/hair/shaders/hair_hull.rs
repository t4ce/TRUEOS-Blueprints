use glam::{Quat, Vec2, Vec3};

use crate::raster::{Texture, Vertex};

const SECTION_VERTICES: u32 = 4;
const STRIP_OFFSETS: [u32; 2] = [0, 2];
const MIN_LINK_LENGTH: f32 = 1e-5;
const OUTLINE_COLOR: [f32; 3] = [9.0, 7.0, 12.0];

#[derive(Clone, Copy)]
pub(crate) struct LockProfile {
    pub(crate) half_width: f32,
    pub(crate) half_depth: f32,
    pub(crate) color_top: f32,
    pub(crate) color_span: f32,
}

impl LockProfile {
    fn palette_height(self, position: Vec3) -> f32 {
        (self.color_top - position.y) / self.color_span.max(MIN_LINK_LENGTH)
    }
}

pub(crate) fn lock_texture(stops: &[(f32, [u8; 3]); 5]) -> Texture {
    const WIDTH: u32 = 65;
    const HEIGHT: u32 = 129;
    let mut pixels = Vec::with_capacity((WIDTH * HEIGHT) as usize);
    for row in 0..HEIGHT {
        let progress = row as f32 / (HEIGHT - 1) as f32;
        let color = lock_color(stops, progress);
        for column in 0..WIDTH {
            let across = column as f32 / (WIDTH - 1) as f32;
            let edge = across.min(1.0 - across);
            // The same texture lookup shades the narrow edge and the flat fill.
            let fill = smooth_ramp((edge - 0.025) / 0.055);
            let [red, green, blue] = std::array::from_fn(|channel| {
                let outline = OUTLINE_COLOR[channel];
                (outline + (color[channel] - outline) * fill).round() as u8
            });
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
        if profile.half_width <= 0.0 || profile.half_depth <= 0.0 {
            return;
        }

        let total_length: f32 = points
            .windows(2)
            .map(|pair| pair[0].distance(pair[1]))
            .filter(|&length| length >= MIN_LINK_LENGTH)
            .sum();
        if total_length < MIN_LINK_LENGTH {
            return;
        }

        let first_section = self.vertices.len() as u32;
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
            let curve = Frame::transport(frame, root_normal, start_tangent);
            self.append_section(first_section, start, curve, profile, distance / total_length);
            frame = Some(curve);
            distance += length;
        }

        let tip = self.vertices.len() as u32;
        let end = *points.last().unwrap();
        self.vertices.push(Vertex {
            position: end,
            normal: frame.unwrap().tangent,
            uv: Vec2::new(0.5, profile.palette_height(end)),
        });
        let last_section = tip - SECTION_VERTICES;
        for strip in STRIP_OFFSETS {
            let side = last_section + strip;
            self.triangles.push([side, side + 1, tip]);
        }
    }

    fn append_section(
        &mut self,
        first_section: u32,
        center: Vec3,
        frame: Frame,
        profile: LockProfile,
        progress: f32,
    ) {
        let taper = (1.0 - progress).max(0.0);
        let half_width = profile.half_width * taper.sqrt() * (1.0 + 0.30 * progress);
        let half_depth = profile.half_depth * taper * (1.0 - 0.55 * progress);
        let depth_axis = frame.tangent.cross(frame.width_axis);
        let section = self.vertices.len() as u32;
        for (offset, normal) in [
            (half_width * frame.width_axis, -depth_axis),
            (half_depth * depth_axis, frame.width_axis),
        ] {
            for (position, across) in [(center - offset, 0.0), (center + offset, 1.0)] {
                self.vertices.push(Vertex {
                    position,
                    normal,
                    uv: Vec2::new(across, profile.palette_height(position)),
                });
            }
        }

        if section != first_section {
            for strip in STRIP_OFFSETS {
                let next = section + strip;
                let previous = next - SECTION_VERTICES;
                self.triangles.push([previous, previous + 1, next]);
                self.triangles.push([previous + 1, next + 1, next]);
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
        color_top: 0.0,
        color_span: 0.8,
    };

    #[test]
    fn two_open_strips_reuse_one_exact_tip() {
        let mut hull = HairHull::default();
        let tip = Vec3::new(0.04, -0.30, 0.01);
        let points = [Vec3::ZERO, Vec3::new(0.01, -0.14, 0.02), tip];
        hull.append_lock(&points, Vec3::Z, BASE_PROFILE);

        assert_eq!(hull.vertices.len(), 2 * SECTION_VERTICES as usize + 1);
        assert_eq!(hull.triangles.len(), 6);
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
        assert_eq!(hull.triangles[4..], [[4, 5, 8], [6, 7, 8]]);

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
        assert_eq!(hull.vertices.len(), SECTION_VERTICES as usize + 1);
        assert_eq!(hull.triangles.len(), 2);
        assert_eq!(hull.vertices.last().unwrap().position, tip);
        assert!(hull.vertices.iter().all(|vertex| vertex.normal.is_finite()));
    }

    #[test]
    fn strips_cross_at_each_usable_knot() {
        let points = std::array::from_fn::<_, 11, _>(|index| {
            let after_bend = index.saturating_sub(5) as f32;
            Vec3::new(0.05 * after_bend, -0.08 * index as f32, 0.0)
        });
        let mut hull = HairHull::default();
        hull.append_lock(&points, Vec3::Z, BASE_PROFILE);

        let links = points.len() - 1;
        assert_eq!(hull.vertices.len(), links * SECTION_VERTICES as usize + 1);
        assert_eq!(hull.triangles.len(), links * 4 - 2);
        for (&point, section) in points
            .iter()
            .zip(hull.vertices.chunks_exact(SECTION_VERTICES as usize))
        {
            let broad = section[1].position - section[0].position;
            let narrow = section[3].position - section[2].position;
            assert!(broad.dot(narrow).abs() < 1e-7);
            for strip in section.chunks_exact(2) {
                let center = (strip[0].position + strip[1].position) * 0.5;
                assert!(center.distance(point) < 1e-6);
                assert_eq!([strip[0].uv.x, strip[1].uv.x], [0.0, 1.0]);
            }
        }
        assert_eq!(hull.vertices.last().unwrap().position, points[links]);
        assert!(hull.vertices.iter().all(|vertex| {
            vertex.position.is_finite() && (vertex.normal.length_squared() - 1.0).abs() < 1e-4
        }));
        for triangle in &hull.triangles {
            let [a, b, c] = triangle.map(|index| hull.vertices[index as usize].position);
            assert!((b - a).cross(c - a).length_squared() > 1e-16);
        }
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
    fn strip_texture_keeps_a_uniform_fill_inside_the_narrow_outline() {
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
            assert_eq!(texture.pixels[start], [9, 7, 12, 255]);
            assert!(texture.pixels[start + 3][0] > texture.pixels[start][0]);
            assert_eq!(texture.pixels[start + 6], center);
            assert_eq!(
                texture.pixels[start],
                texture.pixels[start + texture.width as usize - 1]
            );
        }
        assert!(texture.pixels.iter().all(|pixel| pixel[3] == 255));
    }

    #[test]
    fn changing_the_color_span_preserves_the_hull() {
        let points = [Vec3::ZERO, Vec3::NEG_Y * 0.2, Vec3::NEG_Y * 0.4];
        let mut original = HairHull::default();
        original.append_lock(&points, Vec3::Z, BASE_PROFILE);
        let mut tinted = HairHull::default();
        tinted.append_lock(
            &points,
            Vec3::Z,
            LockProfile {
                color_span: 0.4,
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
        assert_eq!(original.vertices[SECTION_VERTICES as usize].uv.y, 0.25);
        assert_eq!(tinted.vertices[SECTION_VERTICES as usize].uv.y, 0.5);
        assert_eq!(original.vertices.last().unwrap().uv.y, 0.5);
        assert_eq!(tinted.vertices.last().unwrap().uv.y, 1.0);
    }

    #[test]
    fn palette_projects_by_avatar_height_through_bends_and_different_roots() {
        let straight_points = [
            Vec3::ZERO,
            Vec3::new(0.0, -0.25, 0.0),
            Vec3::new(0.0, -1.0, 0.0),
        ];
        let bent_points = [
            Vec3::new(0.05, -0.05, 0.0),
            Vec3::new(0.15, -0.18, 0.1),
            Vec3::new(0.22, -0.25, 0.18),
        ];
        let mut long = HairHull::default();
        long.append_lock(&straight_points, Vec3::Z, BASE_PROFILE);
        let mut short = HairHull::default();
        short.append_lock(&bent_points, Vec3::Z, BASE_PROFILE);

        let short_tip = short.vertices.last().unwrap();
        assert_eq!(short_tip.position, bent_points[2]);
        assert_eq!(short_tip.uv.y, long.vertices[SECTION_VERTICES as usize].uv.y);
        assert_eq!(short_tip.uv.y, 0.25 / BASE_PROFILE.color_span);
        assert!(long.vertices.last().unwrap().uv.y > 1.0);
        for vertex in &short.vertices {
            assert_eq!(
                vertex.uv.y,
                (BASE_PROFILE.color_top - vertex.position.y) / BASE_PROFILE.color_span
            );
        }
    }
}
