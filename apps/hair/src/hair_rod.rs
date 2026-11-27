use glam::Vec3;

use crate::hair_rope::{GRAVITY, MAX_SEGMENTS};

const MAX_POINTS: usize = MAX_SEGMENTS + 1;
const ANGULAR_DAMPING: f32 = 9.0;
const STRAIGHTENING: f32 = 95.0;
const MAX_ANGULAR_SPEED: f32 = 9.0;
const REFERENCE_HZ: f32 = 180.0;
const JOINT_LAG: f32 = 0.22;
const TIP_LAG_GAIN: f32 = 0.05;
const CONTACT_RETENTION: f32 = 0.45;
const TIP_CONTACT_RETENTION: f32 = 0.63;
const BEND_COSINE: f32 = 0.866_025_4;
const BEND_SINE: f32 = 0.5;
const CONTACT_PASSES: usize = 4;

#[derive(Clone, Copy, Default)]
pub(crate) struct RodEffects {
    pub(crate) tip_weight: f32,
    pub(crate) spread_acceleration: Vec3,
    pub(crate) spread_shape: f32,
}

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
        effects: RodEffects,
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
        let tip_contact_gain = TIP_CONTACT_RETENTION.powf(reference_steps) - contact_retention;
        let tip_lag_gain = TIP_LAG_GAIN * reference_steps;
        let spread_start = 0.5 + 0.25 * effects.spread_shape;
        let segment_count = self.point_count - 1;
        for i in 1..self.point_count {
            let progress = i as f32 / segment_count as f32;
            let tip_weight = effects.tip_weight * smooth_ramp(progress, 0.5);
            let spread = smooth_ramp(progress, spread_start);
            let direction = (before[i] - before[i - 1])
                .try_normalize()
                .unwrap_or(Vec3::NEG_Y);
            let trailing_length = length * (self.point_count - i) as f32;
            let mut torque = (GRAVITY + effects.spread_acceleration * spread) / trailing_length;
            if let Some(previous) = parent_direction {
                torque += previous * STRAIGHTENING;
            }
            let velocity = (tangent(self.direction_velocity[i], direction) * damping
                + tangent(torque, direction) * dt)
                .clamp_length_max(MAX_ANGULAR_SPEED);

            // A moved joint leaves its tip behind in world space. Integrate
            // direction around that joint and rebuild the link at its rest length.
            // Weighted ends follow the moving parent more slowly.
            let held_tip = (before[i] - parent).try_normalize().unwrap_or(direction);
            let lag = joint_lag + tip_lag_gain * tip_weight;
            let mut next_direction = (direction.lerp(held_tip, lag) + velocity * dt)
                .try_normalize()
                .unwrap_or(direction);
            if let Some(previous) = parent_direction {
                next_direction = limit_bend(previous, next_direction);
            }

            let (point, contacted) =
                place_link(parent, next_direction, length, |point| project(i, point));
            let actual_direction = (point - parent).try_normalize().unwrap_or(next_direction);
            self.direction_velocity[i] = tangent(velocity, actual_direction)
                * if contacted {
                    contact_retention + tip_contact_gain * tip_weight
                } else {
                    1.0
                };
            self.positions[i] = point;
            parent = point;
            parent_direction = Some(actual_direction);
        }
    }
}

fn smooth_ramp(progress: f32, start: f32) -> f32 {
    let amount = ((progress - start) / (1.0 - start)).clamp(0.0, 1.0);
    amount * amount * (3.0 - 2.0 * amount)
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

#[cfg(test)]
mod tests {
    use super::*;

    const LINK_LENGTH: f32 = 0.075;
    const DT: f32 = 1.0 / REFERENCE_HZ;

    fn hanging_rod() -> Rod {
        let points: [Vec3; 9] = std::array::from_fn(|i| Vec3::NEG_Y * LINK_LENGTH * i as f32);
        Rod::new(&points)
    }

    #[test]
    fn added_tip_weight_lags_a_moving_anchor_in_the_lower_half() {
        let mut baseline = hanging_rod();
        let mut weighted = hanging_rod();
        let anchor = 0.05 * Vec3::X;
        baseline.step(
            anchor,
            LINK_LENGTH,
            DT,
            RodEffects::default(),
            |_, point| point,
        );
        weighted.step(
            anchor,
            LINK_LENGTH,
            DT,
            RodEffects {
                tip_weight: 1.0,
                ..RodEffects::default()
            },
            |_, point| point,
        );
        assert_eq!(baseline.points()[..=4], weighted.points()[..=4]);
        assert!(weighted.points()[8].x < baseline.points()[8].x);
    }

    #[test]
    fn spread_shape_delays_the_fan_and_width_controls_its_reach() {
        let mut broad = hanging_rod();
        let mut narrow = hanging_rod();
        let mut off = hanging_rod();
        let spread = RodEffects {
            spread_acceleration: 13.0 * Vec3::X,
            ..RodEffects::default()
        };
        for _ in 0..90 {
            broad.step(Vec3::ZERO, LINK_LENGTH, DT, spread, |_, point| point);
            narrow.step(
                Vec3::ZERO,
                LINK_LENGTH,
                DT,
                RodEffects {
                    spread_shape: 1.0,
                    ..spread
                },
                |_, point| point,
            );
            off.step(
                Vec3::ZERO,
                LINK_LENGTH,
                DT,
                RodEffects {
                    spread_shape: 1.0,
                    ..RodEffects::default()
                },
                |_, point| point,
            );
        }
        let baseline = hanging_rod();
        assert_eq!(broad.points()[..=4], off.points()[..=4]);
        assert!(broad.points()[6].x > narrow.points()[6].x + 1e-3);
        assert!(narrow.points()[8].x > off.points()[8].x + 1e-3);
        for (point, rest) in off.points().iter().zip(baseline.points()) {
            assert!(point.distance(*rest) < 1e-6);
        }
    }
}
