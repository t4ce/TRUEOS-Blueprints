use std::f32::consts::TAU;

use glam::{Mat4, Vec2, Vec3};

// Snapshot of bfly::geometry::LAYERED_VERTEX_BYTES, with each position
// stored as little-endian [x, y, path_tag]. F5 uses the fill patches first.
const CONTROL_BYTES: &[u8; 960] = include_bytes!("../../assets/bfly-layered-controls.bin");
const PATCH_BYTES: usize = 4 * 3 * size_of::<f32>();
const FILL_BYTES: usize = CONTROL_BYTES.len() / 2;
const MAX_PATH_CURVES: usize = 5;
const MAX_FILL_STEPS: usize = 64;
const MAX_OUTLINE_STEPS: usize = 128;
pub(super) const MAX_TRIANGLES: usize =
    2 * (MAX_PATH_CURVES * MAX_FILL_STEPS + (MAX_PATH_CURVES + 1) * 2 * MAX_OUTLINE_STEPS + 2);

const BIRTH_TIER: usize = 1;
const LAST_TIER: usize = 7;
pub(super) const BUTTERFLIES_PER_SKETCH: usize = 3;
const FOCAL_LENGTH: f32 = 8_000.0;
const TIER_MOVE_NS: u64 = 4_400_000_000;
const DEPARTURE_LEAD_NS: u64 = 2_000_000_000;
const BIRTH_REST_NS: u64 = 1_500_000_000;
const CLEARANCE_NS: u64 = 440_000_000;
const TAKEOFF_NS: u64 = 1_200_000_000;
pub(super) const GROUP_TAKEOFF_END_NS: u64 =
    BUTTERFLIES_PER_SKETCH as u64 * BIRTH_REST_NS + TAKEOFF_NS;
const ORBIT_SECONDS: f32 = 2.5;
const ORBIT_RAMP: f32 = 0.6;
const FLAP_HZ: f32 = 0.80;
const ORBIT_FLAP_HZ: f32 = 0.32;
const NANOS_TO_SECONDS: f32 = 1e-9;
const EXIT_Y: f32 = -16.0;
const INSET: f32 = 0.894_427_2; // 80% of the area.
const HINGE_Y: [f32; 2] = [0.011_652_005, -0.004_455_092_4];
pub(super) const WING_BOUNDS: [[f32; 3]; 2] = [
    [-0.332_086_77, 0.662_223_34, 0.331_109_94],
    [-0.398_931_6, 0.652_023_5, 0.285_934_5],
];
// Once airborne, layer the wings behind the avatar and keep their outline in front.
const FILL_DEPTH: f32 = 0.91;
const OUTLINE_DEPTH: f32 = 0.90;
const BIRTH_DEPTH_BIAS: f32 = 0.000_5;

#[derive(Clone, Copy)]
pub(super) struct Curve {
    pub points: [Vec2; 4],
    pub straight: bool,
}

impl Curve {
    fn read(bytes: &[u8; PATCH_BYTES]) -> Self {
        let float = |offset| f32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap());
        Self {
            points: std::array::from_fn(|i| Vec2::new(float(i * 12), float(i * 12 + 4))),
            straight: float(8) as u32 & 1 != 0,
        }
    }

    pub(super) fn sample(&self, t: f32) -> Vec2 {
        let [a, b, c, d] = self.points;
        if self.straight {
            return a.lerp(d, t);
        }
        let [p, q, r] = [a.lerp(b, t), b.lerp(c, t), c.lerp(d, t)];
        p.lerp(q, t).lerp(q.lerp(r, t), t)
    }
}

pub(super) fn artwork_geometry(artwork: usize) -> (Vec<Curve>, Curve, Curve) {
    let read = |offset, patch| {
        let start = offset + patch * PATCH_BYTES;
        Curve::read(
            CONTROL_BYTES[start..start + PATCH_BYTES]
                .try_into()
                .unwrap(),
        )
    };
    // The fill controls describe the perimeter exactly once. The original
    // outline controls contribute the branch inside each half-wing.
    let (outer, inset, hinge) = if artwork == 0 {
        (0..3, 1, 3)
    } else {
        (4..9, 7, 9)
    };
    (
        outer.map(|patch| read(0, patch)).collect(),
        read(FILL_BYTES, inset),
        read(0, hinge),
    )
}

pub(super) struct LiveButterfly {
    artwork: usize,
    fill: [u8; 4],
    fill_curves: Vec<Curve>,
    inset: Curve,
    hinge: Curve,
    started_ns: u64,
    release_ns: u64,
    birth: Birth,
    takeoff_pixels_per_unit: Option<f32>,
    flight_focal_length: f32,
    lane: usize,
    flight_seed: u32,
    speed_percent: u32,
    duration_ns: u64,
}

#[derive(Clone, Copy)]
pub(super) struct BirthAnchor {
    pub draw_to_world: Mat4,
    pub center: Vec3,
    pub width: Vec3,
}

#[derive(Clone, Copy)]
pub(super) struct Birth {
    pub center: Vec2,
    pub width_pixels: f32,
    pub depth: f32,
    pub draw_to_clip: Mat4,
    pub stroke_width: f32,
    pub anchor: BirthAnchor,
}

pub(super) struct FlightLight {
    pub center: Vec2,
    pub recession: f32,
    pub phase: f32,
    pub colors: [[u8; 3]; 2],
}

struct FlightSample {
    center: Vec2,
    depth: f32,
    bank: Vec2,
    flaps: [f32; 2],
    takeoff: f32,
    clearance: f32,
    clip_depths: [f32; 2],
    fill_steps: usize,
    outline_steps: usize,
    outline_width: f32,
    recession: f32,
    phase: f32,
}

