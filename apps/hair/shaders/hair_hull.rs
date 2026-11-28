use glam::{Quat, Vec2, Vec3};

use crate::raster::{Texture, Vertex};

const SECTION_VERTICES: u32 = 4;
const STRIP_OFFSETS: [u32; 2] = [0, 2];
const MIN_LINK_LENGTH: f32 = 1e-5;
const MAX_TIP_MIDPOINT_OFFSET: f32 = 0.003;
const TIP_THREAD_SPLAY: f32 = 0.12;
const THREAD_ROOT_FRACTION: f32 = 2.0 / 3.0;
const HIGH_THREAD_OFFSET: f32 = 3.25;
const OUTLINE_COLOR: [f32; 3] = [9.0, 7.0, 12.0];
const OUTLINE_STRENGTH: f32 = 0.42;
const OUTLINE_WIDTH: f32 = 0.045;

#[derive(Clone, Copy)]
pub(crate) struct LockProfile {
    pub(crate) half_width: f32,
    pub(crate) half_depth: f32,
    pub(crate) tip_half_width: f32,
    pub(crate) color_top: f32,
    pub(crate) color_span: f32,
    pub(crate) thread_starts_earlier: bool,
    pub(crate) trim_main_tip: bool,
    pub(crate) high_offshoot: bool,
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
            // Blend a thin ink line into the fill so overlapping strips stay soft.
            let ink = OUTLINE_STRENGTH * (1.0 - smooth_ramp(edge / OUTLINE_WIDTH));
            let [red, green, blue] = std::array::from_fn(|channel| {
                (color[channel] + (OUTLINE_COLOR[channel] - color[channel]) * ink).round() as u8
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
    pub(crate) tips: Vec<HairTip>,
}

pub(crate) struct HairTip {
    points: [Vec3; 5],
    point_count: usize,
    threads: [[Vec3; 2]; 2],
    thread_count: usize,
}

impl HairTip {
    pub(crate) fn stroke_points(&self) -> &[Vec3] {
        &self.points[..self.point_count]
    }

    pub(crate) fn threads(&self) -> &[[Vec3; 2]] {
        &self.threads[..self.thread_count]
    }

    fn new(
        previous: Option<Vec3>,
        start: Vec3,
        split: Vec3,
        tip: Vec3,
        root_normal: Vec3,
        profile: LockProfile,
        high_root: Option<Vec3>,
    ) -> Self {
        let wide = usable_direction(split - start);
        let thin = usable_direction(tip - split);
        let incoming = previous.map_or(wide, |point| usable_direction(start - point));
        let start_tangent = (incoming + wide).try_normalize().unwrap_or(wide);
        let split_tangent = (wide + thin).try_normalize().unwrap_or(wide);
        let wide_middle = tip_midpoint(start, split, start_tangent, split_tangent);
        let thin_middle = tip_midpoint(split, tip, split_tangent, thin);
        let wide_root = wide_middle.lerp(split, 1.0 / 3.0);
        let target = thin_middle.lerp(tip, 0.8);
        // Keep the same endpoint and splay when the attachment moves up a link.
        let offset = (target - wide_root).cross(root_normal) * TIP_THREAD_SPLAY;
        let root = previous
            .filter(|_| profile.thread_starts_earlier)
            .map_or(wide_root, |point| point.lerp(start, THREAD_ROOT_FRACTION));
        let thread_end = target - offset;
        // The midpoint halves the thin stroke. The straight offshoot trims
        // back by half the length of the same solved link.
        let (point_count, thread_end) = if profile.trim_main_tip {
            (4, thread_end)
        } else {
            (5, thread_end.move_towards(root, split.distance(tip) * 0.5))
        };
        let high_thread = high_root.map(|root| [root, target + offset]);
        Self {
            points: [start, wide_middle, split, thin_middle, tip],
            point_count,
            threads: [[root, thread_end], high_thread.unwrap_or_default()],
            thread_count: 1 + usize::from(high_thread.is_some()),
        }
    }
}

fn high_thread_root(points: &[Vec3], tip_links: [usize; 3], staggered: bool) -> Vec3 {
    let [previous, start, split] = tip_links.map(|index| points[index]);
    let tip = points[points.len() - 1];
    let wide_length = start.distance(split);
    let thin_length = split.distance(tip);
    let lower_reach = thin_length + wide_length * (1.0 - THREAD_ROOT_FRACTION);
    let reach = if staggered {
        thin_length + wide_length + previous.distance(start) * (1.0 - THREAD_ROOT_FRACTION)
    } else {
        lower_reach
    };
    // Use the unstaggered reach as the scale for both attachment patterns.
    let mut distance = reach + HIGH_THREAD_OFFSET * lower_reach;
    for pair in points.windows(2).rev() {
        let [upper, lower] = [pair[0], pair[1]];
        let length = upper.distance(lower);
        if length >= MIN_LINK_LENGTH && distance <= length {
            return lower.lerp(upper, distance / length);
        }
        distance -= length;
    }
    points[0]
}

fn tip_midpoint(start: Vec3, end: Vec3, start_tangent: Vec3, end_tangent: Vec3) -> Vec3 {
    let chord = end - start;
    let midpoint = start.lerp(end, 0.5);
    let Some(direction) = chord.try_normalize() else {
        return midpoint;
    };
    // Use the lateral part of the Hermite midpoint offset. The midpoint
    // stays halfway along the chord and never strays far from the guide.
    let bend = start_tangent - end_tangent;
    let offset_scale = 0.125 * chord.length();
    let offset = (bend - direction * bend.dot(direction)) * offset_scale;
    midpoint + offset.clamp_length_max(MAX_TIP_MIDPOINT_OFFSET.min(offset_scale))
}

impl HairHull {
    pub(crate) fn clear(&mut self) {
        self.vertices.clear();
        self.triangles.clear();
        self.tips.clear();
    }

    pub(crate) fn append_lock(&mut self, points: &[Vec3], root_normal: Vec3, profile: LockProfile) {
        if profile.half_width <= 0.0 || profile.half_depth <= 0.0 {
            return;
        }

        let mut total_length = 0.0;
        let mut link_count = 0;
        let mut last_links = [0; 3];
        for (index, pair) in points.windows(2).enumerate() {
            let length = pair[0].distance(pair[1]);
            if length >= MIN_LINK_LENGTH {
                total_length += length;
                link_count += 1;
                last_links = [last_links[1], last_links[2], index];
            }
        }
        if link_count == 0 {
            return;
        }

        let tip = points[points.len() - 1];
        if link_count == 1 {
            let start = points[last_links[2]];
            self.tips.push(HairTip::new(
                None,
                start,
                start.lerp(tip, 2.0 / 3.0),
                tip,
                root_normal,
                profile,
                None,
            ));
            return;
        }
        let [previous, stroke_start, split] = last_links;
        self.tips.push(HairTip::new(
            (link_count > 2).then(|| points[previous]),
            points[stroke_start],
            points[split],
            tip,
            root_normal,
            profile,
            (profile.high_offshoot && link_count > 2)
                .then(|| high_thread_root(points, last_links, profile.thread_starts_earlier)),
        ));
        if link_count == 2 {
            return;
        }

        let first_section = self.vertices.len() as u32;
        let mut frame = None;
        let mut distance = 0.0;
        for (link, pair) in points.windows(2).take(stroke_start + 1).enumerate() {
            let [start, end] = [pair[0], pair[1]];
            let chord = end - start;
            let length = chord.length();
            if length < MIN_LINK_LENGTH {
                continue;
            }
            let direction = chord / length;
            let start_tangent = guide_tangent(points, link, direction);
            let curve = Frame::transport(frame, root_normal, start_tangent);
            let width_limit = if link == stroke_start {
                profile.tip_half_width
            } else {
                f32::INFINITY
            };
            self.append_section(
                first_section,
                start,
                curve,
                profile,
                distance / total_length,
                width_limit,
            );
            frame = Some(curve);
            distance += length;
        }
    }

    fn append_section(
        &mut self,
        first_section: u32,
        center: Vec3,
        frame: Frame,
        profile: LockProfile,
        progress: f32,
        width_limit: f32,
    ) {
        let taper = (1.0 - progress).max(0.0);
        let half_width =
            (profile.half_width * taper.sqrt() * (1.0 + 0.30 * progress)).min(width_limit);
        let half_depth = (profile.half_depth * taper * (1.0 - 0.55 * progress)).min(width_limit);
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
        tip_half_width: f32::INFINITY,
        color_top: 0.0,
        color_span: 0.8,
        thread_starts_earlier: false,
        trim_main_tip: false,
        high_offshoot: false,
    };

    #[test]
    fn two_open_strips_end_at_the_start_of_the_tip_stroke() {
        let mut hull = HairHull::default();
        let tip = Vec3::new(0.013, -0.306, 0.009);
        let points = [
            Vec3::ZERO,
            Vec3::NEG_Y * 0.2,
            Vec3::new(0.03, -0.27, 0.01),
            tip,
        ];
        hull.append_lock(&points, Vec3::Z, BASE_PROFILE);

        assert_eq!(hull.vertices.len(), 2 * SECTION_VERTICES as usize);
        assert_eq!(hull.triangles.len(), 4);
        assert_eq!(hull.tips.len(), 1);
        let [start, wide_middle, split, thin_middle, end] = hull.tips[0].points;
        assert_eq!([start, split, end], [points[1], points[2], tip]);
        for (a, middle, b) in [(start, wide_middle, split), (split, thin_middle, end)] {
            let offset = middle - a.lerp(b, 0.5);
            assert!(offset.length() > 1e-4);
            assert!(offset.length() <= MAX_TIP_MIDPOINT_OFFSET + 1e-6);
            assert!(offset.dot(b - a).abs() < 1e-6);
        }
        assert!(hull.vertices.iter().all(|vertex| {
            vertex.position.is_finite() && (vertex.normal.length_squared() - 1.0).abs() < 1e-4
        }));

        let capacity = hull.vertices.capacity();
        hull.clear();
        assert!(hull.vertices.is_empty() && hull.triangles.is_empty());
        assert!(hull.tips.is_empty());
        assert_eq!(hull.vertices.capacity(), capacity);
    }

    #[test]
    fn tip_thread_branches_from_the_drawn_wide_stroke_and_follows_the_tip() {
        let previous = Vec3::ZERO;
        let start = Vec3::NEG_Y * 0.2;
        let split = Vec3::new(0.03, -0.27, 0.01);
        let tip = Vec3::new(0.013, -0.306, 0.009);
        let normal = Vec3::Z;
        let full_thread = LockProfile {
            trim_main_tip: true,
            ..BASE_PROFILE
        };
        let hair = HairTip::new(Some(previous), start, split, tip, normal, full_thread, None);
        let [_, wide_middle, _, thin_middle, _] = hair.points;
        let [root, end] = hair.threads()[0];
        assert_eq!(root, wide_middle.lerp(split, 1.0 / 3.0));
        let target = thin_middle.lerp(tip, 0.8);
        let reach = target - root;
        let splay = end - target;
        assert!(splay.dot(reach).abs() < 1e-6);
        assert!(splay.length() <= reach.length() * TIP_THREAD_SPLAY + 1e-6);
        assert!(splay.dot(reach.cross(normal)) < 0.0);

        let staggered_profile = LockProfile {
            thread_starts_earlier: true,
            ..full_thread
        };
        let staggered = HairTip::new(
            Some(previous),
            start,
            split,
            tip,
            normal,
            staggered_profile,
            None,
        );
        assert_eq!(
            staggered.threads()[0],
            [previous.lerp(start, 2.0 / 3.0), end]
        );
        assert_eq!(staggered.points, hair.points);
        let regular_short = HairTip::new(None, start, split, tip, normal, full_thread, None);
        let staggered_short =
            HairTip::new(None, start, split, tip, normal, staggered_profile, None);
        assert_eq!(staggered_short.threads(), regular_short.threads());

        let rotation = Quat::from_rotation_z(0.8);
        let moved = HairTip::new(
            Some(rotation * previous),
            rotation * start,
            rotation * split,
            rotation * tip,
            rotation * normal,
            full_thread,
            None,
        );
        for (before, after) in [root, end].into_iter().zip(moved.threads()[0]) {
            assert!((rotation * before).distance(after) < 1e-6);
        }
    }

    #[test]
    fn only_the_selected_end_is_shortened_by_half_the_final_link() {
        let previous = Vec3::ZERO;
        let start = Vec3::NEG_Y * 0.2;
        let split = Vec3::new(0.03, -0.27, 0.01);
        let tip = Vec3::new(0.013, -0.306, 0.009);
        for thread_starts_earlier in [false, true] {
            let profile = LockProfile {
                thread_starts_earlier,
                ..BASE_PROFILE
            };
            let make_tip = |trim_main_tip| {
                HairTip::new(
                    Some(previous),
                    start,
                    split,
                    tip,
                    Vec3::Z,
                    LockProfile {
                        trim_main_tip,
                        ..profile
                    },
                    None,
                )
            };
            let short_main = make_tip(true);
            let short_thread = make_tip(false);
            let [.., thin_middle, _] = short_main.points;
            assert_eq!(
                short_main.stroke_points(),
                &short_thread.stroke_points()[..4]
            );
            assert_eq!(short_main.stroke_points().last(), Some(&thin_middle));
            assert_eq!(short_thread.stroke_points().last(), Some(&tip));
            assert!((split.distance(thin_middle) - tip.distance(thin_middle)).abs() < 1e-6);

            let [root, full_end] = short_main.threads()[0];
            let [short_root, short_end] = short_thread.threads()[0];
            assert_eq!(short_root, root);
            assert!((full_end.distance(short_end) - split.distance(tip) * 0.5).abs() < 1e-6);
            assert!(
                (root.distance(full_end) - root.distance(short_end) - full_end.distance(short_end))
                    .abs()
                    < 1e-6
            );
        }
    }

    #[test]
    fn high_offshoot_starts_farther_up_both_attachment_patterns() {
        let points =
            [0.0, 1.0, 2.0, 3.0, 4.0, 4.0 + 2.0 / 3.0, 5.0].map(|distance| Vec3::NEG_Y * distance);
        for (thread_starts_earlier, expected_root_y) in [(false, -2.638889), (true, -1.861111)] {
            let mut hull = HairHull::default();
            hull.append_lock(
                &points,
                Vec3::Z,
                LockProfile {
                    thread_starts_earlier,
                    high_offshoot: true,
                    ..BASE_PROFILE
                },
            );
            let [low, high] = hull.tips[0].threads() else {
                panic!("expected two offshoots");
            };
            assert!((high[0].y - expected_root_y).abs() < 1e-5);
            assert!(high[0].y > low[0].y);
            assert!((high[1].y + 4.966667).abs() < 1e-5);
            assert!(high[1].x < 0.0 && low[1].x > 0.0);
        }
    }

    #[test]
    fn only_the_last_section_is_narrowed_at_the_stroke_join() {
        let points = [
            Vec3::ZERO,
            Vec3::NEG_Y * 0.2,
            Vec3::NEG_Y * 0.3,
            Vec3::NEG_Y * 0.35,
        ];
        let mut hull = HairHull::default();
        let tip_half_width = 0.001;
        hull.append_lock(
            &points,
            Vec3::Z,
            LockProfile {
                tip_half_width,
                ..BASE_PROFILE
            },
        );
        let width = |first: usize| {
            hull.vertices[first]
                .position
                .distance(hull.vertices[first + 1].position)
        };
        assert_eq!(width(0), 2.0 * BASE_PROFILE.half_width);
        assert_eq!(width(2), 2.0 * BASE_PROFILE.half_depth);
        assert!((width(4) - 2.0 * tip_half_width).abs() < 1e-6);
        assert!((width(6) - 2.0 * tip_half_width).abs() < 1e-6);
    }

    #[test]
    fn coincident_control_points_keep_the_end_and_a_finite_frame() {
        let mut hull = HairHull::default();
        let middle = Vec3::NEG_Y * 0.2;
        let tip = Vec3::NEG_Y * 0.4;
        hull.append_lock(&[Vec3::ZERO; 3], Vec3::NEG_Y, BASE_PROFILE);
        assert!(hull.vertices.is_empty() && hull.tips.is_empty());
        hull.append_lock(
            &[Vec3::ZERO, Vec3::ZERO, tip, tip],
            Vec3::NEG_Y,
            BASE_PROFILE,
        );
        assert!(hull.vertices.is_empty() && hull.triangles.is_empty());
        assert_eq!(hull.tips.len(), 1);
        let fallback = hull.tips[0].points;
        assert_eq!(fallback[0], Vec3::ZERO);
        assert_eq!(fallback[2], tip * (2.0 / 3.0));
        assert_eq!(fallback[4], tip);

        hull.clear();
        hull.append_lock(
            &[Vec3::ZERO, Vec3::ZERO, middle, middle, tip, tip],
            Vec3::NEG_Y,
            BASE_PROFILE,
        );
        assert!(hull.vertices.is_empty() && hull.triangles.is_empty());
        assert_eq!(hull.tips.len(), 1);
        let [start, wide_middle, split, thin_middle, end] = hull.tips[0].points;
        assert_eq!([start, split, end], [Vec3::ZERO, middle, tip]);
        assert_eq!(wide_middle, start.lerp(split, 0.5));
        assert_eq!(thin_middle, split.lerp(end, 0.5));
    }

    #[test]
    fn strips_cross_at_each_usable_knot_before_the_tip() {
        let points = std::array::from_fn::<_, 11, _>(|index| {
            let after_bend = index.saturating_sub(5) as f32;
            Vec3::new(0.05 * after_bend, -0.08 * index as f32, 0.0)
        });
        let mut hull = HairHull::default();
        hull.append_lock(&points, Vec3::Z, BASE_PROFILE);

        let links = points.len() - 1;
        assert_eq!(hull.vertices.len(), (links - 1) * SECTION_VERTICES as usize);
        assert_eq!(hull.triangles.len(), (links - 2) * 4);
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
        assert_eq!(hull.tips[0].points[0], points[links - 2]);
        assert_eq!(hull.tips[0].points[2], points[links - 1]);
        assert_eq!(hull.tips[0].points[4], points[links]);
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
        let points = [Vec3::ZERO, tip * 0.4, tip * 0.8, tip];
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
            assert_eq!(scaled.tips[0].points[4], tip);
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
            let edge = texture.pixels[start][0];
            assert!(red / 2 < edge && edge < red);
            assert!(texture.pixels[start + 1][0] > edge);
            assert_eq!(texture.pixels[start + 3], center);
            assert_eq!(
                texture.pixels[start],
                texture.pixels[start + texture.width as usize - 1]
            );
        }
        assert!(texture.pixels.iter().all(|pixel| pixel[3] == 255));
    }

    #[test]
    fn changing_the_color_span_preserves_the_hull() {
        let points = [
            Vec3::ZERO,
            Vec3::NEG_Y * 0.2,
            Vec3::NEG_Y * 0.3,
            Vec3::NEG_Y * 0.4,
        ];
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
        assert_eq!(original.tips[0].points, tinted.tips[0].points);
    }

    #[test]
    fn palette_projects_by_avatar_height_through_bends_and_different_roots() {
        let straight_points = [
            Vec3::ZERO,
            Vec3::new(0.0, -0.25, 0.0),
            Vec3::new(0.0, -0.75, 0.0),
            Vec3::new(0.0, -1.0, 0.0),
        ];
        let bent_points = [
            Vec3::new(0.05, -0.05, 0.0),
            Vec3::new(0.11, -0.10, 0.04),
            Vec3::new(0.15, -0.18, 0.1),
            Vec3::new(0.22, -0.25, 0.18),
        ];
        let mut long = HairHull::default();
        long.append_lock(&straight_points, Vec3::Z, BASE_PROFILE);
        let mut short = HairHull::default();
        short.append_lock(&bent_points, Vec3::Z, BASE_PROFILE);

        let short_tip = short.tips[0].points[4];
        assert_eq!(short_tip, bent_points[3]);
        let short_tip_uv = BASE_PROFILE.palette_height(short_tip);
        assert_eq!(short_tip_uv, long.vertices[SECTION_VERTICES as usize].uv.y);
        assert_eq!(short_tip_uv, 0.25 / BASE_PROFILE.color_span);
        assert!(BASE_PROFILE.palette_height(long.tips[0].points[4]) > 1.0);
        for vertex in &short.vertices {
            assert_eq!(
                vertex.uv.y,
                (BASE_PROFILE.color_top - vertex.position.y) / BASE_PROFILE.color_span
            );
        }
    }
}
