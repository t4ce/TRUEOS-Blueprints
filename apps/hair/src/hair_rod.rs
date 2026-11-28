use glam::Vec3;

use crate::hair_rope::{GRAVITY, MAX_SEGMENTS, rod_rest_units};

const MAX_POINTS: usize = MAX_SEGMENTS + 1;
const ANGULAR_DAMPING: f32 = 9.0;
const SETTLE_ANGULAR_DAMPING: f32 = 18.0;
const SETTLE_STEP_REDUCTION: f32 = 0.35;
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
// Mobility at 50% and 100% fixation, followed by the cone angles in degrees.
const HEADBAND_LINKS: [(f32, f32, f32, f32); 3] = [
    (0.25, 0.10, 15.0, 7.5),
    (0.50, 0.25, 22.5, 12.5),
    (0.75, 0.50, 30.0, 20.0),
];

#[derive(Clone, Copy)]
pub(crate) struct HeadbandFixation(Option<[(f32, f32, f32); 3]>);

impl HeadbandFixation {
    pub(crate) fn new(strength: f32) -> Self {
        if strength == 0.0 {
            return Self(None);
        }
        let baseline = (2.0 * strength).min(1.0);
        let extra = (2.0 * strength - 1.0).max(0.0);
        let limits = HEADBAND_LINKS.map(|(mobility, tight_mobility, angle, tight_angle)| {
            let mobility = 1.0 + (mobility - 1.0) * baseline + (tight_mobility - mobility) * extra;
            let angle = 180.0 + (angle - 180.0) * baseline + (tight_angle - angle) * extra;
            let (sine, cosine) = angle.to_radians().sin_cos();
            (mobility, cosine, sine)
        });
        Self(Some(limits))
    }
}

#[derive(Clone, Copy)]
pub(crate) struct RodEffects {
    pub(crate) tip_weight: f32,
    pub(crate) spawn_direction: Vec3,
    pub(crate) spread_acceleration: [Vec3; MAX_POINTS],
    pub(crate) wind_acceleration: Vec3,
    pub(crate) fixation: HeadbandFixation,
    pub(crate) settle_damping: f32,
}

impl Default for RodEffects {
    fn default() -> Self {
        Self {
            tip_weight: 0.0,
            spawn_direction: Vec3::NEG_Y,
            spread_acceleration: [Vec3::ZERO; MAX_POINTS],
            wind_acceleration: Vec3::ZERO,
            fixation: HeadbandFixation::new(0.5),
            settle_damping: 0.0,
        }
    }
}

pub(crate) struct Rod {
    positions: [Vec3; MAX_POINTS],
    direction_velocity: [Vec3; MAX_POINTS],
    point_count: usize,
}

