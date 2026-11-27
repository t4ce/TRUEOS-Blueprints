use glam::{Quat, Vec3};

use crate::{
    hair_rod::Rod,
    hair_spring::Spring,
    obstacle::{BODY_OBSTACLES, HEAD, downward_head_exit, rotate_about_head},
};

pub(crate) const POINTS: usize = 13;
pub(crate) const MAX_SEGMENTS: usize = 12;
pub(crate) const GRAVITY: Vec3 = Vec3::new(0.0, -9.81, 0.0);
const FRAME_SECONDS: f32 = 1.0 / 45.0;
const SUBSTEPS: u32 = 4;
const CLEARANCE: f32 = 0.009;
// Approximate longest free span in the reference cape hair, in metres.
const REFERENCE_LENGTH: f32 = 0.68;
const REFERENCE_SEGMENTS: f32 = 8.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum HairMotion {
    Rope,
    Spring,
    Rod,
}

impl HairMotion {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Rope => "Loose rope",
            Self::Spring => "Elastic springs",
            Self::Rod => "Bending rod",
        }
    }
}

pub(crate) struct RopeGuides {
    strands: Vec<Strand>,
    pose: Quat,
}

struct Strand {
    origin: Vec3,
    launch: Vec3,
    link_length: f32,
    chain: Chain,
}

enum Chain {
    Rope(Verlet),
    Spring(Spring),
    Rod(Rod),
}

impl Chain {
    fn new(motion: HairMotion, points: &[Vec3]) -> Self {
        match motion {
            HairMotion::Rope => {
                let points = points.try_into().unwrap();
                Self::Rope(Verlet {
                    points,
                    previous: points,
                })
            }
            HairMotion::Spring => Self::Spring(Spring::new(points.try_into().unwrap())),
            HairMotion::Rod => Self::Rod(Rod::new(points)),
        }
    }

    fn points(&self) -> &[Vec3] {
        match self {
            Self::Rope(chain) => &chain.points,
            Self::Spring(chain) => chain.points(),
            Self::Rod(chain) => chain.points(),
        }
    }

    fn step(
        &mut self,
        anchor: Vec3,
        length: f32,
        dt: f32,
        project: impl FnMut(usize, Vec3) -> Vec3,
    ) {
        match self {
            Self::Rope(chain) => chain.step(anchor, length, dt, project),
            Self::Spring(chain) => chain.step(anchor, length, dt, project),
            Self::Rod(chain) => chain.step(anchor, length, dt, project),
        }
    }
}

impl RopeGuides {
    pub(crate) fn new(roots: &[Vec3], tip_y: f32, pose: Quat, motion: HairMotion) -> Self {
        let avatar_to_world = pose.conjugate();
        let strands = roots
            .iter()
            .map(|&origin| {
                // Preserve the straight drop from the authored vertex under the band.
                let launch = downward_head_exit(origin, 0.026, CLEARANCE);
                let rest_length = (launch.y - tip_y).max(0.12);
                let segments = match motion {
                    HairMotion::Rod => rod_segments(rest_length),
                    HairMotion::Rope | HairMotion::Spring => POINTS - 1,
                };
                let link_length = rest_length / segments as f32;
                let anchor = rotate_about_head(launch, avatar_to_world);
                let mut points = [anchor; MAX_SEGMENTS + 1];
                let points = &mut points[..=segments];
                for (index, point) in points.iter_mut().enumerate().skip(1) {
                    *point = contact(
                        anchor - (link_length * index as f32) * Vec3::Y,
                        pose,
                        avatar_to_world,
                    );
                }
                Strand {
                    origin,
                    launch,
                    link_length,
                    chain: Chain::new(motion, points),
                }
            })
            .collect();
        Self { strands, pose }
    }

    pub(crate) fn advance(&mut self, pose: Quat, motion_speed_percent: u8) {
        let speed = f32::from(motion_speed_percent) / 100.0;
        let dt = FRAME_SECONDS * speed / SUBSTEPS as f32;
        for step in 1..=SUBSTEPS {
            let world_to_avatar = self.pose.slerp(pose, step as f32 / SUBSTEPS as f32);
            let avatar_to_world = world_to_avatar.conjugate();
            for strand in &mut self.strands {
                let anchor = rotate_about_head(strand.launch, avatar_to_world);
                strand
                    .chain
                    .step(anchor, strand.link_length, dt, |_, point| {
                        contact(point, world_to_avatar, avatar_to_world)
                    });
            }
        }
        self.pose = pose;
    }

    pub(crate) fn for_each_line(&self, mut emit: impl FnMut(Vec3, Vec3, f32)) -> usize {
        let mut line_count = 0;
        for (index, strand) in self.strands.iter().enumerate() {
            let highlight = (index % 7) as f32 / 6.0;
            let mut from = strand.origin;
            let points = strand.chain.points();
            for &point in points {
                let to = rotate_about_head(point, self.pose);
                emit(from, to, highlight);
                from = to;
            }
            line_count += points.len();
        }
        line_count
    }