impl LiveButterfly {
    pub(super) fn new(seed: u32, started_ns: u64, birth: Birth, lane: usize) -> Self {
        let artwork = artwork(seed);
        let (fill_curves, inset, hinge) = artwork_geometry(artwork);
        let seed = seed & 127;
        let flight_seed = (seed + 47 * lane as u32) & 127;
        let speed_percent = 85 + (flight_seed * 17 + 13) % 31;
        let release_ns = (lane as u64 + 1) * BIRTH_REST_NS;
        let travel_ns = DEPARTURE_LEAD_NS
            + (LAST_TIER - lane) as u64 * TIER_MOVE_NS * 100 / u64::from(speed_percent);
        // Travel through the background at two thirds of the old speed.
        let duration_ns = release_ns + TAKEOFF_NS + travel_ns * 3 / 2;
        Self {
            artwork,
            fill: fill_color(seed, artwork),
            fill_curves,
            inset,
            hinge,
            started_ns,
            release_ns,
            birth,
            takeoff_pixels_per_unit: None,
            flight_focal_length: FOCAL_LENGTH,
            lane,
            flight_seed,
            speed_percent,
            duration_ns,
        }
    }

    pub(super) fn fill(&self) -> [u8; 4] {
        self.fill
    }

    pub(super) fn outline(&self) -> [u8; 4] {
        let [r, g, b, _] = self.fill;
        [r, g, b, 255]
    }

    pub(super) fn is_lead(&self) -> bool {
        self.lane == 0
    }

    pub(super) fn started_ns(&self) -> u64 {
        self.started_ns
    }

    pub(super) fn has_departed(&self, now_ns: u64) -> bool {
        now_ns.saturating_sub(self.started_ns) >= self.duration_ns
    }

    pub(super) fn update_projection(
        &mut self,
        view_projection: Mat4,
        extent: (u32, u32),
        now_ns: u64,
    ) {
        if extent.0.min(extent.1) == 0 {
            return;
        }
        let half_width = 0.5 * extent.0 as f32;
        // The length of the orthographic view-projection's first row gives
        // the zoom independently of the observer's yaw.
        let pixels_per_unit = view_projection.row(0).truncate().length() * half_width;
        if let Some(takeoff_scale) = self.takeoff_pixels_per_unit {
            // Keep the takeoff perspective and scale its projected wing offsets.
            self.flight_focal_length = FOCAL_LENGTH * (pixels_per_unit / takeoff_scale);
            return;
        }
        let anchor = self.birth.anchor;
        let center = view_projection.project_point3(anchor.center);
        let width = view_projection.transform_vector3(anchor.width).x.abs();
        self.birth.draw_to_clip = view_projection * anchor.draw_to_world;
        self.birth.center = center.truncate() * 0.5 + Vec2::splat(0.5);
        self.birth.depth = center.z;
        self.birth.width_pixels = width * half_width;
        if now_ns.saturating_sub(self.started_ns) >= self.release_ns + TAKEOFF_NS {
            self.takeoff_pixels_per_unit = Some(pixels_per_unit);
        }
    }

    pub(super) fn light(&self, extent: (u32, u32), now_ns: u64) -> Option<FlightLight> {
        let size = Vec2::new(extent.0 as f32, extent.1 as f32);
        let flight = self.visible_flight(size, now_ns)?;
        Some(FlightLight {
            center: flight.center,
            recession: flight.recession,
            phase: flight.phase,
            colors: light_colors_for_fill(self.fill),
        })
    }

