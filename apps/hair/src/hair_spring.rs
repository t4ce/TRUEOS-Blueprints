use glam::Vec3;

use crate::hair_rope::{GRAVITY, POINTS};

const LINK_STIFFNESS: f32 = 11_000.0;
const LINK_DAMPING: f32 = 22.0;
const AIR_DRAG: f32 = 3.8;
const STRETCH_LIMIT: f32 = 1.42;
const MAX_SPEED: f32 = 5.0;
const MAX_SPRING_FORCE: f32 = 900.0;

pub(crate) struct Spring {
    positions: [Vec3; POINTS],
    velocities: [Vec3; POINTS],
}

impl Spring {
    pub(crate) fn new(positions: [Vec3; POINTS]) -> Self {
        Self {
            positions,
            velocities: [Vec3::ZERO; POINTS],
        }
    }

    pub(crate) fn points(&self) -> &[Vec3; POINTS] {
        &self.positions
    }

    pub(crate) fn step(
        &mut self,
        anchor: Vec3,
        length: f32,
        dt: f32,
        mut project: impl FnMut(usize, Vec3) -> Vec3,
    ) {
        self.velocities[0] = ((anchor - self.positions[0]) / dt).clamp_length_max(MAX_SPEED);
        self.positions[0] = anchor;

        let mut accelerations = [GRAVITY; POINTS];
        for index in 1..POINTS {
            let offset = self.positions[index] - self.positions[index - 1];
            let distance = offset.length();
            let axis = if distance > 1e-6 {
                offset / distance
            } else {
                Vec3::NEG_Y
            };
            let separation_speed = (self.velocities[index] - self.velocities[index - 1]).dot(axis);
            let tension = (LINK_STIFFNESS * (distance - length) + LINK_DAMPING * separation_speed)
                .clamp(-MAX_SPRING_FORCE, MAX_SPRING_FORCE);
            let acceleration = axis * tension;
            accelerations[index - 1] += acceleration;
            accelerations[index] -= acceleration;
        }

        let drag = (1.0 + AIR_DRAG * dt).recip();
        for (index, acceleration) in accelerations.into_iter().enumerate().skip(1) {
            let previous = self.positions[index];
            let velocity =
                ((self.velocities[index] + acceleration * dt) * drag).clamp_length_max(MAX_SPEED);
            let proposed = previous + velocity * dt;
            let parent = self.positions[index - 1];
            // The spring sets the normal length. A loose limit keeps a sudden
            // anchor jump from pulling a single link out into a spike.
            let limited = parent + (proposed - parent).clamp_length_max(length * STRETCH_LIMIT);
            let point = project(index, limited);
            let mut velocity = ((limited - previous) / dt).clamp_length_max(MAX_SPEED);
            if let Some(normal) = (point - limited).try_normalize() {
                velocity = (velocity - normal * velocity.dot(normal).min(0.0)) * 0.75;
            }
            self.positions[index] = point;
            self.velocities[index] = velocity;
        }
    }
}
