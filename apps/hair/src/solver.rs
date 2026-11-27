//! CPU adaptation of the D3Q19 SRT model and streamline draw in FluidX3D.
//! FluidX3D by Dr. Moritz Lehmann, reference t4ce/FluidX3D at
//! 3d46aef9d4e67c1e84f41f7ae10e9573af4bf81b. This adaptation uses two population
//! buffers in place of its in-place OpenCL streaming.

use glam::{Quat, Vec3};

use crate::obstacle::{BODY_OBSTACLES, HEAD, HEAD_CENTER, HEAD_RADIUS};

// The longest resting back-hair vertices sit about 0.733 m from the head center.
pub(crate) const HAIR_REACH_RADIUS: f32 = 0.75;
pub(crate) const FIELD_STREAMLINE_SEEDS: usize = 44;
pub(crate) const ROOT_STREAMLINE_SEEDS: usize = if cfg!(target_os = "trueos") { 32 } else { 64 };

const SIZE: [usize; 3] = [64; 3];
const CELL_COUNT: usize = SIZE[0] * SIZE[1] * SIZE[2];
const CELL_WIDTH: f32 = 0.026;
const DOMAIN_HALF: f32 = SIZE[0] as f32 * CELL_WIDTH * 0.5;
const DOMAIN_MIN: Vec3 = Vec3::new(
    HEAD_CENTER.x - DOMAIN_HALF,
    HEAD_CENTER.y - DOMAIN_HALF,
    HEAD_CENTER.z - DOMAIN_HALF,
);
const RELAXATION_RATE: f32 = 1.0 / 0.60;
// The authored roots average Y=1.418 m and the longest tip reaches Y=0.741 m.
const TIP_SPREAD_START_Y: f32 = 1.08;
const TIP_SPREAD_FULL_Y: f32 = 0.94;
const TIP_SPREAD_END_Y: f32 = 0.741;
const TIP_SPREAD_ACCELERATION: f32 = 0.00054;
// Keep the world-space sampling interval near 3.4 cm.
const STREAMLINE_STEP: f32 = 1.3;
const STREAMLINE_HALF_LENGTH: usize = 64;
const REACH_RADIUS_SQUARED: f32 = HAIR_REACH_RADIUS * HAIR_REACH_RADIUS;
// The reference's GRAPHICS_U_MAX (0.18) is in units of lattice sound speed.
const COLOR_SPEED: f32 = 0.18 * 0.577_350_26;
const COLOR_VELOCITY_DELTA: f32 = 0.035;
const FIELD_MIN_SPEED: f32 = 0.1 * COLOR_SPEED;
// The guide direction is still useful beneath the diagnostic field's cutoff.
const GUIDE_MIN_SPEED: f32 = if cfg!(target_os = "trueos") {
    FIELD_MIN_SPEED
} else {
    0.02 * COLOR_SPEED
};

const VELOCITIES: [Vec3; 19] = [
    Vec3::ZERO,
    Vec3::X,
    Vec3::NEG_X,
    Vec3::Y,
    Vec3::NEG_Y,
    Vec3::Z,
    Vec3::NEG_Z,
    Vec3::new(1.0, 1.0, 0.0),
    Vec3::new(-1.0, -1.0, 0.0),
    Vec3::new(1.0, 0.0, 1.0),
    Vec3::new(-1.0, 0.0, -1.0),
    Vec3::new(0.0, 1.0, 1.0),
    Vec3::new(0.0, -1.0, -1.0),
    Vec3::new(1.0, -1.0, 0.0),
    Vec3::new(-1.0, 1.0, 0.0),
    Vec3::new(1.0, 0.0, -1.0),
    Vec3::new(-1.0, 0.0, 1.0),
    Vec3::new(0.0, 1.0, -1.0),
    Vec3::new(0.0, -1.0, 1.0),
];
const OPPOSITE: [usize; 19] = [
    0, 2, 1, 4, 3, 6, 5, 8, 7, 10, 9, 12, 11, 14, 13, 16, 15, 18, 17,
];
const WEIGHTS: [f32; 19] = [
    1.0 / 3.0,
    1.0 / 18.0,
    1.0 / 18.0,
    1.0 / 18.0,
    1.0 / 18.0,
    1.0 / 18.0,
    1.0 / 18.0,
    1.0 / 36.0,
    1.0 / 36.0,
    1.0 / 36.0,
    1.0 / 36.0,
    1.0 / 36.0,
    1.0 / 36.0,
    1.0 / 36.0,
    1.0 / 36.0,
    1.0 / 36.0,
    1.0 / 36.0,
    1.0 / 36.0,
    1.0 / 36.0,
];
type Populations = [f32; VELOCITIES.len()];