    fn flight(&self, size: Vec2, now_ns: u64) -> FlightSample {
        let release = self.release_ns;
        let travel_ns = self.duration_ns - release - TAKEOFF_NS;
        let age_ns = now_ns
            .saturating_sub(self.started_ns)
            .saturating_sub(release)
            .min(TAKEOFF_NS + travel_ns);
        let age = age_ns as f32 * NANOS_TO_SECONDS;
        let takeoff = smoothstep(age_ns as f32 / TAKEOFF_NS as f32);
        let clearance = smoothstep(age_ns as f32 / CLEARANCE_NS as f32);
        let sideways = smoothstep(
            age_ns.saturating_sub(CLEARANCE_NS) as f32 / (TAKEOFF_NS - CLEARANCE_NS) as f32,
        );
        let recede_ns = age_ns.saturating_sub(TAKEOFF_NS);
        let progress = smoothstep(recede_ns as f32 / travel_ns as f32);
        let settle = smoothstep(recede_ns as f32 * NANOS_TO_SECONDS);
        let launch_tier = self.lane;
        let level = BIRTH_TIER as f32
            + (launch_tier as f32 - BIRTH_TIER as f32) * takeoff
            + progress * (LAST_TIER - launch_tier) as f32;
        let start = self.birth.center * size;
        let min_size = size.min_element();
        let birth_width = self.birth.width_pixels.max(1.0);
        let (aim_x, up, bend) = match self.lane {
            0 => (start.x * 0.5, 0.04, Vec2::new(-0.025, -0.04)),
            1 => (start.x * 0.72, -0.07, Vec2::new(-0.12, 0.035)),
            _ => (
                start.x + (size.x - start.x) * 0.44,
                0.09,
                Vec2::new(0.02, 0.09),
            ),
        };
        let [lo, hi] = panel(size, launch_tier);
        let inset = (hi - lo) * 0.15;
        let aim = Vec2::new(aim_x, start.y + up * min_size).clamp(lo + inset, hi - inset);
        // Clear the avatar by one projected wing before turning into the old lanes.
        let clear = start + Vec2::Y * (0.5 * birth_width * clearance);
        let launch =
            clear.lerp(aim, sideways) + 4.0 * sideways * (1.0 - sideways) * min_size * bend;
        let (orbit, tangent) = flight_path(self.flight_seed, self.speed_percent, age);
        let variation = self.flight_seed.wrapping_mul(0x9e37_79b9);
        let spread = 0.13 + (variation >> 24) as f32 * (0.06 / 255.0);
        let tier = level as usize;
        let extent = |tier| {
            let [lo, hi] = panel(size, tier);
            (hi - lo).min_element()
        };
        let radius_from = extent(tier);
        let radius_to = extent((tier + 1).min(LAST_TIER));
        let radius = spread * (radius_from + (radius_to - radius_from) * level.fract());
        let (loop_offset, orbit_weight, lost_beats) = orbital_flight(variation, age);
        let drift = (1.0 - progress) * settle;
        let descent = progress + 0.06 * progress * drift * orbit.y;
        let center = Vec2::new(
            launch.x + orbit.x * radius * drift,
            launch.y + (EXIT_Y - launch.y) * descent,
        ) + loop_offset * (min_size * 0.065 * drift);
        let bank = -18.0_f32.to_radians() * tangent.normalize_or_zero().x * settle;
        let [bottom, half_span, top] = WING_BOUNDS[self.artwork];
        // Keep the projection denominator positive even when a wing folds
        // toward the camera at unusually large viewport sizes.
        let birth_depth = 2.0 * half_span * (FOCAL_LENGTH / birth_width).max(2.0);
        let depth = birth_depth * (level - BIRTH_TIER as f32).exp2();
        let flight_span = (2.0 * half_span).max(top - bottom) * (self.flight_focal_length / depth);
        let birth_span = birth_width.max((top - bottom) * birth_width / (2.0 * half_span));
        let span = birth_span + (flight_span - birth_span) * takeoff;
        let (fill_steps, outline_steps, outline_width) = detail(span);
        let (relaxed, departure) = flap_phases(self.flight_seed, age);
        let departure = departure - lost_beats;
        let closed = (78.0 + 6.0 * (TAU * departure).sin()).to_radians();
        let flaps = flap_angles(self.flight_seed, relaxed, departure, settle).map(|flap| {
            let launch_flap = flap * takeoff;
            launch_flap + (closed - launch_flap) * orbit_weight
        });
        let approach = self.birth.depth * 0.78;
        let clip_depths = [
            (approach + BIRTH_DEPTH_BIAS, FILL_DEPTH),
            (approach - BIRTH_DEPTH_BIAS, OUTLINE_DEPTH),
        ]
        .map(|(front, distant)| {
            let front = front.clamp(0.0, 1.0);
            front + (distant - front) * progress
        });
        FlightSample {
            center,
            depth,
            bank: Vec2::from_angle(bank),
            flaps,
            takeoff,
            clearance,
            clip_depths,
            fill_steps,
            outline_steps,
            outline_width,
            recession: progress,
            phase: (relaxed + (departure - relaxed) * settle).fract(),
        }
    }

    fn visible_flight(&self, size: Vec2, now_ns: u64) -> Option<FlightSample> {
        if size.min_element() == 0.0 || self.has_departed(now_ns) {
            return None;
        }
        // Reveal each sibling at the drawing as the previous one takes off.
        // Each gets the same full rest at the original size before its turn.
        if now_ns
            < self
                .started_ns
                .saturating_add(self.release_ns - BIRTH_REST_NS)
        {
            return None;
        }
        let flight = self.flight(size, now_ns);
        let [bottom, half_span, top] = WING_BOUNDS[self.artwork];
        let up = (top - bottom) * 0.5 * flight.bank.x + half_span * flight.bank.y.abs();
        let top = flight.center.y
            + up * self.flight_focal_length / (flight.depth - half_span)
            + flight.outline_width;
        (top >= 0.0).then_some(flight)
    }

