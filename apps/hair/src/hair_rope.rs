use std::{
    collections::HashMap,
    f32::consts::{FRAC_1_SQRT_2, TAU},
};

use glam::{IVec3, Quat, Vec3};

use crate::{
    hair_rod::{HeadbandFixation, Rod, RodEffects},
    hair_spring::Spring,
    obstacle::{BODY_OBSTACLES, HEAD, downward_head_exit, rotate_about_head},
};

pub(crate) const POINTS: usize = 13;
pub(crate) const MAX_SEGMENTS: usize = 12;
pub(crate) const GRAVITY: Vec3 = Vec3::new(0.0, -9.81, 0.0);
const FRAME_SECONDS: f32 = 1.0 / 45.0;
const SUBSTEPS: u32 = 4;
const ROD_SPEED: f32 = 0.9;
const ROD_SETTLE_DAMPING: f32 = 1.0; // Medium
const TIP_WEIGHT: f32 = 0.5;
const MAX_WIND_ACCELERATION: f32 = 37.5;
const WIND_RESPONSE_SECONDS: f32 = 0.35;
const GUST_HZ: f32 = 0.18;
const GUST_WAVELENGTH: f32 = 1.35;
const GUST_GAIN: f32 = 0.70;
const WIND_HEADINGS: [(Vec3, &str); 8] = [
    (Vec3::X, "+X"),
    (Vec3::new(FRAC_1_SQRT_2, 0.0, FRAC_1_SQRT_2), "+X/+Z"),
    (Vec3::Z, "+Z"),
    (Vec3::new(-FRAC_1_SQRT_2, 0.0, FRAC_1_SQRT_2), "-X/+Z"),
    (Vec3::NEG_X, "-X"),
    (Vec3::new(-FRAC_1_SQRT_2, 0.0, -FRAC_1_SQRT_2), "-X/-Z"),
    (Vec3::NEG_Z, "-Z"),
    (Vec3::new(FRAC_1_SQRT_2, 0.0, -FRAC_1_SQRT_2), "+X/-Z"),
];
#[cfg(target_os = "linux")]
pub(crate) const WIND_HEADING_COUNT: u8 = WIND_HEADINGS.len() as u8;
const UPRIGHT_FIXATION_PERCENT: f32 = 50.0;
// Within 15° of vertical, hold the endpoint so a screen-plane half turn
// reaches the inverted setting even with the camera's slight elevation.
const FIXATION_VERTICAL_ALIGNMENT: f32 = 0.965_925_8;
const CLEARANCE: f32 = 0.009;
const SPREAD_STATIONS: usize = 4;
const SPREAD_PEAK_PROGRESS: f32 = 0.90;
const SPREAD_RADIUS: f32 = 0.11;
const SPREAD_NEIGHBOR_SCALE: f32 = 5.0;
const MAX_SPREAD_ACCELERATION: f32 = 13.0;
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct RodTuning {
    pub(crate) wind_strength_percent: u8,
    pub(crate) wind_heading: u8,
    spread_width_percent: u8,
}

impl Default for RodTuning {
    fn default() -> Self {
        Self {
            wind_strength_percent: 0,
            wind_heading: 1,
            spread_width_percent: 100,
        }
    }
}

impl RodTuning {
    fn wind_target(self) -> Vec3 {
        let strength = f32::from(self.wind_strength_percent) / 100.0;
        WIND_HEADINGS[self.wind_heading as usize].0 * (MAX_WIND_ACCELERATION * strength)
    }

    #[cfg(target_os = "linux")]
    pub(crate) fn wind_heading_label(self) -> &'static str {
        WIND_HEADINGS[self.wind_heading as usize].1
    }
}

pub(crate) fn fixation_percent_for(spawn_direction: Vec3) -> f32 {
    let alignment = spawn_direction.dot(Vec3::NEG_Y);
    let position = (0.5 + 0.5 * alignment / FIXATION_VERTICAL_ALIGNMENT).clamp(0.0, 1.0);
    UPRIGHT_FIXATION_PERCENT * (position * position * (3.0 - 2.0 * position))
}