#[derive(Clone, Copy)]
pub(crate) struct RootSample {
    pub(crate) position: Vec3,
    pub(crate) outward: Vec3,
}

pub(crate) struct Solver {
    populations: Vec<Populations>,
    next: Vec<Populations>,
    velocity: Vec<Vec3>,
    solid: Vec<bool>,
    fluid_cells: Vec<(usize, f32)>,
    boundary_cells: Vec<usize>,
    pull_offsets: [isize; VELOCITIES.len()],
    upright_flow_direction: Vec3,
    world_to_avatar: Quat,
    field_seeds: [Vec3; FIELD_STREAMLINE_SEEDS],
    root_positions: [Vec3; ROOT_STREAMLINE_SEEDS],
    root_seeds: [Vec3; ROOT_STREAMLINE_SEEDS],
    tick: u64,
}

impl Solver {
    pub(crate) fn new(
        hair_down: Vec3,
        world_to_avatar: Quat,
        roots: [RootSample; ROOT_STREAMLINE_SEEDS],
    ) -> Self {
        let upright_flow_direction = hair_down.normalize();
        let inflow = inlet_velocity(world_to_avatar * upright_flow_direction, 0);
        let equilibrium = equilibrium(1.0, inflow);
        let mut solid = vec![false; CELL_COUNT];
        let mut velocity = vec![inflow; CELL_COUNT];
        let mut fluid_cells = Vec::with_capacity(CELL_COUNT);
        let mut boundary_cells = Vec::new();
        for z in 0..SIZE[2] {
            for y in 0..SIZE[1] {
                for x in 0..SIZE[0] {
                    let index = index(x, y, z);
                    let position = world_position(Vec3::new(x as f32, y as f32, z as f32) + 0.5);
                    if HEAD.contains(position)
                        || BODY_OBSTACLES.iter().any(|shape| shape.contains(position))
                    {
                        solid[index] = true;
                        velocity[index] = Vec3::ZERO;
                    } else if [x, y, z]
                        .into_iter()
                        .zip(SIZE)
                        .any(|(coordinate, size)| coordinate == 0 || coordinate + 1 == size)
                    {
                        boundary_cells.push(index);
                    } else {
                        fluid_cells.push((index, tip_spread_acceleration(position)));
                    }
                }
            }
        }
        let pull_offsets = VELOCITIES.map(|direction| {
            -(direction.x as isize
                + direction.y as isize * SIZE[0] as isize
                + direction.z as isize * (SIZE[0] * SIZE[1]) as isize)
        });
        Self {
            populations: vec![equilibrium; CELL_COUNT],
            next: vec![equilibrium; CELL_COUNT],
            velocity,
            solid,
            fluid_cells,
            boundary_cells,
            pull_offsets,
            upright_flow_direction,
            world_to_avatar,
            field_seeds: field_seeds(upright_flow_direction),
            root_positions: roots.map(|root| root.position),
            root_seeds: roots.map(root_seed),
            tick: 0,
        }
    }

    pub(crate) fn step(&mut self, world_to_avatar: Quat, tip_spread: bool) {
        self.tick += 1;
        self.world_to_avatar = world_to_avatar;
        let inflow = inlet_velocity(world_to_avatar * self.upright_flow_direction, self.tick);
        let boundary_equilibrium = equilibrium(1.0, inflow);
        for &cell in &self.boundary_cells {
            self.next[cell] = boundary_equilibrium;
            self.velocity[cell] = inflow;
        }

        let spread_scale = if tip_spread { 1.0 } else { 0.0 };
        for &(cell, spread_acceleration) in &self.fluid_cells {
            let mut incoming = [0.0; VELOCITIES.len()];
            let mut density = 1.0;
            let mut momentum = Vec3::ZERO;
            for (direction, population) in incoming.iter_mut().enumerate() {
                let source = cell.wrapping_add_signed(self.pull_offsets[direction]);
                // Mid-grid bounce-back returns the population leaving toward a solid cell.
                *population = if self.solid[source] {
                    self.populations[cell][OPPOSITE[direction]]
                } else {
                    self.populations[source][direction]
                };
                density += *population;
                momentum += VELOCITIES[direction] * *population;
            }
            let velocity = momentum / density;
            let acceleration = Vec3::X * (spread_scale * spread_acceleration);
            self.velocity[cell] = velocity + 0.5 * acceleration;
            // Shifting equilibrium by a / omega gives the collision a momentum
            // increment of density * a while its zeroth moment stays density.
            let equilibrium = equilibrium(density, velocity + acceleration / RELAXATION_RATE);
            for (direction, population) in incoming.into_iter().enumerate() {
                self.next[cell][direction] =
                    population + RELAXATION_RATE * (equilibrium[direction] - population);
            }
        }
        core::mem::swap(&mut self.populations, &mut self.next);
    }

