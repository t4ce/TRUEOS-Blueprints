use glam::{Mat4, Vec3};

use crate::{animation::SAMPLE_HZ, hair_body::BodyBack};

pub(super) const PARTICLES: usize = 9;
const SEGMENTS: usize = PARTICLES - 1;
const SUBSTEPS: usize = 2;
const STEP_SECONDS: f32 = 1.0 / (SAMPLE_HZ as f32 * SUBSTEPS as f32);
const SOLVER_PASSES: usize = 4;
const GRAVITY: Vec3 = Vec3::new(0.0, -9.81, 0.0);
const VELOCITY_DAMPING: f32 = 0.92;
const ROOT_TANGENT_STIFFNESS: f32 = 0.28;
const BEND_STIFFNESS: f32 = 0.12;
const MIN_TANGENT_DOT: f32 = 0.906_307_8; // cos(25 degrees)
const MAX_TANGENT_SIN: f32 = 0.422_618_27; // sin(25 degrees)
const ROOT_EXIT_BLEND: [f32; 2] = [0.85, 0.25];
const UPPER_BACK_SPREAD: [f32; 3] = [1.0, 0.85, 0.45];
const SPREAD_WEIGHT: f32 = 2.2;
const BODY_CLEARANCE: f32 = 0.009;

#[derive(Clone)]
pub(super) struct Hair {
    strands: Vec<Strand>,
    head: Mat4,
}

#[derive(Clone)]
struct Strand {
    root: Vec3,
    exit_direction: Vec3,
    lateral_slope: f32,
    upper_back_slope: f32,
    points: [Vec3; PARTICLES],
    previous: [Vec3; PARTICLES],
    lengths: [f32; SEGMENTS],
    bend_fractions: [f32; SEGMENTS - 1],
}

/// One gentle gust in world space, calm at the start and end of an action.
pub(super) fn breeze(progress: f32) -> Vec3 {
    if !(0.0..1.0).contains(&progress) {
        return Vec3::ZERO;
    }
    let envelope = (std::f32::consts::PI * progress).sin();
    Vec3::new(2.2, 0.0, 0.7) * (envelope * envelope)
}

impl Hair {
    /// The head transform takes points from bind space to the sampled pose.
    pub(super) fn new(guides: Vec<[Vec3; PARTICLES]>, head: Mat4) -> Self {
        let right = horizontal_right(head);
        let strands = guides
            .into_iter()
            .map(|guide| Strand::new(guide, head, right))
            .collect();
        Self { strands, head }
    }

    /// Advance one 45 Hz animation pose with two smaller physics steps.
    pub(super) fn step(&mut self, head: Mat4, wind_acceleration: Vec3, body: &BodyBack) {
        let previous_right = horizontal_right(self.head);
        let right = horizontal_right(head);
        for strand in &mut self.strands {
            let from = strand.hanging_guide(self.head, previous_right);
            let to = strand.hanging_guide(head, right);
            for step in 1..=SUBSTEPS {
                let fraction = step as f32 / SUBSTEPS as f32;
                let target = std::array::from_fn(|i| from[i].lerp(to[i], fraction));
                strand.advance(&target, wind_acceleration, body);
            }
        }
        self.head = head;
    }

    pub(super) fn strands(&self) -> impl ExactSizeIterator<Item = &[Vec3; PARTICLES]> {
        self.strands.iter().map(|strand| &strand.points)
    }

    /// Blend a displayed drape around the current roots without changing the solver state.
    pub(super) fn append_loop_pose(
        &self,
        reference: &[[Vec3; PARTICLES]],
        weight: f32,
        result: &mut Vec<[Vec3; PARTICLES]>,
    ) {
        debug_assert_eq!(self.strands.len(), reference.len());
        for (strand, reference) in self.strands.iter().zip(reference) {
            if weight >= 1.0 {
                let shift = strand.points[0] - reference[0];
                result.push(reference.map(|point| point + shift));
                continue;
            }
            let mut points = strand.points;
            for (i, &length) in strand.lengths.iter().enumerate() {
                let current = strand.points[i + 1] - strand.points[i];
                let target = reference[i + 1] - reference[i];
                let direction = current
                    .lerp(target, weight)
                    .try_normalize()
                    .unwrap_or_else(|| target.normalize_or_zero());
                points[i + 1] = points[i] + direction * length;
            }
            result.push(points);
        }
    }
}