pub(crate) struct RopeGuides {
    strands: Vec<Strand>,
    motion: HairMotion,
    pose: Quat,
    step_seconds: f32,
    wind_baseline: Vec3,
    gust_phase: f32,
    density: [HashMap<IVec3, DensityCell>; SPREAD_STATIONS],
}

struct Strand {
    origin: Vec3,
    launch: Vec3,
    link_length: f32,
    gust_gain: f32,
    spread_weights: [f32; MAX_SEGMENTS + 1],
    spread_acceleration: [Vec3; MAX_SEGMENTS + 1],
    chain: Chain,
}

struct CutProfile {
    center_x: f32,
    half_width: f32,
    low_y: f32,
    height: f32,
}

impl CutProfile {
    fn new(roots: &[Vec3]) -> Self {
        let first = roots.first().copied().unwrap_or(Vec3::ZERO);
        let (low, high) = roots.iter().fold((first, first), |(low, high), &root| {
            (low.min(root), high.max(root))
        });
        Self {
            center_x: (low.x + high.x) * 0.5,
            half_width: ((high.x - low.x) * 0.5).max(1e-4),
            low_y: low.y,
            height: (high.y - low.y).max(1e-4),
        }
    }

    fn retained_length(&self, root: Vec3) -> f32 {
        let side = (root.x - self.center_x).abs() / self.half_width;
        let height = (root.y - self.low_y) / self.height;
        let edge_trim = 0.20 * smoothstep(0.70, 1.0, side);
        let layers = smoothstep(0.41, 0.52, height)
            + smoothstep(0.60, 0.71, height)
            + smoothstep(0.79, 0.90, height);
        (1.0 - edge_trim - 0.06 * layers).max(0.60)
    }
}

#[derive(Default)]
struct DensityCell {
    sum: Vec3,
    guides: u32,
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
        effects: RodEffects,
        project: impl FnMut(usize, Vec3) -> Vec3,
    ) {
        match self {
            Self::Rope(chain) => chain.step(anchor, length, dt, project),
            Self::Spring(chain) => chain.step(anchor, length, dt, project),
            Self::Rod(chain) => chain.step(anchor, length, dt, effects, project),
        }
    }
}

impl RopeGuides {
    pub(crate) fn new(roots: &[Vec3], tip_y: f32, pose: Quat, motion: HairMotion) -> Self {
        Self::with_extra_roots(roots, roots.len(), tip_y, pose, motion)
    }