    /// Emit diagnostic field segments inside the hair reach.
    pub(crate) fn for_each_field_line(&self, mut emit: impl FnMut(Vec3, Vec3, f32, f32)) -> usize {
        let mut segments = 0;
        let center = (HEAD_CENTER - DOMAIN_MIN) / CELL_WIDTH;
        for &seed in &self.field_seeds {
            // Keep the field disk upstream in world coordinates. Root seeds
            // stay fixed to the avatar because the grid is in avatar space.
            let seed = center + self.world_to_avatar * (seed - center);
            for direction in [-1.0, 1.0] {
                segments += self.trace(seed, direction, FIELD_MIN_SPEED, &mut emit);
            }
        }
        segments
    }

    /// Emit downstream guide segments from the authored root patch.
    /// The first fluid segment of each guide also carries its surface attachment.
    pub(crate) fn for_each_guide_line(
        &self,
        mut emit: impl FnMut(Option<Vec3>, Vec3, Vec3, f32, f32),
    ) -> usize {
        self.root_positions
            .iter()
            .zip(&self.root_seeds)
            .map(|(&position, &seed)| {
                let mut attachment = Some(position);
                self.trace(
                    seed,
                    1.0,
                    GUIDE_MIN_SPEED,
                    &mut |from, to, speed, change| {
                        emit(attachment.take(), from, to, speed, change);
                    },
                )
            })
            .sum()
    }

    fn trace(
        &self,
        seed: Vec3,
        direction: f32,
        minimum_speed: f32,
        emit: &mut impl FnMut(Vec3, Vec3, f32, f32),
    ) -> usize {
        let Some(mut velocity) = self.velocity_at(seed) else {
            return 0;
        };
        let mut point = seed;
        let mut from = world_position(seed);
        let mut segments = 0;
        for _ in 0..STREAMLINE_HALF_LENGTH {
            let speed = velocity.length();
            if speed < minimum_speed {
                break;
            }
            let mut next = point + (STREAMLINE_STEP * direction / speed) * velocity;
            let mut to = world_position(next);
            let exits_reach = to.distance_squared(HEAD_CENTER) > REACH_RADIUS_SQUARED;
            if exits_reach {
                let fraction = reach_exit_fraction(from, to);
                if fraction <= 0.0 {
                    break;
                }
                next = point.lerp(next, fraction);
                to = from.lerp(to, fraction);
            }
            let Some(next_velocity) = self.velocity_at(next) else {
                break;
            };
            // A step can cross a proxy between two fluid samples.
            if HEAD.intersects(from, to)
                || BODY_OBSTACLES
                    .iter()
                    .any(|shape| shape.intersects(from, to))
            {
                break;
            }
            let normalized_speed = (speed / COLOR_SPEED).clamp(0.0, 1.0);
            let delta = (next_velocity.distance(velocity) / COLOR_VELOCITY_DELTA).clamp(0.0, 1.0);
            emit(from, to, normalized_speed, delta);
            segments += 1;
            if exits_reach {
                break;
            }
            point = next;
            from = to;
            velocity = next_velocity;
        }
        segments
    }

    fn velocity_at(&self, point: Vec3) -> Option<Vec3> {
        if !point.is_finite()
            || point.x < 1.0
            || point.y < 1.0
            || point.z < 1.0
            || point.x >= (SIZE[0] - 1) as f32
            || point.y >= (SIZE[1] - 1) as f32
            || point.z >= (SIZE[2] - 1) as f32
        {
            return None;
        }
        let cell = index(point.x as usize, point.y as usize, point.z as usize);
        (!self.solid[cell]).then_some(self.velocity[cell])
    }
}