fn horizontal_right(head: Mat4) -> Vec3 {
    let head_right = head.x_axis;
    let head_back = head.z_axis;
    Vec3::new(head_right.x + head_back.z, 0.0, head_right.z - head_back.x)
        .try_normalize()
        .unwrap_or(Vec3::X)
}

impl Strand {
    fn new(guide: [Vec3; PARTICLES], head: Mat4, right: Vec3) -> Self {
        let lengths = std::array::from_fn(|i| guide[i].distance(guide[i + 1]));
        let bend_fractions =
            std::array::from_fn(|i| lengths[i] / (lengths[i] + lengths[i + 1]).max(f32::EPSILON));
        let total_length: f32 = lengths.iter().sum();
        let root = guide[0];
        // Keep the spread across and behind the shoulders as two slopes per
        // strand. The sampled bends and curled ends are discarded.
        let lateral_reach = guide
            .iter()
            .map(|point| point.x - root.x)
            .max_by(|a, b| a.abs().total_cmp(&b.abs()))
            .unwrap_or_default();
        let back_reach = guide
            .iter()
            .map(|point| point.z - root.z)
            .fold(0.0, f32::max);
        let upper_length: f32 = lengths[..UPPER_BACK_SPREAD.len()].iter().sum();
        let mut strand = Self {
            root,
            exit_direction: (guide[1] - root).normalize_or_zero(),
            lateral_slope: SPREAD_WEIGHT * lateral_reach / total_length.max(f32::EPSILON),
            upper_back_slope: back_reach / upper_length.max(f32::EPSILON),
            points: [Vec3::ZERO; PARTICLES],
            previous: [Vec3::ZERO; PARTICLES],
            lengths,
            bend_fractions,
        };
        strand.points = strand.hanging_guide(head, right);
        strand.previous = strand.points;
        strand
    }

    fn hanging_guide(&self, head: Mat4, right: Vec3) -> [Vec3; PARTICLES] {
        let mut target = [Vec3::ZERO; PARTICLES];
        target[0] = head.transform_point3(self.root);
        let back = right.cross(Vec3::Y);
        let hang = (Vec3::NEG_Y + right * self.lateral_slope).normalize();
        let exit = head.transform_vector3(self.exit_direction);
        for (i, &length) in self.lengths.iter().enumerate() {
            let direction = UPPER_BACK_SPREAD.get(i).map_or(hang, |&amount| {
                (hang + back * self.upper_back_slope * amount).normalize()
            });
            let direction = ROOT_EXIT_BLEND.get(i).map_or(direction, |&follow| {
                direction.lerp(exit, follow).normalize_or_zero()
            });
            target[i + 1] = target[i] + direction * length;
        }
        target
    }

    fn advance(&mut self, target: &[Vec3; PARTICLES], wind_acceleration: Vec3, body: &BodyBack) {
        self.points[0] = target[0];
        for (i, ((point, previous), &goal)) in self
            .points
            .iter_mut()
            .zip(&mut self.previous)
            .zip(target)
            .enumerate()
            .skip(1)
        {
            let velocity = (*point - *previous) * VELOCITY_DAMPING;
            let progress = i as f32 / SEGMENTS as f32;
            let shape_stiffness = 60.0 - 38.0 * progress;
            let acceleration =
                (goal - *point) * shape_stiffness + GRAVITY + wind_acceleration * progress;
            *previous = *point;
            *point += velocity + acceleration * (STEP_SECONDS * STEP_SECONDS);
        }
        for _ in 0..SOLVER_PASSES {
            self.points[1] = self.points[1].lerp(target[1], ROOT_TANGENT_STIFFNESS);
            for i in 1..SEGMENTS {
                let fraction = self.bend_fractions[i - 1];
                let rest_offset = target[i] - target[i - 1].lerp(target[i + 1], fraction);
                let bend = self.points[i - 1].lerp(self.points[i + 1], fraction) + rest_offset;
                self.points[i] = self.points[i].lerp(bend, BEND_STIFFNESS);
            }
            for (i, &length) in self.lengths.iter().enumerate().rev() {
                let offset = self.points[i + 1] - self.points[i];
                let correction = offset - segment_direction(offset, target, i) * length;
                if i == 0 {
                    self.points[i + 1] -= correction;
                } else {
                    self.points[i] += correction * 0.5;
                    self.points[i + 1] -= correction * 0.5;
                }
            }
        }
        // Finish from the pinned root with exact lengths and a limit on how far
        // each segment can bend away from its guide during abrupt pose changes.
        for (i, &length) in self.lengths.iter().enumerate() {
            let offset = self.points[i + 1] - self.points[i];
            let preferred = (target[i + 1] - target[i]).normalize_or_zero();
            let direction = offset.try_normalize().unwrap_or(preferred);
            let parent = self.points[i];
            let offset = limit_bend(direction, preferred) * length;
            let cleared = clear_body(parent, offset, length, body);
            self.points[i + 1] = parent + cleared;
            // Contact changes position without adding outward velocity.
            self.previous[i + 1] += cleared - offset;
        }
    }
}

