use glam::{IVec3, Vec3};

use crate::{
    obstacle::{BODY_OBSTACLES, HEAD_CENTER},
    solver::{HAIR_REACH_RADIUS, Solver},
    water_iso,
};

// A local 15 mm fill grid feeds the medium-quality triangle pass. Each frame
// reconstructs the liquid-shaped surface from the existing flow trajectories.
const STEP: f32 = 0.015;

#[derive(Clone, Copy)]
struct PathPoint {
    position: Vec3,
    distance: f32,
}

#[derive(Default)]
pub(crate) struct WaterPass {
    phase: f32,
    paths: Vec<usize>,
    points: Vec<PathPoint>,
    fill: Vec<f32>,
    pub(crate) vertices: Vec<Vec3>,
    pub(crate) triangles: Vec<[u32; 3]>,
}

impl WaterPass {
    pub(crate) fn advance(&mut self, speed: f32) {
        self.phase = (self.phase + 0.18 * speed).rem_euclid(core::f32::consts::TAU);
    }

    pub(crate) fn rebuild(&mut self, solver: &Solver, volume: f32) {
        self.paths.clear();
        self.points.clear();
        solver.for_each_guide_line(|attachment, _, to, _, _| {
            if let Some(root) = attachment {
                self.paths.push(self.points.len());
                self.points.push(PathPoint {
                    position: root,
                    distance: 0.0,
                });
            }
            self.push_point(to);
        });
        if self.points.is_empty() {
            self.vertices.clear();
            self.triangles.clear();
            return;
        }
        self.paths.push(self.points.len());

        let (min, max) = self.points.iter().fold(
            (Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY)),
            |(min, max), point| (min.min(point.position), max.max(point.position)),
        );
        let thickness = volume.sqrt();
        let pad = 0.05 * thickness + STEP;
        let origin = ((min - pad) / STEP).floor() * STEP;
        let upper = ((max + pad - origin) / STEP).ceil().as_ivec3();
        let extent = (upper + IVec3::ONE).to_array().map(|size| size as usize);
        self.fill.resize(extent.into_iter().product(), 0.0);
        self.fill.fill(0.0);

        for path in self.paths.windows(2) {
            let points = &self.points[path[0]..path[1]];
            let length = points.last().unwrap().distance.max(STEP);
            for segment in points.windows(2) {
                let from = segment[0];
                let to = segment[1];
                let radii = [from, to].map(|point| radius(point, length, thickness, self.phase));
                deposit(
                    &mut self.fill,
                    extent,
                    origin,
                    upper,
                    from.position,
                    to.position,
                    radii,
                );
            }
        }

        for (index, fill) in self.fill.iter_mut().enumerate() {
            if *fill == 0.0 {
                continue;
            }
            let x = index % extent[0];
            let y = index / extent[0] % extent[1];
            let z = index / (extent[0] * extent[1]);
            let point = origin + STEP * Vec3::new(x as f32, y as f32, z as f32);
            if point.distance_squared(HEAD_CENTER) >= HAIR_REACH_RADIUS * HAIR_REACH_RADIUS
                || BODY_OBSTACLES
                    .iter()
                    .any(|obstacle| obstacle.contains(point))
            {
                *fill = 0.0;
            }
        }
        water_iso::extract(
            extent,
            origin,
            STEP,
            &self.fill,
            &mut self.vertices,
            &mut self.triangles,
        );
    }

    fn push_point(&mut self, position: Vec3) {
        let previous = self.points.last().unwrap();
        self.points.push(PathPoint {
            position,
            distance: previous.distance + position.distance(previous.position),
        });
    }
}

fn radius(point: PathPoint, length: f32, thickness: f32, phase: f32) -> f32 {
    let along = point.distance / length;
    let body = 0.020 + 0.015 * (core::f32::consts::PI * along).sin();
    let taper = 1.0 - 0.48 * ((along - 0.85) / 0.15).clamp(0.0, 1.0);
    let ripple = 1.0 + 0.13 * (point.distance * 65.0 - phase + point.position.x * 36.0).sin();
    body * taper * ripple * thickness
}

fn deposit(
    fill: &mut [f32],
    extent: [usize; 3],
    origin: Vec3,
    upper: IVec3,
    from: Vec3,
    to: Vec3,
    radii: [f32; 2],
) {
    let pad = radii[0].max(radii[1]) + 0.5 * STEP;
    let low = ((from.min(to) - pad - origin) / STEP)
        .floor()
        .as_ivec3()
        .max(IVec3::ZERO);
    let high = ((from.max(to) + pad - origin) / STEP)
        .ceil()
        .as_ivec3()
        .min(upper);
    let segment = to - from;
    let inverse_length = segment.length_squared().max(1e-9).recip();
    for z in low.z..=high.z {
        for y in low.y..=high.y {
            for x in low.x..=high.x {
                let point = origin + STEP * Vec3::new(x as f32, y as f32, z as f32);
                let alpha = ((point - from).dot(segment) * inverse_length).clamp(0.0, 1.0);
                let center = from + alpha * segment;
                let radius = radii[0] + alpha * (radii[1] - radii[0]);
                let value = (0.5 + (radius - point.distance(center)) / STEP).clamp(0.0, 1.0);
                let cell = (z as usize * extent[1] + y as usize) * extent[0] + x as usize;
                fill[cell] = fill[cell].max(value);
            }
        }
    }
}