fn field_seeds(flow_direction: Vec3) -> [Vec3; FIELD_STREAMLINE_SEEDS] {
    // Map Hammersley points to equal-area rings in the upstream slice of the sphere.
    let upstream = HEAD_RADIUS + CELL_WIDTH;
    let center = HEAD_CENTER - upstream * flow_direction;
    let disk_radius = (REACH_RADIUS_SQUARED - upstream * upstream).sqrt();
    let (u, v) = flow_direction.any_orthonormal_pair();
    core::array::from_fn(|index| {
        let area_fraction = (index as f32 + 0.5) / FIELD_STREAMLINE_SEEDS as f32;
        let turns = ((index as u32).reverse_bits() as f64 / 4_294_967_296.0) as f32;
        let (sin, cos) = (core::f32::consts::TAU * turns).sin_cos();
        let radius = disk_radius * area_fraction.sqrt();
        let position = center + radius * (cos * u + sin * v);
        (position - DOMAIN_MIN) / CELL_WIDTH
    })
}

fn root_seed(root: RootSample) -> Vec3 {
    // Follow the local surface normal to a sampling position with one cell of
    // clearance from the head proxy, including room for voxel discretization.
    let radius = HEAD_RADIUS + CELL_WIDTH;
    let offset = root.position - HEAD_CENTER;
    let along = offset.dot(root.outward);
    let inside = (radius * radius - offset.length_squared()).max(0.0);
    let distance = (along * along + inside).sqrt() - along;
    let position = root.position + distance * root.outward;
    (position - DOMAIN_MIN) / CELL_WIDTH
}

fn inlet_velocity(flow_direction: Vec3, tick: u64) -> Vec3 {
    let phase = (tick % 360) as f32 * (core::f32::consts::TAU / 360.0);
    flow_direction * (0.074 * (1.0 + 0.16 * phase.sin()))
}

fn tip_spread_acceleration(position: Vec3) -> f32 {
    let along = 1.0 - smoothstep(TIP_SPREAD_FULL_Y, TIP_SPREAD_START_Y, position.y);
    let below_tip = smoothstep(0.68, TIP_SPREAD_END_Y, position.y);
    // Build the outward force farther down and behind the torso so that
    // the wider tip paths can still clear the hips.
    let depth = smoothstep(0.13, 0.16, position.z) * (1.0 - smoothstep(0.19, 0.24, position.z));
    let edge = 1.0 - smoothstep(0.07, 0.16, position.x.abs());
    let reach = 1.0
        - smoothstep(
            0.71 * 0.71,
            REACH_RADIUS_SQUARED,
            position.distance_squared(HEAD_CENTER),
        );
    let outward = (position.x / 0.06).clamp(-1.0, 1.0);
    TIP_SPREAD_ACCELERATION * along * below_tip * depth * edge * reach * outward
}