    pub(crate) fn len(&self) -> usize {
        self.strands.len()
    }
}

fn rod_segments(rest_length: f32) -> usize {
    let segments = (rest_length / REFERENCE_LENGTH * REFERENCE_SEGMENTS).round() as usize;
    segments.clamp(4, MAX_SEGMENTS)
}

fn contact(point: Vec3, world_to_avatar: Quat, avatar_to_world: Quat) -> Vec3 {
    let mut local = rotate_about_head(point, world_to_avatar);
    local = HEAD.push_out(local, CLEARANCE);
    for shape in BODY_OBSTACLES {
        local = shape.push_out(local, CLEARANCE);
    }
    rotate_about_head(local, avatar_to_world)
}

struct Verlet {
    points: [Vec3; POINTS],
    previous: [Vec3; POINTS],
}

impl Verlet {
    fn step(
        &mut self,
        anchor: Vec3,
        length: f32,
        dt: f32,
        mut project: impl FnMut(usize, Vec3) -> Vec3,
    ) {
        self.points[0] = anchor;
        for (point, previous) in self.points[1..].iter_mut().zip(&mut self.previous[1..]) {
            let motion = (*point - *previous).clamp_length_max(4.5 * dt);
            *previous = *point;
            *point += 0.978 * motion + GRAVITY * dt * dt;
        }
        for _ in 0..5 {
            for index in 1..POINTS {
                let offset = self.points[index] - self.points[index - 1];
                let direction = offset.try_normalize().unwrap_or(Vec3::NEG_Y);
                let correction = offset - length * direction;
                if index == 1 {
                    self.points[index] -= correction;
                } else {
                    self.points[index - 1] += 0.5 * correction;
                    self.points[index] -= 0.5 * correction;
                }
            }
            for (index, point) in self.points.iter_mut().enumerate().skip(1) {
                let projected = project(index, *point);
                self.previous[index] += projected - *point;
                *point = projected;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_three_models_keep_the_exact_root_during_turns() {
        let roots = crate::scene::reference_roots();
        let roots = [roots[0], roots[45], roots[100]];
        let modes = [HairMotion::Rope, HairMotion::Spring, HairMotion::Rod];
        for motion in modes {
            let mut guides = RopeGuides::new(&roots, 0.74, Quat::IDENTITY, motion);
            for frame in 0..30 {
                let phase = frame as f32 * 0.07;
                let pose = Quat::from_rotation_x(0.9 * phase.sin()) * Quat::from_rotation_z(phase);
                guides.advance(pose, 100);
                let mut remaining = 0;
                let mut strands = guides.strands.iter();
                let count = guides.for_each_line(|from, to, _| {
                    if remaining == 0 {
                        let strand = strands.next().unwrap();
                        assert_eq!(from, strand.origin);
                        assert!((to.x - from.x).abs() < 1e-6 && (to.z - from.z).abs() < 1e-6);
                        assert!(to.y < from.y);
                        remaining = strand.chain.points().len();
                    }
                    assert!(from.is_finite() && to.is_finite());
                    assert!(from.distance(to) < 0.25);
                    remaining -= 1;
                });
                assert_eq!(remaining, 0);
                assert!(strands.next().is_none());
                assert_eq!(
                    count,
                    guides
                        .strands
                        .iter()
                        .map(|s| s.chain.points().len())
                        .sum::<usize>()
                );
            }
        }
    }

    #[test]
    fn rod_segments_scale_with_free_length_and_keep_the_caps() {
        let root = crate::scene::reference_roots()[45];
        let launch = downward_head_exit(root, 0.026, CLEARANCE);
        for (scale, segments) in [(0.2, 4), (0.5, 4), (1.0, 8), (1.5, 12), (2.0, 12)] {
            let length = scale * REFERENCE_LENGTH;
            let guides =
                RopeGuides::new(&[root], launch.y - length, Quat::IDENTITY, HairMotion::Rod);
            let strand = &guides.strands[0];
            assert_eq!(strand.chain.points().len(), segments + 1);
            assert!((strand.link_length * segments as f32 - length).abs() < 1e-6);
        }
    }

    #[test]
    fn rod_speed_changes_motion_without_resampling_links() {
        let root = crate::scene::reference_roots()[45];
        let make_guides = || RopeGuides::new(&[root], 0.74, Quat::IDENTITY, HairMotion::Rod);
        let mut slow = make_guides();
        let mut fast = make_guides();
        let pose = Quat::from_rotation_z(1.0);
        for _ in 0..8 {
            slow.advance(pose, 90);
            fast.advance(pose, 110);
        }
        let slow = &slow.strands[0];
        let fast = &fast.strands[0];
        let slow_points = slow.chain.points();
        let fast_points = fast.chain.points();
        assert_eq!(slow.link_length, fast.link_length);
        assert_eq!(slow_points.len(), fast_points.len());
        assert_eq!(slow_points[0], fast_points[0]);
        assert!(
            slow_points
                .last()
                .unwrap()
                .distance(*fast_points.last().unwrap())
                > 1e-3
        );
    }
}