    pub(crate) fn with_extra_roots(
        roots: &[Vec3],
        base_count: usize,
        tip_y: f32,
        pose: Quat,
        motion: HairMotion,
    ) -> Self {
        assert!(base_count <= roots.len() && (base_count > 0 || roots.is_empty()));
        let avatar_to_world = pose.conjugate();
        let profile = CutProfile::new(&roots[..base_count]);
        let strands = roots
            .iter()
            .map(|&origin| {
                // Preserve the straight drop from the authored vertex under the band.
                let launch = downward_head_exit(origin, 0.026, CLEARANCE);
                let source_length = (launch.y - tip_y).max(0.12);
                let rest_length = match motion {
                    HairMotion::Rod => source_length * profile.retained_length(origin),
                    HairMotion::Rope | HairMotion::Spring => source_length,
                };
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
                    gust_gain: 1.0,
                    spread_weights: std::array::from_fn(|index| {
                        spread_weight(index as f32 / segments as f32)
                    }),
                    spread_acceleration: [Vec3::ZERO; MAX_SEGMENTS + 1],
                    chain: Chain::new(motion, points),
                }
            })
            .collect();
        let speed = if motion == HairMotion::Rod {
            ROD_SPEED
        } else {
            1.0
        };
        Self {
            strands,
            motion,
            pose,
            step_seconds: FRAME_SECONDS * speed / SUBSTEPS as f32,
            wind_baseline: Vec3::ZERO,
            gust_phase: 0.0,
            density: Default::default(),
        }
    }

    pub(crate) fn advance(&mut self, pose: Quat, tuning: RodTuning) {
        let rod_motion = self.motion == HairMotion::Rod;
        if rod_motion {
            self.update_wind(tuning.wind_target());
        }
        let spread = rod_motion && tuning.spread_width_percent != 0;
        if spread {
            self.update_spread(f32::from(tuning.spread_width_percent) / 100.0);
        }
        for step in 1..=SUBSTEPS {
            let world_to_avatar = self.pose.slerp(pose, step as f32 / SUBSTEPS as f32);
            let avatar_to_world = world_to_avatar.conjugate();
            let spawn_direction = avatar_to_world * Vec3::NEG_Y;
            let fixation = HeadbandFixation::new(fixation_percent_for(spawn_direction) / 100.0);
            for strand in &mut self.strands {
                let anchor = rotate_about_head(strand.launch, avatar_to_world);
                let effects = RodEffects {
                    tip_weight: TIP_WEIGHT,
                    fixation,
                    spawn_direction,
                    settle_damping: ROD_SETTLE_DAMPING,
                    wind_acceleration: self.wind_baseline * strand.gust_gain,
                    spread_acceleration: if spread {
                        strand.spread_acceleration
                    } else {
                        [Vec3::ZERO; MAX_SEGMENTS + 1]
                    },
                };
                strand.chain.step(
                    anchor,
                    strand.link_length,
                    self.step_seconds,
                    effects,
                    |_, point| contact(point, world_to_avatar, avatar_to_world),
                );
            }
        }
        self.pose = pose;
    }

    fn update_wind(&mut self, target: Vec3) {
        let blend = 1.0 - (-FRAME_SECONDS / WIND_RESPONSE_SECONDS).exp();
        self.wind_baseline = self.wind_baseline.lerp(target, blend);
        self.gust_phase = (self.gust_phase + TAU * GUST_HZ * FRAME_SECONDS).rem_euclid(TAU);
        let Some(heading) = self.wind_baseline.try_normalize() else {
            return;
        };
        // Sample a traveling wave in world space once per guide per frame.
        for strand in &mut self.strands {
            let point = sample_along(strand.chain.points(), 0.65);
            strand.gust_gain = gust_gain(self.gust_phase, point, heading);
        }
    }

    fn update_spread(&mut self, strength: f32) {
        // One sample per guide at each arc-length station keeps the density
        // estimate independent of how many simulated links a guide needs.
        for (index, grid) in self.density.iter_mut().enumerate() {
            grid.clear();
            let progress = (index + 1) as f32 / SPREAD_STATIONS as f32;
            for strand in &self.strands {
                let point = sample_along(strand.chain.points(), progress);
                let cell = grid.entry(density_cell(point)).or_default();
                cell.sum += point;
                cell.guides += 1;
            }
        }
        // All rods see the same positions from the start of the frame.
        for strand in &mut self.strands {
            let points = strand.chain.points();
            let mut stations = [Vec3::ZERO; SPREAD_STATIONS + 1];
            for (index, (grid, acceleration)) in
                self.density.iter().zip(&mut stations[1..]).enumerate()
            {
                let progress = (index + 1) as f32 / SPREAD_STATIONS as f32;
                *acceleration = density_pressure(grid, sample_along(points, progress)) * strength;
            }
            let segments = points.len() - 1;
            for (index, acceleration) in strand.spread_acceleration[1..=segments]
                .iter_mut()
                .enumerate()
            {
                let progress = (index + 1) as f32 / segments as f32;
                *acceleration =
                    sample_along(&stations, progress) * strand.spread_weights[index + 1];
            }
        }
    }

    pub(crate) fn for_each_curve(&self, mut emit: impl FnMut(usize, &mut [Vec3])) {
        for (index, strand) in self.strands.iter().enumerate() {
            let points = strand.chain.points();
            let mut local = [strand.origin; MAX_SEGMENTS + 2];
            let local = &mut local[..=points.len()];
            for (point, &world) in local[1..].iter_mut().zip(points) {
                *point = rotate_about_head(world, self.pose);
            }
            emit(index, local);
        }
    }

    pub(crate) fn for_each_line(&self, mut emit: impl FnMut(Vec3, Vec3, f32)) -> usize {
        let mut line_count = 0;
        self.for_each_curve(|index, points| {
            let highlight = (index % 7) as f32 / 6.0;
            for pair in points.windows(2) {
                emit(pair[0], pair[1], highlight);
            }
            line_count += points.len() - 1;
        });
        line_count
    }

    pub(crate) fn len(&self) -> usize {
        self.strands.len()
    }
}