fn smoothstep(low: f32, high: f32, value: f32) -> f32 {
    let t = ((value - low) / (high - low)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn equilibrium(density: f32, velocity: Vec3) -> Populations {
    // Store populations relative to their rest weights, as in FluidX3D.
    let square = 1.5 * velocity.length_squared();
    core::array::from_fn(|direction| {
        let projection = 3.0 * velocity.dot(VELOCITIES[direction]);
        WEIGHTS[direction]
            * ((density - 1.0) + density * (projection + 0.5 * projection * projection - square))
    })
}

fn index(x: usize, y: usize, z: usize) -> usize {
    x + (y + z * SIZE[1]) * SIZE[0]
}

fn world_position(lattice: Vec3) -> Vec3 {
    DOMAIN_MIN + lattice * CELL_WIDTH
}

// The start is inside and the end is outside, so the larger root marks the exit.
fn reach_exit_fraction(from: Vec3, to: Vec3) -> f32 {
    let offset = from - HEAD_CENTER;
    let segment = to - from;
    let a = segment.length_squared();
    let b = offset.dot(segment);
    let c = offset.length_squared() - REACH_RADIUS_SQUARED;
    ((-b + (b * b - a * c).sqrt()) / a).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn standing_solver() -> Solver {
        Solver::new(Vec3::NEG_Y, Quat::IDENTITY, crate::scene::reference_roots())
    }

    #[test]
    fn equilibrium_preserves_density_and_momentum() {
        let density = 1.012;
        let velocity = Vec3::new(0.08, -0.01, 0.025);
        let populations = equilibrium(density, velocity);
        let actual_density = 1.0 + populations.iter().sum::<f32>();
        let actual_momentum: Vec3 = VELOCITIES
            .into_iter()
            .zip(populations)
            .map(|(direction, population)| direction * population)
            .sum();
        assert!((actual_density - density).abs() < 1e-6);
        assert!(actual_momentum.distance(density * velocity) < 1e-6);
    }

    #[test]
    fn tip_spread_adds_outward_velocity_in_the_lower_rear_field() {
        let mut baseline = standing_solver();
        let mut spread = standing_solver();
        baseline.step(Quat::IDENTITY, false);
        spread.step(Quat::IDENTITY, true);
        let delta = |position| {
            let lattice = (position - DOMAIN_MIN) / CELL_WIDTH;
            spread.velocity_at(lattice).unwrap() - baseline.velocity_at(lattice).unwrap()
        };
        assert!(delta(Vec3::new(0.09, 0.87, 0.16)).x > 0.000_08);
        assert!(delta(Vec3::new(-0.09, 0.87, 0.16)).x < -0.000_08);
        assert_eq!(delta(Vec3::new(0.09, 1.16, 0.16)), Vec3::ZERO);
        assert_eq!(delta(Vec3::new(0.09, 0.87, -0.22)), Vec3::ZERO);
    }

    #[test]
    fn reach_extrema_and_streamline_seeds_remain_in_the_solved_interior() {
        let solver = standing_solver();
        let axes = [Vec3::X, Vec3::Y, Vec3::Z];
        for axis in axes {
            for sign in [-1.0, 1.0] {
                let edge = HEAD_CENTER + sign * HAIR_REACH_RADIUS * axis;
                let lattice = (edge - DOMAIN_MIN) / CELL_WIDTH;
                assert!(lattice.min_element() >= 1.0);
                assert!(lattice.max_element() < (SIZE[0] - 1) as f32);
            }
        }
        let mut quadrants = [0; 4];
        for &seed in &solver.field_seeds {
            assert!(solver.velocity_at(seed).is_some());
            let offset = world_position(seed) - HEAD_CENTER;
            assert!(offset.length_squared() < REACH_RADIUS_SQUARED);
            assert!((offset.y - (HEAD_RADIUS + CELL_WIDTH)).abs() < 1e-6);
            let quadrant = 2 * usize::from(offset.x < 0.0) + usize::from(offset.z < 0.0);
            quadrants[quadrant] += 1;
        }
        assert!(quadrants.into_iter().all(|count| (9..=13).contains(&count)));
        assert_eq!(solver.field_seeds.len(), 44);
        let roots = crate::scene::reference_roots();
        for (&seed, root) in solver.root_seeds.iter().zip(roots) {
            let position = world_position(seed);
            assert!(solver.velocity_at(seed).is_some());
            let direction = (position - root.position).normalize();
            assert!(direction.distance(root.outward) < 1e-5);
            assert!(position.distance(HEAD_CENTER) >= HEAD_RADIUS + CELL_WIDTH - 1e-6);
        }
        let mut attached = 0;
        solver.for_each_guide_line(|attachment, from, _, _, _| {
            if let Some(position) = attachment {
                assert_eq!(position, roots[attached].position);
                assert_eq!(from, world_position(solver.root_seeds[attached]));
                attached += 1;
            }
        });
        assert_eq!(attached, ROOT_STREAMLINE_SEEDS);
    }

    #[test]
    fn obstacles_change_the_field_and_streamlines_remain_outside() {
        let mut solver = standing_solver();
        for _ in 0..18 {
            solver.step(Quat::IDENTITY, true);
        }
        let head = (HEAD_CENTER - DOMAIN_MIN) / CELL_WIDTH;
        assert!(solver.velocity_at(head).is_none());
        for shape in BODY_OBSTACLES {
            let center = (shape.center - DOMAIN_MIN) / CELL_WIDTH;
            assert!(solver.velocity_at(center).is_none());
        }
        assert!(solver.velocity.iter().all(|velocity| velocity.is_finite()));
        let sample = |position| {
            solver
                .velocity_at((position - DOMAIN_MIN) / CELL_WIDTH)
                .unwrap()
        };
        let inflow = inlet_velocity(Vec3::NEG_Y, solver.tick);
        let free_flow = sample(Vec3::new(0.53, 1.35, 0.45));
        assert!(free_flow.distance(inflow) < 0.005);
        assert!(free_flow.dot(Vec3::NEG_Y) > 0.06);
        let arm_wake = sample(Vec3::new(0.430, 1.10, 0.016));
        assert!(arm_wake.dot(Vec3::NEG_Y) < free_flow.dot(Vec3::NEG_Y) * 0.96);
        let mut roots_below_shoulders = 0;
        for &seed in &solver.root_seeds {
            let mut lowest = world_position(seed).y;
            assert!(
                solver.trace(seed, 1.0, GUIDE_MIN_SPEED, &mut |_, to, _, _| {
                    lowest = lowest.min(to.y);
                }) > 0
            );
            roots_below_shoulders += usize::from(lowest < 1.15);
        }
        assert!(
            roots_below_shoulders >= ROOT_STREAMLINE_SEEDS * 3 / 4,
            "{roots_below_shoulders} roots below the shoulders"
        );
        let mut count = 0;
        let mut varied = false;
        let mut reach_exits = [false; 2];
        let mut observe = |from: Vec3, to: Vec3, speed, delta| {
            let from_radius = from.distance(HEAD_CENTER);
            let to_radius = to.distance(HEAD_CENTER);
            assert!(from_radius <= HAIR_REACH_RADIUS + 1e-6);
            assert!(to_radius <= HAIR_REACH_RADIUS + 1e-6);
            assert!(!HEAD.intersects(from, to));
            assert!(
                !BODY_OBSTACLES
                    .iter()
                    .any(|shape| shape.intersects(from, to))
            );
            assert!((0.0..=1.0).contains(&speed) && (0.0..=1.0).contains(&delta));
            count += 1;
            varied |= delta > 0.2;
            if (to_radius - HAIR_REACH_RADIUS).abs() < 1e-6 {
                reach_exits[usize::from(to.y > from.y)] = true;
            }
        };
        let field_segments = solver.for_each_field_line(&mut observe);
        let guide_segments =
            solver.for_each_guide_line(|_, from, to, speed, delta| observe(from, to, speed, delta));
        assert_eq!(field_segments + guide_segments, count);
        assert!(
            (1_200..=1_600).contains(&field_segments) && varied,
            "{field_segments} field segments, {guide_segments} guide segments"
        );
        assert!(guide_segments >= ROOT_STREAMLINE_SEEDS * 10);
        assert_eq!(reach_exits, [true; 2]);
    }

    #[cfg(not(target_os = "trueos"))]
    #[test]
    fn slow_guides_stay_visible_to_the_reach_boundary() {
        let mut solver = standing_solver();
        let seed = (HEAD_CENTER + 0.20 * Vec3::Z - DOMAIN_MIN) / CELL_WIDTH;
        solver.velocity.fill(0.05 * COLOR_SPEED * Vec3::Z);
        assert_eq!(
            solver.trace(seed, 1.0, FIELD_MIN_SPEED, &mut |_, _, _, _| {}),
            0
        );
        let mut tip = Vec3::ZERO;
        let segments = solver.trace(seed, 1.0, GUIDE_MIN_SPEED, &mut |_, to, _, _| tip = to);
        assert!(segments > 0);
        assert!(tip.distance(HEAD_CENTER + HAIR_REACH_RADIUS * Vec3::Z) < 1e-6);

        solver.velocity.fill(Vec3::ZERO);
        assert_eq!(
            solver.trace(seed, 1.0, GUIDE_MIN_SPEED, &mut |_, _, _, _| {}),
            0
        );
    }

    #[test]
    fn inverted_inlet_stays_world_down_and_preserves_interior_flow() {
        let mut solver = standing_solver();
        let interior = Vec3::new(52.0, 32.0, 50.0);
        let before = solver.velocity_at(interior).unwrap();
        let inverted = Quat::from_rotation_z(-core::f32::consts::PI);
        solver.step(inverted, true);
        let world_inlet = inverted.conjugate() * solver.velocity[index(0, 32, 0)];
        assert!(world_inlet.distance(inlet_velocity(Vec3::NEG_Y, 1)) < 1e-6);
        assert!(solver.velocity_at(interior).unwrap().distance(before) < 1e-6);
    }
}