fn clear_body(parent: Vec3, mut offset: Vec3, length: f32, body: &BodyBack) -> Vec3 {
    let plane_direction = offset
        .truncate()
        .try_normalize()
        .unwrap_or(Vec3::NEG_Y.truncate());
    // Sample along the segment as well as at the tip to keep it from cutting
    // through the curve of the back. Slide over the surface at fixed length.
    for _ in 0..3 {
        let mut minimum_z = offset.z;
        for fraction in [0.25, 0.50, 0.75, 1.0] {
            let point = parent + offset * fraction;
            let back = body.depth_at(point.x, point.y) + BODY_CLEARANCE;
            minimum_z = minimum_z.max((back - parent.z) / fraction);
        }
        if minimum_z <= offset.z + 1e-6 {
            break;
        }
        let z = minimum_z.min(length);
        let planar_length = (length * length - z * z).max(0.0).sqrt();
        offset = (plane_direction * planar_length).extend(z);
    }
    offset
}

fn limit_bend(direction: Vec3, preferred: Vec3) -> Vec3 {
    let dot = direction.dot(preferred);
    if dot >= MIN_TANGENT_DOT || preferred == Vec3::ZERO {
        return direction;
    }
    let perpendicular = (direction - preferred * dot)
        .try_normalize()
        .unwrap_or_else(|| preferred.any_orthonormal_vector());
    preferred * MIN_TANGENT_DOT + perpendicular * MAX_TANGENT_SIN
}