fn gust_gain(phase: f32, point: Vec3, heading: Vec3) -> f32 {
    let local_phase = phase - point.dot(heading) * (TAU / GUST_WAVELENGTH);
    let pulse = 0.5 + 0.5 * local_phase.sin();
    1.0 + GUST_GAIN * pulse * pulse
}

fn spread_weight(progress: f32) -> f32 {
    let distance_from_peak = 1.0 - (progress / SPREAD_PEAK_PROGRESS).min(1.0);
    1.0 - (1.0 + 9.0 * distance_from_peak).log10()
}

fn smoothstep(low: f32, high: f32, value: f32) -> f32 {
    let t = ((value - low) / (high - low)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn sample_along(points: &[Vec3], progress: f32) -> Vec3 {
    let last = points.len() - 1;
    let position = progress * last as f32;
    let from = position as usize;
    points[from].lerp(points[(from + 1).min(last)], position - from as f32)
}

fn density_cell(point: Vec3) -> IVec3 {
    (point / SPREAD_RADIUS).floor().as_ivec3()
}

fn density_pressure(grid: &HashMap<IVec3, DensityCell>, point: Vec3) -> Vec3 {
    let own_cell = density_cell(point);
    let mut pressure = Vec3::ZERO;
    for z in -1..=1 {
        for y in -1..=1 {
            for x in -1..=1 {
                let key = own_cell + IVec3::new(x, y, z);
                let Some(cell) = grid.get(&key) else {
                    continue;
                };
                let mut count = cell.guides;
                let mut sum = cell.sum;
                if key == own_cell {
                    count -= 1;
                    sum -= point;
                }
                if count > 0 {
                    let offset = (point - sum / count as f32) / SPREAD_RADIUS;
                    let falloff = (1.0 - offset.length_squared()).max(0.0);
                    pressure += offset * (count as f32 * falloff * falloff);
                }
            }
        }
    }
    (pressure / SPREAD_NEIGHBOR_SCALE).clamp_length_max(1.0) * MAX_SPREAD_ACCELERATION
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
    fn fixation_eases_between_upright_and_inverted_pose_caps() {
        assert_eq!(fixation_percent_for(Vec3::NEG_Y), 50.0);
        assert_eq!(fixation_percent_for(Vec3::X), 25.0);
        assert_eq!(fixation_percent_for(Vec3::Y), 0.0);
        let mut previous = 50.0;
        for angle in 1..=180 {
            let spawn = Quat::from_rotation_z((angle as f32).to_radians()) * Vec3::NEG_Y;
            let fixation = fixation_percent_for(spawn);
            assert!((0.0..=50.0).contains(&fixation));
            assert!(fixation <= previous && previous - fixation < 0.7);
            previous = fixation;
        }
    }

    #[test]
    fn wind_headings_are_horizontal_and_converge_in_world_space_as_the_avatar_turns() {
        assert_eq!(RodTuning::default().wind_target(), Vec3::ZERO);
        assert_eq!(
            WIND_HEADINGS[RodTuning::default().wind_heading as usize].1,
            "+X/+Z"
        );
        for (index, (heading, _)) in WIND_HEADINGS.iter().enumerate() {
            let tuning = RodTuning {
                wind_strength_percent: 100,
                wind_heading: index as u8,
                ..RodTuning::default()
            };
            let wind = tuning.wind_target();
            assert_eq!(wind.y, 0.0);
            assert!(wind.dot(*heading) > MAX_WIND_ACCELERATION - 1e-5);
            assert!((wind.length() - MAX_WIND_ACCELERATION).abs() < 1e-5);
        }
        let tuning = RodTuning {
            wind_strength_percent: 100,
            ..RodTuning::default()
        };
        let roots = [crate::scene::reference_roots()[45]];
        let mut guides = RopeGuides::new(&roots, 0.74, Quat::IDENTITY, HairMotion::Rod);
        let mut minimum_gain = f32::INFINITY;
        let mut maximum_gain: f32 = 0.0;
        for frame in 0..330 {
            let pose = Quat::from_rotation_z(frame as f32 * 0.04);
            guides.advance(pose, tuning);
            let gain = guides.strands[0].gust_gain;
            minimum_gain = minimum_gain.min(gain);
            maximum_gain = maximum_gain.max(gain);
        }
        assert!(guides.wind_baseline.distance(tuning.wind_target()) < 0.01);
        assert!(minimum_gain < 1.01);
        assert!(maximum_gain > 1.65);
        for _ in 0..180 {
            guides.advance(Quat::IDENTITY, RodTuning::default());
        }
        assert!((guides.wind_baseline * guides.strands[0].gust_gain).length() < 0.01);
    }

    #[test]
    fn gust_crest_travels_along_the_heading_and_leaves_the_baseline() {
        let crest_phase = std::f32::consts::FRAC_PI_2;
        let downstream = Vec3::X * (GUST_WAVELENGTH / 2.0);
        for (point, phase, expected) in [
            (Vec3::ZERO, crest_phase, 1.0 + GUST_GAIN),
            (downstream, crest_phase, 1.0),
            (downstream, crest_phase + TAU / 2.0, 1.0 + GUST_GAIN),
        ] {
            assert!((gust_gain(phase, point, Vec3::X) - expected).abs() < 1e-6);
        }
    }

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
                guides.advance(pose, RodTuning::default());
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
    fn full_guide_budget_keeps_the_authored_cut_and_anchor_order() {
        let roots: [Vec3; 15] =
            std::array::from_fn(|index| Vec3::new(0.006 * index as f32, 1.45, -0.5));
        let authored = &roots[..10];
        let base = RopeGuides::new(authored, 0.74, Quat::IDENTITY, HairMotion::Rod);
        let mut guides = RopeGuides::with_extra_roots(
            &roots,
            authored.len(),
            0.74,
            Quat::IDENTITY,
            HairMotion::Rod,
        );
        assert_eq!(guides.len(), roots.len());
        for (strand, base) in guides.strands.iter().zip(&base.strands) {
            assert_eq!(strand.link_length, base.link_length);
            assert_eq!(strand.chain.points(), base.chain.points());
        }
        let pose = Quat::from_rotation_z(0.40);
        for _ in 0..20 {
            guides.advance(pose, RodTuning::default());
        }
        assert_eq!(guides.len(), roots.len());
        guides.for_each_curve(|index, points| {
            assert_eq!(points[0], roots[index]);
            assert!(points.iter().all(|point| point.is_finite()));
        });
    }

    #[test]
    fn cut_shortens_the_outer_edges_and_the_higher_anchor_layers() {
        let point = |x, y| Vec3::new(x, y, -0.2);
        let profile = CutProfile::new(&[point(-1.0, 0.0), point(1.0, 1.0)]);
        assert_eq!(profile.retained_length(point(0.0, 0.10)), 1.0);
        assert_eq!(profile.retained_length(point(-0.70, 0.10)), 1.0);
        assert_eq!(profile.retained_length(point(1.0, 0.10)), 0.80);
        assert!((profile.retained_length(point(0.85, 0.10)) - 0.90).abs() < 1e-6);
        for (y, retained) in [(0.55, 0.94), (0.75, 0.88), (0.95, 0.82)] {
            assert!((profile.retained_length(point(0.0, y)) - retained).abs() < 1e-6);
        }
        assert!((profile.retained_length(point(-1.0, 1.0)) - 0.62).abs() < 1e-6);
    }

    #[test]
    fn density_spread_counts_nearby_guides_and_ignores_distant_ones() {
        let root = Vec3::new(0.0, 1.45, -0.5);
        let near = root + 0.05 * Vec3::X;
        let far = root + 0.33 * Vec3::X;
        let pressure = |roots: &[Vec3]| {
            // Use the untrimmed geometry to isolate the effect of local guide count.
            let mut guides = RopeGuides::new(roots, 0.74, Quat::IDENTITY, HairMotion::Spring);
            guides.update_spread(1.0);
            let strand = &guides.strands[0];
            strand.spread_acceleration[strand.chain.points().len() - 1]
        };
        let alone = pressure(&[root, far]);
        let pair = pressure(&[root, near]);
        let crowded = pressure(&[root, near, near + 0.01 * Vec3::Z, near - 0.01 * Vec3::Z]);
        assert_eq!(alone, Vec3::ZERO);
        assert!(pair.x < -0.1);
        assert!(crowded.x < 2.0 * pair.x);
        assert_eq!(pressure(&[root, near, far]), pair);
    }

    #[test]
    fn density_spread_weights_taper_from_the_last_tenth_of_each_guide() {
        assert_eq!(spread_weight(0.0), 0.0);
        assert_eq!(spread_weight(0.9), 1.0);
        assert_eq!(spread_weight(1.0), 1.0);
        assert!(spread_weight(0.25) < 0.2);
        assert!(spread_weight(0.5) < 0.4);
        assert!(spread_weight(0.8) > 0.65);

        let root = Vec3::new(0.0, 1.45, -0.5);
        let roots = [root, root + 0.05 * Vec3::X];
        for length in [0.34, 0.68, 1.02] {
            let launch = downward_head_exit(root, 0.026, CLEARANCE);
            let mut guides =
                RopeGuides::new(&roots, launch.y - length, Quat::IDENTITY, HairMotion::Rod);
            guides.update_spread(1.0);
            let strand = &guides.strands[0];
            let tip = strand.chain.points().len() - 1;
            let force = |index: usize| strand.spread_acceleration[index].length();
            assert!(force(tip / 2) > 0.0 && force(tip / 2) < 0.4 * force(tip));
            if tip >= 10 {
                assert!((force(tip) - force(tip - 1)).abs() < 1e-5);
            } else {
                assert!(force(tip - 1) < force(tip));
            }
        }
    }

    #[test]
    fn density_spread_widens_the_avatar_guides() {
        let roots = crate::scene::reference_roots();
        let settle = |tuning| {
            let mut guides = RopeGuides::new(&roots, 0.74, Quat::IDENTITY, HairMotion::Rod);
            for _ in 0..90 {
                guides.advance(Quat::IDENTITY, tuning);
            }
            guides
        };
        let base = settle(RodTuning {
            spread_width_percent: 0,
            ..RodTuning::default()
        });
        let spread = settle(RodTuning::default());
        let tip_width = |guides: &RopeGuides| {
            let (min, max) = guides
                .strands
                .iter()
                .map(|strand| strand.chain.points().last().unwrap().x)
                .fold((f32::INFINITY, f32::NEG_INFINITY), |(min, max), x| {
                    (min.min(x), max.max(x))
                });
            max - min
        };
        assert!(tip_width(&spread) > tip_width(&base) + 0.02);
        assert_eq!(
            base.for_each_line(|_, _, _| {}),
            spread.for_each_line(|_, _, _| {})
        );
    }
}
