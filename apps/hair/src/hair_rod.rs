use glam::Vec3;

use crate::hair_rope::{GRAVITY, MAX_SEGMENTS};

const MAX_POINTS: usize = MAX_SEGMENTS + 1;
const ANGULAR_DAMPING: f32 = 9.0;
const STRAIGHTENING: f32 = 95.0;
const MAX_ANGULAR_SPEED: f32 = 9.0;
const REFERENCE_HZ: f32 = 180.0;
const JOINT_LAG: f32 = 0.22;
const CONTACT_RETENTION: f32 = 0.45;
const BEND_COSINE: f32 = 0.866_025_4;
const BEND_SINE: f32 = 0.5;
const CONTACT_PASSES: usize = 4;

pub(crate) struct Rod {
    positions: [Vec3; MAX_POINTS],
    direction_velocity: [Vec3; MAX_POINTS],
    point_count: usize,
}

impl Rod {
    pub(crate) fn new(positions: &[Vec3]) -> Self {
        assert!((1..=MAX_POINTS).contains(&positions.len()));
        let mut rod = Self {
            positions: [Vec3::ZERO; MAX_POINTS],
            direction_velocity: [Vec3::ZERO; MAX_POINTS],
            point_count: positions.len(),
        };
        rod.positions[..rod.point_count].copy_from_slice(positions);
        rod
    }

    pub(crate) fn points(&self) -> &[Vec3] {
        &self.positions[..self.point_count]
    }

    pub(crate) fn step(
        &mut self,
        anchor: Vec3,
        length: f32,
        dt: f32,
        mut project: impl FnMut(usize, Vec3) -> Vec3,
    ) {
        let before = self.positions;
        let mut parent = anchor;
        let mut parent_direction = None;
        self.positions[0] = anchor;

        let damping = 1.0 / (1.0 + ANGULAR_DAMPING * dt);
        let reference_steps = dt * REFERENCE_HZ;
        let joint_lag = if reference_steps == 1.0 {
            JOINT_LAG
        } else {
            1.0 - (1.0 - JOINT_LAG).powf(reference_steps)
        };
        let contact_retention = CONTACT_RETENTION.powf(reference_steps);
        for i in 1..self.point_count {
            let direction = (before[i] - before[i - 1])
                .try_normalize()
                .unwrap_or(Vec3::NEG_Y);
            let trailing_length = length * (self.point_count - i) as f32;
            let mut torque = GRAVITY / trailing_length;
            if let Some(previous) = parent_direction {
                torque += previous * STRAIGHTENING;
            }
            let velocity = (tangent(self.direction_velocity[i], direction) * damping
                + tangent(torque, direction) * dt)
                .clamp_length_max(MAX_ANGULAR_SPEED);

            // A moved joint leaves its tip behind in world space. Integrate
            // direction around that joint and rebuild the link at its rest length.
            let held_tip = (before[i] - parent).try_normalize().unwrap_or(direction);
            let mut next_direction = (direction.lerp(held_tip, joint_lag) + velocity * dt)
                .try_normalize()
                .unwrap_or(direction);
            if let Some(previous) = parent_direction {
                next_direction = limit_bend(previous, next_direction);
            }

            let (point, contacted) =
                place_link(parent, next_direction, length, |point| project(i, point));
            let actual_direction = (point - parent).try_normalize().unwrap_or(next_direction);
            self.direction_velocity[i] = tangent(velocity, actual_direction)
                * if contacted { contact_retention } else { 1.0 };
            self.positions[i] = point;
            parent = point;
            parent_direction = Some(actual_direction);
        }
    }
}

fn tangent(vector: Vec3, direction: Vec3) -> Vec3 {
    vector - direction * vector.dot(direction)
}

fn limit_bend(parent: Vec3, direction: Vec3) -> Vec3 {
    if direction.dot(parent) >= BEND_COSINE {
        return direction;
    }
    let sideways = tangent(direction, parent)
        .try_normalize()
        .unwrap_or_else(|| parent.any_orthonormal_vector());
    parent * BEND_COSINE + sideways * BEND_SINE
}

fn place_link(
    parent: Vec3,
    direction: Vec3,
    length: f32,
    mut project: impl FnMut(Vec3) -> Vec3,
) -> (Vec3, bool) {
    let mut point = parent + direction * length;
    for pass in 0..CONTACT_PASSES {
        let clear = project(point);
        if clear.distance_squared(point) < 1e-12 {
            return (point, pass > 0);
        }
        let next = (clear - parent).try_normalize().unwrap_or_else(|| {
            // At a flat contact the projected tip can coincide with its
            // parent. Turn along the surface so the link doesn't collapse.
            let normal = (clear - point).normalize();
            tangent(direction, normal)
                .try_normalize()
                .unwrap_or_else(|| normal.any_orthonormal_vector())
        });
        point = parent + next * length;
    }
    // When the proxy moves across the link, prefer a clear endpoint.
    (project(point), true)
}