fn segment_direction(offset: Vec3, target: &[Vec3; PARTICLES], i: usize) -> Vec3 {
    offset
        .try_normalize()
        .unwrap_or_else(|| (target[i + 1] - target[i]).normalize_or_zero())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn guide() -> [Vec3; PARTICLES] {
        std::array::from_fn(|i| {
            let t = i as f32 / SEGMENTS as f32;
            Vec3::new(
                0.10 + 0.12 * t * t,
                1.47 - 0.70 * t,
                0.07 + 0.30 * t - 0.14 * t * t,
            )
        })
    }

    #[test]
    fn loop_pose_blend_pins_the_roots_and_preserves_segment_lengths() {
        let mut hair = Hair::new(vec![guide()], Mat4::IDENTITY);
        let reference = [*hair.strands().next().unwrap()];
        let body = BodyBack::default();
        for _ in 0..45 {
            hair.step(Mat4::IDENTITY, Vec3::Z * 3.0, &body);
        }
        let live = *hair.strands().next().unwrap();
        assert!(live[SEGMENTS].distance(reference[0][SEGMENTS]) > 0.01);
        let mut blended = Vec::new();
        hair.append_loop_pose(&reference, 0.5, &mut blended);
        assert_eq!(blended[0][0], reference[0][0]);
        for (actual, rest) in blended[0].windows(2).zip(reference[0].windows(2)) {
            assert!((actual[0].distance(actual[1]) - rest[0].distance(rest[1])).abs() < 1e-6);
        }
        hair.append_loop_pose(&reference, 1.0, &mut blended);
        assert_eq!(blended[1], reference[0]);
        assert_eq!(*hair.strands().next().unwrap(), live);
    }

    #[test]
    fn root_follows_the_head_while_the_tip_lags_without_stretching() {
        let guide = guide();
        let mut hair = Hair::new(vec![guide], Mat4::IDENTITY);
        let body = BodyBack::default();
        for _ in 0..120 {
            hair.step(Mat4::IDENTITY, Vec3::ZERO, &body);
        }
        let settled_tip = hair.strands().next().unwrap()[SEGMENTS];
        let shift = Vec3::new(0.16, -0.025, 0.035);
        let head = Mat4::from_translation(shift);
        for _ in 0..120 {
            hair.step(head, Vec3::ZERO, &body);
            let points = hair.strands().next().unwrap();
            assert!(points[0].distance(guide[0] + shift) < 0.000_01);
            for (actual, rest) in points.windows(2).zip(guide.windows(2)) {
                assert!(
                    (actual[0].distance(actual[1]) - rest[0].distance(rest[1])).abs() < 0.000_01
                );
            }
        }
        let moved_tip = hair.strands().next().unwrap()[SEGMENTS];
        assert!(moved_tip.distance(settled_tip + shift) < 0.001);

        hair.step(Mat4::IDENTITY, Vec3::ZERO, &body);
        let moving = hair.strands().next().unwrap();
        let tip = moving[SEGMENTS];
        assert!(tip.distance(moved_tip) < tip.distance(settled_tip));
        assert!(tip.distance(settled_tip) > 0.04);
    }

    #[test]
    fn abrupt_head_motion_respects_the_bend_limit() {
        let mut hair = Hair::new(vec![guide()], Mat4::IDENTITY);
        let hanging_guide = *hair.strands().next().unwrap();
        hair.step(
            Mat4::from_translation(Vec3::X * 0.6),
            Vec3::ZERO,
            &BodyBack::default(),
        );
        let points = hair.strands().next().unwrap();
        for (actual, rest) in points.windows(2).zip(hanging_guide.windows(2)) {
            let direction = (actual[1] - actual[0]).normalize();
            let preferred = (rest[1] - rest[0]).normalize();
            assert!(direction.dot(preferred) >= MIN_TANGENT_DOT - 0.000_01);
        }
    }

    #[test]
    fn loose_ends_settle_down_when_the_head_stays_tilted() {
        let guide = guide();
        let head = Mat4::from_rotation_z(55.0_f32.to_radians());
        let mut hair = Hair::new(vec![guide], head);
        let body = BodyBack::default();
        for _ in 0..120 {
            hair.step(head, Vec3::ZERO, &body);
        }
        let points = hair.strands().next().unwrap();
        let lower = (points[SEGMENTS] - points[SEGMENTS / 2]).normalize();
        let rigid_lower = head
            .transform_vector3(guide[SEGMENTS] - guide[SEGMENTS / 2])
            .normalize();
        assert!(lower.dot(Vec3::NEG_Y) > 0.94, "lower direction: {lower:?}");
        assert!(lower.z.abs() < 0.05);
        assert!(lower.dot(Vec3::NEG_Y) > rigid_lower.dot(Vec3::NEG_Y) + 0.2);
        assert_eq!(points[0], head.transform_point3(guide[0]));
    }

    #[test]
    fn unsupported_lower_segments_release_the_authored_curl() {
        let guide = std::array::from_fn(|i| {
            let t = i as f32 / SEGMENTS as f32;
            Vec3::new(
                0.10 + 0.16 * t,
                1.47 - 0.70 * t,
                0.12 + 0.22 * (t * 5.0).sin(),
            )
        });
        let mut hair = Hair::new(vec![guide], Mat4::IDENTITY);
        let body = BodyBack::default();
        for _ in 0..120 {
            hair.step(Mat4::IDENTITY, Vec3::ZERO, &body);
        }
        let points = hair.strands().next().unwrap();
        let lower = &points[SEGMENTS / 2..];
        let chord = (lower[lower.len() - 1] - lower[0]).normalize();
        assert!(chord.dot(Vec3::NEG_Y) > 0.94, "lower direction: {chord:?}");
        assert!(chord.z.abs() < 0.05);
        assert!(points[SEGMENTS].x > points[0].x + 0.05);
        for pair in lower.windows(2) {
            assert!((pair[1] - pair[0]).normalize().dot(chord) > 0.999);
        }
    }

    #[test]
    fn breeze_moves_free_tips_sideways_from_pinned_roots() {
        let guide = guide();
        let mut still = Hair::new(vec![guide], Mat4::IDENTITY);
        let mut wind = Hair::new(vec![guide], Mat4::IDENTITY);
        let body = BodyBack::default();
        assert_eq!(breeze(0.0), Vec3::ZERO);
        assert_eq!(breeze(1.0), Vec3::ZERO);
        for i in 0..=60 {
            still.step(Mat4::IDENTITY, Vec3::ZERO, &body);
            wind.step(Mat4::IDENTITY, breeze(i as f32 / 90.0), &body);
        }
        let still = still.strands().next().unwrap();
        let wind = wind.strands().next().unwrap();
        assert_eq!(still[0], wind[0]);
        assert!(wind[SEGMENTS].x > still[SEGMENTS].x + 0.02);
    }

    #[test]
    fn hair_responds_to_wind_from_eight_directions_and_settles() {
        let guide = guide();
        let mut body = BodyBack::default();
        let back_z = 0.05;
        let back = [
            [-1.0, 0.4, back_z],
            [1.0, 0.4, back_z],
            [-1.0, 1.3, back_z],
            [1.0, 1.3, back_z],
        ];
        body.update(&back, &[0, 1, 2, 1, 3, 2]);

        for direction in [
            Vec3::X,
            Vec3::X + Vec3::Z,
            Vec3::Z,
            Vec3::NEG_X + Vec3::Z,
            Vec3::NEG_X,
            Vec3::NEG_X + Vec3::NEG_Z,
            Vec3::NEG_Z,
            Vec3::X + Vec3::NEG_Z,
        ] {
            let direction = direction.normalize();
            let mut hair = Hair::new(vec![guide], Mat4::IDENTITY);
            for _ in 0..5 * SAMPLE_HZ {
                hair.step(Mat4::IDENTITY, Vec3::ZERO, &body);
            }
            let calm = *hair.strands().next().unwrap();
            for _ in 0..3 * SAMPLE_HZ {
                hair.step(Mat4::IDENTITY, direction * 2.5, &body);
            }
            let blown = hair.strands().next().unwrap();
            assert_eq!(blown[0], guide[0]);
            assert!(
                (blown[SEGMENTS] - calm[SEGMENTS]).dot(direction) > 0.015,
                "no downwind motion for {direction:?}"
            );
            assert!(
                blown
                    .iter()
                    .filter(|point| point.y <= 1.3)
                    .all(|point| point.z >= back_z + BODY_CLEARANCE - 1.0e-6)
            );
            for (actual, rest) in blown.windows(2).zip(guide.windows(2)) {
                assert!((actual[0].distance(actual[1]) - rest[0].distance(rest[1])).abs() < 1.0e-5);
            }

            for _ in 0..5 * SAMPLE_HZ {
                hair.step(Mat4::IDENTITY, Vec3::ZERO, &body);
            }
            let settled = hair.strands().next().unwrap();
            assert!(
                settled
                    .iter()
                    .zip(calm)
                    .all(|(point, rest)| point.distance(rest) < 0.001)
            );
        }
    }

    #[test]
    fn body_contact_slides_the_strand_behind_the_surface_at_fixed_length() {
        let mut body = BodyBack::default();
        let positions = [[-0.2, 0.0, 0.1], [0.2, 0.0, 0.1], [0.0, 1.0, 0.1]];
        body.update(&positions, &[0, 1, 2]);
        let parent = Vec3::new(0.0, 0.8, 0.13);
        let offset = Vec3::new(0.0, -0.10, -0.08);
        let cleared = clear_body(parent, offset, offset.length(), &body);
        assert!((parent + cleared).z >= 0.1 + BODY_CLEARANCE - 1e-6);
        assert!((cleared.length() - offset.length()).abs() < 1e-6);
    }
}