    pub(super) fn triangles(
        &self,
        extent: (u32, u32),
        now_ns: u64,
        mut emit: impl FnMut([Vec3; 3], bool),
    ) {
        let size = Vec2::new(extent.0 as f32, extent.1 as f32);
        let Some(FlightSample {
            center,
            depth,
            bank,
            flaps,
            takeoff,
            clearance,
            clip_depths,
            fill_steps,
            outline_steps,
            outline_width: width,
            ..
        }) = self.visible_flight(size, now_ns)
        else {
            return;
        };
        let inset_origin = Vec2::new(0.0, HINGE_Y[self.artwork]);
        let [bottom, _, top] = WING_BOUNDS[self.artwork];
        let midpoint = (bottom + top) * 0.5;
        let mut triangle = |pixel: [Vec3; 3], outline: bool| {
            let mut clip = pixel.map(|p| (2.0 * p.truncate() / size - 1.0).extend(p.z));
            let cross = (clip[1] - clip[0])
                .truncate()
                .perp_dot((clip[2] - clip[0]).truncate());
            if cross.abs() <= 1.0e-8 {
                return;
            }
            if cross < 0.0 {
                clip.swap(1, 2);
            }
            emit(clip, outline);
        };
        for (side, flap) in [1.0, -1.0].into_iter().zip(flaps) {
            let (sin, cos) = flap.sin_cos();
            let project = |point: Vec2, outline: bool| {
                let wing = Vec2::new(side * cos * point.x, point.y - midpoint);
                let offset = bank.rotate(wing) * self.flight_focal_length / (depth - point.x * sin);
                let layer_depth = clip_depths[usize::from(outline)];
                if takeoff == 1.0 {
                    return (center + offset).extend(layer_depth);
                }
                let source = self.birth.draw_to_clip.transform_point3(Vec3::new(
                    side * point.x,
                    point.y,
                    0.0,
                ));
                let birth_offset = ((source.truncate() + 1.0) * 0.5 - self.birth.center) * size;
                let bias = if outline {
                    -BIRTH_DEPTH_BIAS
                } else {
                    BIRTH_DEPTH_BIAS
                };
                let birth_depth = (source.z + bias).clamp(0.0, 1.0);
                let clip_depth = birth_depth + (layer_depth - birth_depth) * clearance;
                (center + birth_offset.lerp(offset, takeoff)).extend(clip_depth)
            };
            let origin = project(inset_origin, false);
            // The fill fan already has the shared hinge as its last edge.
            for curve in &self.fill_curves {
                let mut last = project(inset_origin.lerp(curve.points[0], INSET), false);
                for step in 1..=fill_steps {
                    let point = curve.sample(step as f32 / fill_steps as f32);
                    let next = project(inset_origin.lerp(point, INSET), false);
                    triangle([origin, last, next], false);
                    last = next;
                }
            }
            let hinge = (side > 0.0).then_some(&self.hinge);
            for curve in self.fill_curves.iter().chain([&self.inset]).chain(hinge) {
                let mut previous = curve.points[0];
                let mut last = project(previous, true);
                let steps = if curve.straight { 1 } else { outline_steps };
                for step in 1..=steps {
                    let point = curve.sample(step as f32 / steps as f32);
                    let next = project(point, true);
                    if let Some(axis) = (next - last).truncate().try_normalize() {
                        // Add square ends as the ribbon eases into its flight projection.
                        let end = (axis * (takeoff * width * 0.5)).extend(0.0);
                        let live_offset = (axis.perp() * (width * 0.5)).extend(0.0);
                        let side_offset = if takeoff == 1.0 {
                            live_offset
                        } else {
                            let delta = point - previous;
                            let direction = Vec2::new(side * delta.x, delta.y);
                            let normal = direction.normalize_or_zero().perp();
                            let local = normal * (self.birth.stroke_width * 0.5);
                            let clip = self.birth.draw_to_clip.transform_vector3(local.extend(0.0));
                            let mut drawn_offset =
                                (clip.truncate() * 0.5 * size).extend(clip.z * (1.0 - clearance));
                            if drawn_offset.truncate().dot(live_offset.truncate()) < 0.0 {
                                drawn_offset = -drawn_offset;
                            }
                            drawn_offset.lerp(live_offset, takeoff)
                        };
                        let [a, b, c, d] = [
                            last - end + side_offset,
                            last - end - side_offset,
                            next + end + side_offset,
                            next + end - side_offset,
                        ];
                        triangle([a, b, c], true);
                        triangle([c, b, d], true);
                    }
                    previous = point;
                    last = next;
                }
            }
        }
    }
}

