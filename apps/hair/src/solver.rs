//! CPU adaptation of the D3Q19 SRT model and streamline draw in FluidX3D.
//! FluidX3D by Dr. Moritz Lehmann, reference t4ce/FluidX3D at
//! 3d46aef9d4e67c1e84f41f7ae10e9573af4bf81b. This adaptation uses two population
//! buffers in place of its in-place OpenCL streaming.

use glam::Vec3;

use crate::obstacle::{BODY_OBSTACLES, HEAD, HEAD_CENTER, HEAD_RADIUS};

// The longest resting back-hair vertices sit about 0.733 m from the head center.
pub(crate) const HAIR_REACH_RADIUS: f32 = 0.75;
pub(crate) const STREAMLINE_SEEDS: usize = 120;

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
// Keep the world-space sampling interval near 3.4 cm.
const STREAMLINE_STEP: f32 = 1.3;
const STREAMLINE_HALF_LENGTH: usize = 64;
const REACH_RADIUS_SQUARED: f32 = HAIR_REACH_RADIUS * HAIR_REACH_RADIUS;
// The reference's GRAPHICS_U_MAX (0.18) is in units of lattice sound speed.
const COLOR_SPEED: f32 = 0.18 * 0.577_350_26;
const COLOR_VELOCITY_DELTA: f32 = 0.035;

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

pub(crate) struct Solver {
    populations: Vec<Populations>,
    next: Vec<Populations>,
    velocity: Vec<Vec3>,
    solid: Vec<bool>,
    fluid_cells: Vec<usize>,
    boundary_cells: Vec<usize>,
    pull_offsets: [isize; VELOCITIES.len()],
    flow_direction: Vec3,
    seeds: [Vec3; STREAMLINE_SEEDS],
    tick: u64,
}

impl Solver {
    pub(crate) fn new(hair_down: Vec3) -> Self {
        let flow_direction = hair_down.normalize();
        let inflow = inlet_velocity(flow_direction, 0);
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
                        fluid_cells.push(index);
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
            flow_direction,
            seeds: streamline_seeds(flow_direction),
            tick: 0,
        }
    }

    pub(crate) fn step(&mut self) {
        self.tick += 1;
        let inflow = inlet_velocity(self.flow_direction, self.tick);
        let boundary_equilibrium = equilibrium(1.0, inflow);
        for &cell in &self.boundary_cells {
            self.next[cell] = boundary_equilibrium;
            self.velocity[cell] = inflow;
        }

        for &cell in &self.fluid_cells {
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
            self.velocity[cell] = velocity;
            let equilibrium = equilibrium(density, velocity);
            for (direction, population) in incoming.into_iter().enumerate() {
                self.next[cell][direction] =
                    population + RELAXATION_RATE * (equilibrium[direction] - population);
            }
        }
        core::mem::swap(&mut self.populations, &mut self.next);
    }

    /// Emit line segments inside the hair reach, with local speed and velocity change.
    pub(crate) fn for_each_line(&self, mut emit: impl FnMut(Vec3, Vec3, f32, f32)) -> usize {
        let mut segments = 0;
        for &seed in &self.seeds {
            for direction in [-1.0, 1.0] {
                segments += self.trace(seed, direction, &mut emit);
            }
        }
        segments
    }

    fn trace(
        &self,
        seed: Vec3,
        direction: f32,
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
            if speed < 0.1 * COLOR_SPEED {
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

fn streamline_seeds(flow_direction: Vec3) -> [Vec3; STREAMLINE_SEEDS] {
    // Map Hammersley points to equal-area rings in the upstream slice of the sphere.
    let upstream = HEAD_RADIUS + CELL_WIDTH;
    let center = HEAD_CENTER - upstream * flow_direction;
    let disk_radius = (REACH_RADIUS_SQUARED - upstream * upstream).sqrt();
    let (u, v) = flow_direction.any_orthonormal_pair();
    core::array::from_fn(|index| {
        let area_fraction = (index as f32 + 0.5) / STREAMLINE_SEEDS as f32;
        let turns = ((index as u32).reverse_bits() as f64 / 4_294_967_296.0) as f32;
        let (sin, cos) = (core::f32::consts::TAU * turns).sin_cos();
        let radius = disk_radius * area_fraction.sqrt();
        let position = center + radius * (cos * u + sin * v);
        (position - DOMAIN_MIN) / CELL_WIDTH
    })
}

fn inlet_velocity(flow_direction: Vec3, tick: u64) -> Vec3 {
    let phase = (tick % 360) as f32 * (core::f32::consts::TAU / 360.0);
    flow_direction * (0.074 * (1.0 + 0.16 * phase.sin()))
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
    fn reach_extrema_and_streamline_seeds_remain_in_the_solved_interior() {
        let solver = Solver::new(Vec3::NEG_Y);
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
        for &seed in &solver.seeds {
            assert!(solver.velocity_at(seed).is_some());
            let offset = world_position(seed) - HEAD_CENTER;
            assert!(offset.length_squared() < REACH_RADIUS_SQUARED);
            assert!((offset.y - (HEAD_RADIUS + CELL_WIDTH)).abs() < 1e-6);
            let quadrant = 2 * usize::from(offset.x < 0.0) + usize::from(offset.z < 0.0);
            quadrants[quadrant] += 1;
        }
        assert!(
            quadrants
                .into_iter()
                .all(|count| (28..=32).contains(&count))
        );
    }

    #[test]
    fn obstacles_change_the_field_and_streamlines_remain_outside() {
        let mut solver = Solver::new(Vec3::NEG_Y);
        for _ in 0..18 {
            solver.step();
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
        let mut count = 0;
        let mut varied = false;
        let mut reach_exits = [false; 2];
        let emitted = solver.for_each_line(|from, to, speed, delta| {
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
        });
        assert_eq!(emitted, count);
        assert!((2_800..=4_000).contains(&emitted) && varied);
        assert_eq!(reach_exits, [true; 2]);
    }
}