impl Rod {
    pub(crate) fn new(positions: &[Vec3]) -> Self {
        assert!((3..=MAX_POINTS).contains(&positions.len()));
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
        regular_length: f32,
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
        let headband_links = effects.fixation.0.as_ref();
        let mut calm_settle = 0.0;
        if effects.settle_damping > 0.0 {
            let wind = effects.wind_acceleration.length();
            let anchor_speed = anchor.distance(before[0]) / dt;
            calm_settle = effects.settle_damping
                * (1.0 - smoothstep(1.0, 7.5, wind))
                * (1.0 - smoothstep(0.025, 0.25, anchor_speed));
        }
        let segment_count = self.point_count - 1;
        let total_rest_units = rod_rest_units(segment_count, segment_count);
        let mut start_units = 0.0;
        for i in 1..self.point_count {
            let end_units = rod_rest_units(i, segment_count);
            let progress = end_units / total_rest_units;
            let length = regular_length * (end_units - start_units);
            let trailing_length = regular_length * (total_rest_units - start_units);
            start_units = end_units;
            let tip_weight = effects.tip_weight * smoothstep(0.5, 1.0, progress);
            let headband = headband_links.and_then(|links| links.get(i - 1));
            let mobility = headband.map_or(1.0, |&(mobility, _, _)| mobility);
            let direction = (before[i] - before[i - 1])
                .try_normalize()
                .unwrap_or(Vec3::NEG_Y);
            let wind_exposure = 0.35 + 0.65 * progress;
            let acceleration = GRAVITY
                + effects.spread_acceleration[i]
                + effects.wind_acceleration * wind_exposure;
            let mut torque = acceleration / trailing_length;
            if let Some(previous) = parent_direction {
                torque += previous * STRAIGHTENING;
            }
            torque += effects.spawn_direction * (STRAIGHTENING * (1.0 - mobility));
            let incoming = tangent(self.direction_velocity[i], direction);
            let mut settle = 0.0;
            if calm_settle > 0.0 && progress > 0.55 {
                settle = calm_settle
                    * smoothstep(0.55, 1.0, progress)
                    * (1.0 - smoothstep(0.2, 1.25, incoming.length()));
            }
            // Resist small residual swings at the tips while the strand settles.
            let link_damping = damping / (1.0 + SETTLE_ANGULAR_DAMPING * settle * dt);
            let velocity = (incoming * link_damping + tangent(torque, direction) * dt)
                .clamp_length_max(MAX_ANGULAR_SPEED);

            // A moved joint leaves its tip behind in world space. Integrate
            // direction around that joint and rebuild the link at its rest length.
            // Weighted ends follow the moving parent more slowly.
            let held_tip = (before[i] - parent).try_normalize().unwrap_or(direction);
            let response = mobility * (1.0 - SETTLE_STEP_REDUCTION * settle);
            let lag = (joint_lag + tip_lag_gain * tip_weight) * response;
            let mut next_direction = (direction.lerp(held_tip, lag) + velocity * (dt * response))
                .try_normalize()
                .unwrap_or(direction);
            if let Some(previous) = parent_direction {
                next_direction = limit_cone(previous, next_direction, BEND_COSINE, BEND_SINE);
            }
            if let Some(&(_, cosine, sine)) = headband {
                next_direction = limit_cone(effects.spawn_direction, next_direction, cosine, sine);
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

fn smoothstep(start: f32, end: f32, value: f32) -> f32 {
    let amount = ((value - start) / (end - start)).clamp(0.0, 1.0);
    amount * amount * (3.0 - 2.0 * amount)
}

fn tangent(vector: Vec3, direction: Vec3) -> Vec3 {
    vector - direction * vector.dot(direction)
}

fn limit_cone(axis: Vec3, direction: Vec3, cosine: f32, sine: f32) -> Vec3 {
    if direction.dot(axis) >= cosine {
        return direction;
    }
    let sideways = tangent(direction, axis)
        .try_normalize()
        .unwrap_or_else(|| axis.any_orthonormal_vector());
    axis * cosine + sideways * sine
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
    const LINKS_BEFORE_SPLIT: usize = 8;
    const SEGMENTS: usize = LINKS_BEFORE_SPLIT + 1;
    const DT: f32 = 1.0 / REFERENCE_HZ;

    fn hanging_rod() -> Rod {
        let points: [Vec3; SEGMENTS + 1] =
            std::array::from_fn(|i| Vec3::NEG_Y * (LINK_LENGTH * rod_rest_units(i, SEGMENTS)));
        Rod::new(&points)
    }

    #[test]
    fn split_tip_keeps_the_rest_length_under_movement() {
        let mut rod = hanging_rod();
        for frame in 0..90 {
            let phase = frame as f32 * 0.12;
            let anchor = Vec3::new(0.05 * phase.sin(), 0.02 * phase.cos(), 0.0);
            let effects = RodEffects {
                wind_acceleration: Vec3::Z * (4.0 * phase.cos()),
                ..RodEffects::default()
            };
            rod.step(anchor, LINK_LENGTH, DT, effects, |_, point| point);
            assert_eq!(rod.points()[0], anchor);
            let mut total = 0.0;
            for (index, points) in rod.points().windows(2).enumerate() {
                let length = points[0].distance(points[1]);
                let fraction = match index {
                    i if i == SEGMENTS - 2 => 2.0 / 3.0,
                    i if i == SEGMENTS - 1 => 1.0 / 3.0,
                    _ => 1.0,
                };
                assert!((length - LINK_LENGTH * fraction).abs() < 1e-6);
                total += length;
            }
            assert!((total - LINKS_BEFORE_SPLIT as f32 * LINK_LENGTH).abs() < 1e-6);
        }
    }

    #[test]
    fn the_settle_filter_damps_small_tip_motion_and_releases_when_driven() {
        let step =
            |settle_damping, wind_acceleration, anchor, [upper_speed, tip_speed]: [f32; 2]| {
                let mut rod = hanging_rod();
                rod.direction_velocity[4] = Vec3::X * upper_speed;
                rod.direction_velocity[SEGMENTS] = Vec3::X * tip_speed;
                rod.step(
                    anchor,
                    LINK_LENGTH,
                    DT,
                    RodEffects {
                        settle_damping,
                        wind_acceleration,
                        ..RodEffects::default()
                    },
                    |_, point| point,
                );
                rod
            };
        let small_sway = [0.12, 0.12];
        let off = step(0.0, Vec3::ZERO, Vec3::ZERO, small_sway);
        let light = step(0.5, Vec3::ZERO, Vec3::ZERO, small_sway);
        let medium = step(1.0, Vec3::ZERO, Vec3::ZERO, small_sway);
        assert_eq!(off.points()[..=4], medium.points()[..=4]);
        let tip_delta = |rod: &Rod| (rod.points()[SEGMENTS] - rod.points()[SEGMENTS - 1]).x;
        assert!(tip_delta(&medium) < 0.8 * tip_delta(&off));
        assert!(tip_delta(&medium) < tip_delta(&light));
        assert!(tip_delta(&light) < tip_delta(&off));
        for (wind, anchor) in [(Vec3::X * 10.0, Vec3::ZERO), (Vec3::ZERO, Vec3::X * 0.05)] {
            let off = step(0.0, wind, anchor, small_sway);
            let medium = step(1.0, wind, anchor, small_sway);
            assert_eq!(off.points(), medium.points());
        }
        let fast_tip = [0.0, 2.0];
        let off = step(0.0, Vec3::ZERO, Vec3::ZERO, fast_tip);
        let medium = step(1.0, Vec3::ZERO, Vec3::ZERO, fast_tip);
        assert_eq!(off.points(), medium.points());
    }

    #[test]
    fn horizontal_wind_deflects_the_tip_without_moving_the_anchor() {
        let heading = (Vec3::X + Vec3::Z).normalize();
        let settled_tip = |rod: &mut Rod, acceleration| {
            let effects = RodEffects {
                wind_acceleration: heading * acceleration,
                ..RodEffects::default()
            };
            for _ in 0..720 {
                rod.step(Vec3::ZERO, LINK_LENGTH, DT, effects, |_, point| point);
                assert!(rod.points().iter().all(|point| point.is_finite()));
            }
            let tip = *rod.points().last().unwrap();
            rod.step(Vec3::ZERO, LINK_LENGTH, DT, effects, |_, point| point);
            assert_eq!(rod.points()[0], Vec3::ZERO);
            assert!(tip.distance(*rod.points().last().unwrap()) < 1e-5);
            tip
        };

        let mut rod = hanging_rod();
        let still = settled_tip(&mut rod, 0.0);
        let mild = settled_tip(&mut rod, 7.5);
        let downwind = settled_tip(&mut rod, 37.5);
        let upwind = settled_tip(&mut rod, -37.5);
        let recovered = settled_tip(&mut rod, 0.0);
        assert!(still.distance(Vec3::NEG_Y * (LINKS_BEFORE_SPLIT as f32 * LINK_LENGTH)) < 1e-6);
        assert!((mild - still).dot(heading) > 0.05);
        assert!((downwind - mild).dot(heading) > 0.05);
        assert!(downwind.distance(Vec3::new(-upwind.x, upwind.y, -upwind.z)) < 1e-5);
        assert!(recovered.distance(still) < 1e-5);
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
        assert!(weighted.points()[SEGMENTS].x < baseline.points()[SEGMENTS].x);
    }

    #[test]
    fn headband_cones_follow_the_avatar_spawn_direction() {
        for spawn_direction in [Vec3::X, Vec3::Y] {
            let mut rod = hanging_rod();
            rod.step(
                Vec3::ZERO,
                LINK_LENGTH,
                DT,
                RodEffects {
                    spawn_direction,
                    ..RodEffects::default()
                },
                |_, point| point,
            );
            let directions = rod.points().windows(2).enumerate().map(|(index, points)| {
                let offset = points[1] - points[0];
                let rest_units =
                    rod_rest_units(index + 1, SEGMENTS) - rod_rest_units(index, SEGMENTS);
                assert!((offset.length() - LINK_LENGTH * rest_units).abs() < 1e-6);
                offset.normalize()
            });
            for (direction, angle) in directions.clone().zip([15.0_f32, 22.5, 30.0]) {
                assert!(direction.dot(spawn_direction) >= angle.to_radians().cos() - 1e-6);
            }
            assert!(
                directions
                    .skip(3)
                    .any(|axis| axis.dot(spawn_direction) < BEND_COSINE)
            );
        }
    }

    #[test]
    fn angular_mobility_increases_after_each_headband_link() {
        let response = |link: usize, fixation| {
            let mut rod = hanging_rod();
            rod.direction_velocity[link] = Vec3::X;
            rod.step(
                Vec3::ZERO,
                LINK_LENGTH,
                DT,
                RodEffects {
                    fixation: HeadbandFixation::new(fixation),
                    ..RodEffects::default()
                },
                |_, point| point,
            );
            (rod.points()[link] - rod.points()[link - 1]).normalize().x
        };
        let free = response(4, 0.5);
        for (link, expected_fraction) in [0.25, 0.5, 0.75].into_iter().enumerate() {
            assert!((response(link + 1, 0.5) / free - expected_fraction).abs() < 1e-3);
        }
        for (link, expected_fraction) in [0.10, 0.25, 0.50].into_iter().enumerate() {
            assert!((response(link + 1, 0.0) / free - 1.0).abs() < 1e-6);
            assert!((response(link + 1, 1.0) / free - expected_fraction).abs() < 1e-3);
        }
    }

    #[test]
    fn headband_fixation_tightens_cones_from_the_free_rope_to_the_upper_limit() {
        let response = |fixation| {
            let mut rod = hanging_rod();
            rod.step(
                Vec3::ZERO,
                LINK_LENGTH,
                DT,
                RodEffects {
                    fixation: HeadbandFixation::new(fixation),
                    spawn_direction: Vec3::X,
                    ..RodEffects::default()
                },
                |_, point| point,
            );
            (rod.points()[1] - rod.points()[0]).normalize().x
        };
        assert_eq!(response(0.0), 0.0);
        assert!(response(0.5) < response(1.0));
        assert!(response(1.0) >= 7.5_f32.to_radians().cos() - 1e-6);
        assert!(HeadbandFixation::new(0.0).0.is_none());
        for i in 2..=10 {
            let previous = HeadbandFixation::new((i - 1) as f32 / 10.0).0.unwrap();
            let next = HeadbandFixation::new(i as f32 / 10.0).0.unwrap();
            for ((prev_mobility, prev_cosine, _), (mobility, cosine, _)) in
                previous.into_iter().zip(next)
            {
                assert!(mobility < prev_mobility);
                assert!(cosine > prev_cosine);
            }
        }
    }
}