fn smoothstep(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn orbital_flight(variation: u32, age: f32) -> (Vec2, f32, f32) {
    let interval = 12.5 + (variation >> 24) as f32 * (5.0 / 255.0);
    let elapsed = age - interval;
    if elapsed < 0.0 {
        return (Vec2::ZERO, 0.0, 0.0);
    }
    let lap = (elapsed / interval).floor();
    let time = elapsed % interval;
    // Integrating the slowdown keeps the wing phase continuous through the loop.
    let ramp_integral = |time: f32| {
        let t = (time / ORBIT_RAMP).clamp(0.0, 1.0);
        ORBIT_RAMP * t.powi(3) * (1.0 - 0.5 * t) + (time - ORBIT_RAMP).max(0.0)
    };
    let slow_span = ORBIT_SECONDS - ORBIT_RAMP;
    let integrated = ramp_integral(time) - ramp_integral(time - slow_span);
    let lost_beats = (FLAP_HZ - ORBIT_FLAP_HZ) * (lap * slow_span + integrated);
    if time >= ORBIT_SECONDS {
        return (Vec2::ZERO, 0.0, lost_beats);
    }
    let (sin, cos) = (TAU * smoothstep(time / ORBIT_SECONDS)).sin_cos();
    let direction = if variation & 1 == 0 { -1.0 } else { 1.0 };
    let offset = Vec2::new(direction * (1.0 - cos), 0.6 * sin);
    let weight = smoothstep(time / ORBIT_RAMP) * smoothstep((ORBIT_SECONDS - time) / ORBIT_RAMP);
    (offset, weight, lost_beats)
}

// Bfly's closed Catmull–Rom path. Each seed picks its lap rate, knot count
// and phase without changing the wing artwork or the backdrop.
fn flight_path(seed: u32, speed_percent: u32, age: f32) -> (Vec2, Vec2) {
    let index = seed & 7;
    let count = 3 + index as i32 % 5;
    let offset = index as f32 / 8.0 + (seed >> 3) as f32 * 0.618_034;
    let progress = (age * speed_percent as f32 / 1_200.0 + offset).fract() * count as f32;
    let segment = progress as i32;
    let t = progress.fract();
    let knot = |knot: i32| {
        let angle = knot.rem_euclid(count) as f32 * (TAU / count as f32) + 0.6 * index as f32;
        let (sin, cos) = angle.sin_cos();
        Vec2::new(cos, 0.82 * sin)
    };
    let [a, b, c, d] = [-1, 0, 1, 2].map(|offset| knot(segment + offset));
    let start = 0.5 * (c - a);
    let end = 0.5 * (d - b);
    let span = c - b;
    let quadratic = 3.0 * span - 2.0 * start - end;
    let cubic = start + end - 2.0 * span;
    let position = b + t * (start + t * (quadratic + t * cubic));
    let tangent = start + t * (2.0 * quadratic + 3.0 * t * cubic);
    (position, tangent)
}

fn flap_phases(seed: u32, age: f32) -> (f32, f32) {
    let offset = (seed & 7) as f32 / 8.0;
    (age * 0.25 + offset, age * FLAP_HZ + offset)
}

fn flap_angles(seed: u32, phase: f32, beat: f32, blend: f32) -> [f32; 2] {
    let relaxed = [0.0, 0.135].map(|wing| 32.0_f32.to_radians() * ((phase + wing) * TAU).sin());
    let index = seed & 7;
    let sweep_step = (beat as u32 * 5 + index * 3) & 15;
    let sweep = 75.0 + 5.0 * sweep_step as f32;
    let turnaround = 0.5 - 0.5 * (TAU * beat.fract()).cos();
    let departure = (87.75 - sweep * turnaround).to_radians();
    relaxed.map(|angle| angle + (departure - angle) * blend)
}

pub(super) fn light_colors(seed: u32) -> [[u8; 3]; 2] {
    light_colors_for_fill(fill_color(seed & 127, artwork(seed)))
}

pub(super) fn artwork(seed: u32) -> usize {
    (seed & 1) as usize
}

pub(super) fn outline_color(seed: u32) -> [u8; 4] {
    let [r, g, b, _] = fill_color(seed & 127, artwork(seed));
    [r, g, b, 255]
}

fn light_colors_for_fill([r, g, b, _]: [u8; 4]) -> [[u8; 3]; 2] {
    let fill = [r, g, b];
    [fill, fill.map(|channel| channel.midpoint(255))]
}

fn fill_color(seed: u32, artwork: usize) -> [u8; 4] {
    let shade = (((seed >> 1) & 63) * 37 + artwork as u32 * 17 + 13) & 63;
    let roll = (shade as f32 + 0.5) / 64.0;
    let toward_red = roll * (0.45 + 0.55 * roll);
    let hue = if artwork == 0 {
        let angle = 58.0 - 51.0 * toward_red;
        [1.0, angle / 60.0, 0.0]
    } else {
        let angle = 288.0 + 64.0 * toward_red;
        [
            ((angle - 240.0) / 60.0).min(1.0),
            0.0,
            ((360.0 - angle) / 60.0).min(1.0),
        ]
    };
    let [r, g, b] = hue.map(|component| (38.0 + 198.0 * component).round() as u8);
    [r, g, b, 217]
}

fn panel(size: Vec2, tier: usize) -> [Vec2; 2] {
    let mut rest = [Vec2::ZERO, size];
    let mut panel = rest;
    // Bfly takes the singleton half at each split and recurses into the rest.
    for i in 0..=tier {
        panel = rest;
        if i == LAST_TIER {
            break;
        }
        let span = rest[1] - rest[0];
        if span.x >= span.y {
            panel[1].x -= span.x * 0.5;
            rest[0].x = panel[1].x;
        } else {
            panel[0].y += span.y * 0.5;
            rest[1].y = panel[0].y;
        }
    }
    panel
}

fn detail(span: f32) -> (usize, usize, f32) {
    let level = (span.clamp(4.0, 1024.0) as usize).ilog2();
    let fill = 1 << level.min(MAX_FILL_STEPS.ilog2());
    let outline = if span < 16.0 {
        4
    } else {
        (1 << (level - 1)).min(MAX_OUTLINE_STEPS)
    };
    let width = 2.0 + level.saturating_sub(8) as f32;
    (fill, outline, width)
}

#[cfg(test)]
mod tests {
    use glam::Vec4;

    use super::*;

    const TEST_DRAW: Mat4 = Mat4::from_cols(
        Vec4::new(0.51, 0.03, 0.06, 0.0),
        Vec4::new(0.02, 0.55, 0.025, 0.0),
        Vec4::Z,
        Vec4::new(-0.34, 0.31, 0.23, 1.0),
    );
    const TEST_BIRTH: Birth = Birth {
        center: Vec2::new(0.33, 0.70),
        width_pixels: 0.48 * 720.0,
        depth: 0.23,
        draw_to_clip: TEST_DRAW,
        stroke_width: 0.009,
        anchor: BirthAnchor {
            draw_to_world: TEST_DRAW,
            center: Vec3::new(-0.34, 0.40, 0.23),
            width: Vec3::new(2.0 * 0.48 * 720.0 / 1024.0, 0.0, 0.0),
        },
    };

    fn projection((width, height): (u32, u32), pixels_per_unit: f32) -> Mat4 {
        Mat4::from_scale(Vec3::new(
            2.0 * pixels_per_unit / width as f32,
            2.0 * pixels_per_unit / height as f32,
            1.0,
        ))
    }

    #[test]
    fn styles_follow_the_seed_and_preserve_the_two_f5_paths() {
        let original = LiveButterfly::new(0, 0, TEST_BIRTH, 0);
        let alternate = LiveButterfly::new(1, 0, TEST_BIRTH, 0);
        assert_eq!(original.fill_curves.len(), 3);
        assert_eq!(alternate.fill_curves.len(), 5);
        for butterfly in [&original, &alternate] {
            assert!(butterfly.hinge.straight);
            assert!(!butterfly.inset.straight);
            for curve in butterfly
                .fill_curves
                .iter()
                .chain([&butterfly.inset, &butterfly.hinge])
            {
                assert!(curve.points.iter().all(|point| point.is_finite()));
            }
        }
        assert_eq!(original.fill(), [236, 209, 38, 217]);
        assert_eq!(alternate.fill(), [236, 38, 204, 217]);
        assert_eq!(original.outline(), outline_color(0));
        assert_eq!(alternate.outline(), outline_color(1));
    }

    #[test]
    fn flight_light_shares_colors_position_and_departure_with_the_wings() {
        let extent = (1024, 720);
        let size = Vec2::new(extent.0 as f32, extent.1 as f32);
        for seed in [0, 1, 132, 353] {
            let butterfly = LiveButterfly::new(seed, 2_000_000_000, TEST_BIRTH, 0);
            let start = butterfly.started_ns();
            let first = butterfly.light(extent, start).unwrap();
            assert_eq!(first.center, butterfly.flight(size, start).center);
            assert_eq!(first.recession, 0.0);
            assert_eq!(first.phase, (seed & 7) as f32 / 8.0);
            assert_eq!(first.colors, light_colors(seed));
            assert_eq!(first.colors[0], butterfly.fill()[..3]);
            assert!(
                first.colors[1]
                    .iter()
                    .zip(first.colors[0])
                    .all(|(&bright, base)| bright > base)
            );

            let half = butterfly
                .light(extent, start + butterfly.duration_ns / 2)
                .unwrap();
            assert!(half.center.y < first.center.y);
            assert!(half.recession > first.recession);
            assert!((0.0..1.0).contains(&half.phase));

            let near_exit = start + butterfly.duration_ns - 1;
            let mut emitted = false;
            butterfly.triangles(extent, near_exit, |_, _| emitted = true);
            assert_eq!(butterfly.light(extent, near_exit).is_some(), emitted);
            assert!(
                butterfly
                    .light(extent, start + butterfly.duration_ns)
                    .is_none()
            );
            assert!(butterfly.light((0, extent.1), start).is_none());
        }
    }

    #[test]
    fn second_tier_splits_the_remainder_on_its_longer_axis() {
        assert_eq!(
            panel(Vec2::new(1920.0, 1080.0), BIRTH_TIER),
            [Vec2::new(960.0, 540.0), Vec2::new(1920.0, 1080.0)]
        );
        assert_eq!(
            panel(Vec2::new(800.0, 1200.0), BIRTH_TIER),
            [Vec2::ZERO, Vec2::new(400.0, 600.0)]
        );
        assert_eq!(detail(5.0), (4, 4, 2.0));
        assert_eq!(detail(16.0), (16, 8, 2.0));
        assert_eq!(detail(512.0), (64, 128, 3.0));
        assert_eq!(detail(1024.0), (64, 128, 4.0));
    }

    #[test]
    fn birth_stays_at_the_drawn_center_size_and_depth_until_takeoff() {
        let extent = (1024, 720);
        let size = Vec2::new(extent.0 as f32, extent.1 as f32);
        for seed in [0, 3] {
            let butterfly = LiveButterfly::new(seed, 2_000_000_000, TEST_BIRTH, 0);
            let start = butterfly.started_ns();
            let spawn = butterfly.flight(size, start);
            assert_eq!(spawn.center, TEST_BIRTH.center * size);
            assert_eq!(spawn.bank, Vec2::X);
            assert_eq!(spawn.flaps, [0.0; 2]);
            let point = butterfly.hinge.points[0];
            let clip_to_draw = TEST_BIRTH.draw_to_clip.inverse();
            let mut saw_projected_hinge = false;
            butterfly.triangles(extent, start, |triangle, outline| {
                if outline {
                    saw_projected_hinge |= triangle.into_iter().any(|vertex| {
                        let local =
                            clip_to_draw.transform_point3(vertex + Vec3::Z * BIRTH_DEPTH_BIAS);
                        local.truncate().distance(point) <= TEST_BIRTH.stroke_width * 0.51
                            && local.z.abs() < 1e-6
                    });
                }
            });
            assert!(saw_projected_hinge);
            for offset in [BIRTH_REST_NS / 2, BIRTH_REST_NS] {
                let still = butterfly.flight(size, start + offset);
                assert_eq!(still.center, spawn.center);
                assert_eq!(still.depth, spawn.depth);
                assert_eq!(still.clip_depths, spawn.clip_depths);
                assert_eq!(still.phase, spawn.phase);
            }
            let takeoff = butterfly.flight(size, start + BIRTH_REST_NS + 1_000_000);
            assert!((takeoff.center - spawn.center).length() < 0.01);
            assert!((takeoff.clip_depths[0] - spawn.clip_depths[0]).abs() < 0.0001);
            let flying = butterfly.flight(size, start + BIRTH_REST_NS + TAKEOFF_NS);
            assert!((flying.center.x - spawn.center.x * 0.5).abs() < 1e-3);
            assert_eq!(flying.depth, spawn.depth * 0.5);
            assert!(flying.clip_depths[0] < TEST_BIRTH.depth);
            assert_eq!(flying.recession, 0.0);
        }
    }

    #[test]
    fn each_butterfly_clears_one_wing_before_turning_sideways() {
        let size = Vec2::new(1024.0, 720.0);
        let start = TEST_BIRTH.center * size;
        let wing = 0.5 * TEST_BIRTH.width_pixels;
        for lane in 0..BUTTERFLIES_PER_SKETCH {
            let butterfly = LiveButterfly::new(1, 0, TEST_BIRTH, lane);
            let release = butterfly.release_ns;
            let midpoint = butterfly.flight(size, release + CLEARANCE_NS / 2);
            assert_eq!(midpoint.center.x, start.x);
            assert!((midpoint.center.y - (start.y + wing * 0.5)).abs() < 1e-4);
            assert_eq!(midpoint.clearance, 0.5);
            let turn = release + CLEARANCE_NS;
            let apex = butterfly.flight(size, turn);
            assert!(apex.center.abs_diff_eq(start + Vec2::Y * wing, 1e-4));
            assert_eq!(apex.clearance, 1.0);
            assert_eq!(apex.recession, 0.0);
            for near in [turn - 1_000_000, turn + 1_000_000] {
                assert!(butterfly.flight(size, near).center.distance(apex.center) < 0.01);
            }
            let merge = release + TAKEOFF_NS;
            let before = butterfly.flight(size, merge - 1_000_000);
            let after = butterfly.flight(size, merge + 1_000_000);
            assert!(before.center.distance(after.center) < 0.01);
        }
    }

    #[test]
    fn birth_tracks_observer_until_takeoff_finishes() {
        let extent = (1024, 720);
        let mut butterfly = LiveButterfly::new(0, 0, TEST_BIRTH, 0);
        let pullback = Mat4::from_translation(Vec3::new(0.0, -0.2, 0.1))
            * Mat4::from_scale(Vec3::new(0.8, 0.8, 1.0));
        let center = TEST_BIRTH.center * 0.8 + Vec2::X * 0.1;
        butterfly.update_projection(pullback, extent, BIRTH_REST_NS / 2);
        assert!(butterfly.birth.center.abs_diff_eq(center, 1e-6));
        assert!((butterfly.birth.width_pixels - 0.8 * TEST_BIRTH.width_pixels).abs() < 1e-4);
        assert!((butterfly.birth.depth - (TEST_BIRTH.depth + 0.1)).abs() < 1e-6);
        assert_eq!(
            butterfly.birth.draw_to_clip,
            pullback * TEST_BIRTH.draw_to_clip
        );
        butterfly.update_projection(pullback, extent, BIRTH_REST_NS + TAKEOFF_NS);
        butterfly.update_projection(Mat4::IDENTITY, extent, BIRTH_REST_NS + 2 * TAKEOFF_NS);
        assert!(butterfly.birth.center.abs_diff_eq(center, 1e-6));
        assert!((butterfly.birth.depth - (TEST_BIRTH.depth + 0.1)).abs() < 1e-6);
        assert_eq!(
            butterfly.birth.draw_to_clip,
            pullback * TEST_BIRTH.draw_to_clip
        );
    }

    #[test]
    fn airborne_wings_follow_camera_zoom_across_backing_resizes() {
        let original = (1024, 720);
        let pixels_per_unit = 320.0;
        let fill_size = |butterfly: &LiveButterfly, extent: (u32, u32), now_ns| {
            let size = Vec2::new(extent.0 as f32, extent.1 as f32);
            let mut low = Vec2::splat(f32::INFINITY);
            let mut high = Vec2::splat(f32::NEG_INFINITY);
            butterfly.triangles(extent, now_ns, |triangle, outline| {
                if !outline {
                    for point in triangle {
                        let pixel = 0.5 * (point.truncate() + 1.0) * size;
                        low = low.min(pixel);
                        high = high.max(pixel);
                    }
                }
            });
            let span = high - low;
            assert!(span.is_finite() && span.min_element() > 0.0);
            span
        };
        for lane in 0..BUTTERFLIES_PER_SKETCH {
            let mut butterfly = LiveButterfly::new(lane as u32, 0, TEST_BIRTH, lane);
            let takeoff = butterfly.release_ns + TAKEOFF_NS;
            let now = takeoff + 1_800_000_000;
            butterfly.update_projection(projection(original, pixels_per_unit), original, takeoff);
            let initial = fill_size(&butterfly, original, now);
            for (extent, zoom, yaw) in [
                ((1280, 900), 1.0, 0.0),
                ((1280, 900), 1.1, 0.12),
                ((1280, 900), 1.25, 0.0),
                ((650, 1050), 1.25, 0.0),
                ((650, 1050), 0.8, -0.12),
                ((650, 1050), 0.65, 0.0),
            ] {
                let view_projection =
                    projection(extent, pixels_per_unit * zoom) * Mat4::from_rotation_y(yaw);
                butterfly.update_projection(view_projection, extent, now);
                let actual = fill_size(&butterfly, extent, now);
                let expected = initial * zoom;
                assert!(
                    actual.abs_diff_eq(expected, 1.0),
                    "lane {lane} at {extent:?} and zoom {zoom}: {actual:?}, expected {expected:?}"
                );
            }
        }
    }

    #[test]
    fn background_travel_keeps_the_slower_per_tier_speed_after_growth() {
        for seed in 0..128 {
            for lane in 0..BUTTERFLIES_PER_SKETCH {
                let butterfly = LiveButterfly::new(seed, 0, TEST_BIRTH, lane);
                let full_speed_travel = DEPARTURE_LEAD_NS
                    + (LAST_TIER - lane) as u64 * TIER_MOVE_NS * 100
                        / u64::from(butterfly.speed_percent);
                assert_eq!(
                    butterfly.duration_ns - butterfly.release_ns - TAKEOFF_NS,
                    full_speed_travel * 3 / 2
                );
                assert!(butterfly.duration_ns < 9 * 7_000_000_000);
            }
        }
    }

    #[test]
    fn siblings_get_the_same_rest_and_take_off_at_successive_tiers() {
        let extent = (1024, 720);
        let trio: [_; BUTTERFLIES_PER_SKETCH] =
            std::array::from_fn(|lane| LiveButterfly::new(7, 0, TEST_BIRTH, lane));
        let geometry = |lane: usize, now| {
            let mut result = Vec::new();
            trio[lane].triangles(extent, now, |triangle, outline| {
                result.push((triangle, outline))
            });
            result
        };
        let drawn = geometry(0, 0);
        assert!(!drawn.is_empty());
        for (lane, butterfly) in trio.iter().enumerate() {
            let reveal = lane as u64 * BIRTH_REST_NS;
            if lane > 0 {
                assert!(geometry(lane, reveal - 1).is_empty());
                assert_eq!(reveal, trio[lane - 1].release_ns);
            }
            assert_eq!(butterfly.release_ns - reveal, BIRTH_REST_NS);
            assert_eq!(geometry(lane, reveal), drawn);
            assert_eq!(geometry(lane, butterfly.release_ns), drawn);
        }
        assert_eq!(
            trio.last().unwrap().release_ns + TAKEOFF_NS,
            GROUP_TAKEOFF_END_NS
        );

        for size in [Vec2::new(1024.0, 720.0), Vec2::new(720.0, 1024.0)] {
            let mut area = size.x * size.y;
            let birth_depth = trio[0].flight(size, 0).depth;
            for (lane, butterfly) in trio.iter().enumerate() {
                let launched = butterfly.flight(size, butterfly.release_ns + TAKEOFF_NS);
                let [lo, hi] = panel(size, lane);
                assert!(launched.center.cmpge(lo).all() && launched.center.cmple(hi).all());
                assert_eq!(
                    launched.depth,
                    birth_depth * (lane as f32 - BIRTH_TIER as f32).exp2()
                );
                let span = hi - lo;
                assert_eq!(span.x * span.y, area * 0.5);
                area *= 0.5;
            }
        }
    }

    #[test]
    fn orbit_returns_to_the_path_and_shares_the_slow_wing_phase() {
        let size = Vec2::new(1024.0, 720.0);
        let butterfly = LiveButterfly::new(0, 0, TEST_BIRTH, 0);
        let orbit_start_ns = BIRTH_REST_NS + 12_500_000_000;
        let first = butterfly.flight(size, orbit_start_ns + 800_000_000);
        let second = butterfly.flight(size, orbit_start_ns + 1_300_000_000);
        assert!(((second.phase - first.phase).rem_euclid(1.0) - ORBIT_FLAP_HZ * 0.5).abs() < 1e-5);
        for angle in first.flaps.into_iter().chain(second.flaps) {
            assert!((72.0..=84.0).contains(&angle.to_degrees()));
        }
        assert_eq!(orbital_flight(0, 12.5).0, Vec2::ZERO);
        assert_eq!(orbital_flight(0, 15.0).0, Vec2::ZERO);
        assert!(orbital_flight(0, 13.6).0.length() > 1.0);
        for edge in [orbit_start_ns, orbit_start_ns + 2_500_000_000] {
            let before = butterfly.flight(size, edge - 1_000_000);
            let after = butterfly.flight(size, edge + 1_000_000);
            assert!(after.center.distance(before.center) < 1.0);
            assert!((after.phase - before.phase).rem_euclid(1.0) < 0.003);
        }
    }

    #[test]
    fn departure_descends_and_shrinks_at_the_seeded_rate() {
        let size = Vec2::new(3072.0, 1728.0);
        for seed in 0..128 {
            let butterfly = LiveButterfly::new(seed, 2_000_000_000, TEST_BIRTH, 0);
            let start = butterfly.started_ns;
            let duration = butterfly.duration_ns;
            let depth_at_birth = butterfly.flight(size, start).depth;
            let launch_end = BIRTH_REST_NS + TAKEOFF_NS;
            let mut previous = butterfly.flight(size, start + launch_end);
            for step in 1..=40 {
                let sample = butterfly.flight(
                    size,
                    start + launch_end + (duration - launch_end) * step / 40,
                );
                assert!(sample.depth > previous.depth);
                previous = sample;
            }
            assert_eq!(previous.center.y, EXIT_Y);
            assert_eq!(previous.depth, depth_at_birth * 64.0);
            assert!(!butterfly.has_departed(start + duration - 1));
            assert!(butterfly.has_departed(start + duration));
        }
        let first = LiveButterfly::new(0, 0, TEST_BIRTH, 0);
        let second = LiveButterfly::new(2, 0, TEST_BIRTH, 0);
        assert_ne!(first.duration_ns, second.duration_ns);
        let first = first.flight(size, BIRTH_REST_NS + 2_000_000_000);
        let second = second.flight(size, BIRTH_REST_NS + 2_000_000_000);
        assert_ne!(first.center.x, second.center.x);
        assert_ne!(first.bank, second.bank);
    }

    #[test]
    fn flapping_geometry_sheds_triangles_and_exits_below_the_viewport() {
        let start = 2_000_000_000;
        let duration = LiveButterfly::new(3, start, TEST_BIRTH, 0).duration_ns;
        let points = |extent: (u32, u32), now| {
            let mut butterfly = LiveButterfly::new(3, start, TEST_BIRTH, 0);
            let size = Vec2::new(extent.0 as f32, extent.1 as f32);
            let pixels_per_unit = 512.0 * size.min_element() / 720.0;
            butterfly.update_projection(projection(extent, pixels_per_unit), extent, now);
            let flight = butterfly.flight(size, now);
            let mut output = Vec::new();
            butterfly.triangles(extent, now, |triangle, outline| {
                let [a, b, c] = triangle;
                assert!((b - a).truncate().perp_dot((c - a).truncate()) > 0.0);
                assert!(triangle.iter().all(|point| point.is_finite()));
                if flight.takeoff == 1.0 {
                    assert_eq!(a.z, flight.clip_depths[usize::from(outline)]);
                }
                output.push(triangle);
            });
            output
        };
        let extent = (1024, 720);
        let original = points(extent, 2_000_000_000);
        assert_eq!(original, points(extent, 2_800_000_000));
        let flapped = points(extent, 2_800_000_000 + BIRTH_REST_NS);
        assert_ne!(original, flapped);
        assert!(points(extent, start + duration / 2).len() < original.len());
        assert!(points((320, 240), 2_000_000_000).len() < original.len());
        assert!(points((7680, 4320), 2_000_000_000).len() <= MAX_TRIANGLES);
        assert!(points((7680, 4320), start + duration - 1_000_000).is_empty());
        assert!(points(extent, start + duration).is_empty());
    }
}
