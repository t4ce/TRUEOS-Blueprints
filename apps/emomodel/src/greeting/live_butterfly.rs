use std::f32::consts::TAU;

use glam::{Vec2, Vec3};

// Snapshot of bfly::geometry::LAYERED_VERTEX_BYTES, with each position
// stored as little-endian [x, y, path_tag]. F5 uses the fill patches first.
const CONTROL_BYTES: &[u8; 960] = include_bytes!("../../assets/bfly-layered-controls.bin");
const PATCH_BYTES: usize = 4 * 3 * size_of::<f32>();
const FILL_BYTES: usize = CONTROL_BYTES.len() / 2;
const MAX_PATH_CURVES: usize = 5;
const MAX_FILL_STEPS: usize = 64;
const MAX_OUTLINE_STEPS: usize = 512;
pub(super) const MAX_TRIANGLES: usize =
    2 * (MAX_PATH_CURVES * (MAX_FILL_STEPS + 4 * MAX_OUTLINE_STEPS) + 2);

const TIER: usize = 1;
const LAST_TIER: usize = 7;
const FOCAL_LENGTH: f32 = 8_000.0;
const TIER_MOVE_NS: u64 = 4_400_000_000;
const DEPARTURE_LEAD_NS: u64 = 2_000_000_000;
const NANOS_TO_SECONDS: f32 = 1e-9;
const EXIT_Y: f32 = -16.0;
const INSET: f32 = 0.894_427_2; // 80% of the area.
const HINGE_Y: [f32; 2] = [0.011_652_005, -0.004_455_092_4];
const WING_BOUNDS: [[f32; 3]; 2] = [
    [-0.332_086_77, 0.662_223_34, 0.331_109_94],
    [-0.398_931_6, 0.652_023_5, 0.285_934_5],
];
// Keep the wings behind the avatar, with the outline just in front of the inset.
const FILL_DEPTH: f32 = 0.91;
const OUTLINE_DEPTH: f32 = 0.90;

#[derive(Clone, Copy)]
struct Curve {
    points: [Vec2; 4],
    straight: bool,
}

impl Curve {
    fn read(bytes: &[u8; PATCH_BYTES]) -> Self {
        let float = |offset| f32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap());
        Self {
            points: std::array::from_fn(|i| Vec2::new(float(i * 12), float(i * 12 + 4))),
            straight: float(8) as u32 & 1 != 0,
        }
    }

    fn sample(&self, t: f32) -> Vec2 {
        let [a, b, c, d] = self.points;
        if self.straight {
            return a.lerp(d, t);
        }
        let [p, q, r] = [a.lerp(b, t), b.lerp(c, t), c.lerp(d, t)];
        p.lerp(q, t).lerp(q.lerp(r, t), t)
    }
}

pub(super) struct LiveButterfly {
    artwork: usize,
    fill: [u8; 4],
    fill_curves: Vec<Curve>,
    outline_curves: Vec<Curve>,
    started_ns: u64,
    seed: u32,
    speed_percent: u32,
    duration_ns: u64,
    initial_orbit: Vec2,
}

struct FlightSample {
    center: Vec2,
    depth: f32,
    bank: Vec2,
    flaps: [f32; 2],
}

impl LiveButterfly {
    pub(super) fn new(seed: u32, started_ns: u64) -> Self {
        let artwork = (seed & 1) as usize;
        let range = if artwork == 0 { 0..4 } else { 4..10 };
        let curves = |offset: usize| -> Vec<Curve> {
            range
                .clone()
                .map(|patch| {
                    let start = offset + patch * PATCH_BYTES;
                    Curve::read(
                        CONTROL_BYTES[start..start + PATCH_BYTES]
                            .try_into()
                            .unwrap(),
                    )
                })
                .collect()
        };
        let fill_curves = curves(0);
        let mut outline_curves = curves(FILL_BYTES);
        // F5 outlines the closed fill contour and the original paths. Their
        // identical arcs need only one draw, including the reversed hinge.
        for &curve in &fill_curves {
            let mut reverse = curve.points;
            reverse.reverse();
            if !outline_curves
                .iter()
                .any(|existing| existing.points == curve.points || existing.points == reverse)
            {
                outline_curves.push(curve);
            }
        }
        let seed = seed & 127;
        let speed_percent = 85 + (seed * 17 + 13) % 31;
        let duration_ns = DEPARTURE_LEAD_NS
            + (LAST_TIER - TIER) as u64 * TIER_MOVE_NS * 100 / u64::from(speed_percent);
        Self {
            artwork,
            fill: fill_color(seed, artwork),
            fill_curves,
            outline_curves,
            started_ns,
            seed,
            speed_percent,
            duration_ns,
            initial_orbit: flight_path(seed, speed_percent, 0.0).0,
        }
    }

    pub(super) fn fill(&self) -> [u8; 4] {
        self.fill
    }

    pub(super) fn has_departed(&self, now_ns: u64) -> bool {
        now_ns.saturating_sub(self.started_ns) >= self.duration_ns
    }

    fn flight(&self, size: Vec2, now_ns: u64) -> FlightSample {
        let age_ns = now_ns.saturating_sub(self.started_ns).min(self.duration_ns);
        let age = age_ns as f32 * NANOS_TO_SECONDS;
        let progress = smoothstep(age_ns as f32 / self.duration_ns as f32);
        let level = TIER as f32 + progress * (LAST_TIER - TIER) as f32;
        let [lo, hi] = panel(size, TIER);
        let start = (lo + hi) * 0.5;
        let (orbit, tangent) = flight_path(self.seed, self.speed_percent, age);
        let orbit = orbit - self.initial_orbit;
        let variation = self.seed.wrapping_mul(0x9e37_79b9);
        let spread = 0.13 + (variation >> 24) as f32 * (0.06 / 255.0);
        let tier = level as usize;
        let extent = |tier| {
            let [lo, hi] = panel(size, tier);
            (hi - lo).min_element()
        };
        let radius_from = extent(tier);
        let radius_to = extent((tier + 1).min(LAST_TIER));
        let radius = spread * (radius_from + (radius_to - radius_from) * level.fract());
        // Attenuate the orbit at the bottom of the frame so the same trajectory
        // carries both wing variants past the edge regardless of their phase.
        let descent = progress + 0.06 * progress * (1.0 - progress) * orbit.y;
        let center = Vec2::new(
            start.x + orbit.x * radius * (1.0 - progress),
            start.y + (EXIT_Y - start.y) * descent,
        );
        let fit = (size.max_element() / 3072.0)
            .min(size.min_element() / 1728.0)
            .min(1.0);
        let bank = -18.0_f32.to_radians() * tangent.normalize_or_zero().x * smoothstep(age);
        FlightSample {
            center,
            depth: 6.0 * level.exp2() / fit,
            bank: Vec2::from_angle(bank),
            flaps: flap_angles(self.seed, age),
        }
    }

    pub(super) fn triangles(
        &self,
        extent: (u32, u32),
        now_ns: u64,
        mut emit: impl FnMut([Vec3; 3], bool),
    ) {
        if extent.0 == 0 || extent.1 == 0 {
            return;
        }
        let size = Vec2::new(extent.0 as f32, extent.1 as f32);
        if self.has_departed(now_ns) {
            return;
        }
        let FlightSample {
            center,
            depth,
            bank,
            flaps,
        } = self.flight(size, now_ns);
        let [bottom, half_span, top] = WING_BOUNDS[self.artwork];
        let span = (2.0 * half_span).max(top - bottom) * (FOCAL_LENGTH / depth);
        let (fill_steps, outline_steps, width) = detail(span);
        let up = top.max(-bottom) * bank.x + half_span * bank.y.abs();
        if center.y + up * FOCAL_LENGTH / (depth - half_span) + width < 0.0 {
            return;
        }
        let inset_origin = Vec2::new(0.0, HINGE_Y[self.artwork]);
        let mut triangle = |pixel: [Vec2; 3], outline: bool| {
            let z = if outline { OUTLINE_DEPTH } else { FILL_DEPTH };
            let mut clip = pixel.map(|p| (2.0 * p / size - 1.0).extend(z));
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
            let project = |point: Vec2| {
                let wing = Vec2::new(side * cos * point.x, point.y);
                center + bank.rotate(wing) * FOCAL_LENGTH / (depth - point.x * sin)
            };
            let origin = project(inset_origin);
            // The fill fan already has the shared hinge as its last edge.
            for curve in self.fill_curves.iter().filter(|curve| !curve.straight) {
                let mut last = project(inset_origin.lerp(curve.points[0], INSET));
                for step in 1..=fill_steps {
                    let point = curve.sample(step as f32 / fill_steps as f32);
                    let next = project(inset_origin.lerp(point, INSET));
                    triangle([origin, last, next], false);
                    last = next;
                }
            }
            for curve in &self.outline_curves {
                let mut last = project(curve.points[0]);
                let steps = if curve.straight { 1 } else { outline_steps };
                for step in 1..=steps {
                    let next = project(curve.sample(step as f32 / steps as f32));
                    if let Some(axis) = (next - last).try_normalize() {
                        // Square ends join adjacent samples without small gaps.
                        let end = axis * (width * 0.5);
                        let side = end.perp();
                        let [a, b, c, d] = [
                            last - end + side,
                            last - end - side,
                            next + end + side,
                            next + end - side,
                        ];
                        triangle([a, b, c], true);
                        triangle([c, b, d], true);
                    }
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

fn flap_angles(seed: u32, age: f32) -> [f32; 2] {
    let index = seed & 7;
    let offset = index as f32 / 8.0;
    let relaxed =
        [0.0, 0.135].map(|wing| 32.0_f32.to_radians() * ((age * 0.25 + offset + wing) * TAU).sin());
    let beat = age / 1.25 + offset;
    let sweep_step = (beat as u32 * 5 + index * 3) & 15;
    let sweep = 75.0 + 5.0 * sweep_step as f32;
    let turnaround = 0.5 - 0.5 * (TAU * beat.fract()).cos();
    let departure = (87.75 - sweep * turnaround).to_radians();
    let blend = smoothstep(age / 0.75);
    relaxed.map(|angle| angle + (departure - angle) * blend)
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
    let outline = if span < 16.0 { 4 } else { 1 << (level - 1) };
    let width = 2.0 + level.saturating_sub(8) as f32;
    (fill, outline, width)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn styles_follow_the_seed_and_preserve_the_two_f5_paths() {
        let original = LiveButterfly::new(0, 0);
        let alternate = LiveButterfly::new(1, 0);
        assert_eq!(original.fill_curves.len(), 4);
        assert_eq!(alternate.fill_curves.len(), 6);
        assert_eq!(original.outline_curves.len(), 6);
        assert_eq!(alternate.outline_curves.len(), 8);
        for butterfly in [&original, &alternate] {
            for curves in [&butterfly.fill_curves, &butterfly.outline_curves] {
                assert_eq!(curves.iter().filter(|curve| curve.straight).count(), 1);
                for curve in curves {
                    assert!(curve.points.iter().all(|point| point.is_finite()));
                }
            }
        }
        assert_eq!(original.fill(), [236, 209, 38, 217]);
        assert_eq!(alternate.fill(), [236, 38, 204, 217]);
    }

    #[test]
    fn second_tier_splits_the_remainder_on_its_longer_axis() {
        assert_eq!(
            panel(Vec2::new(1920.0, 1080.0), TIER),
            [Vec2::new(960.0, 540.0), Vec2::new(1920.0, 1080.0)]
        );
        assert_eq!(
            panel(Vec2::new(800.0, 1200.0), TIER),
            [Vec2::ZERO, Vec2::new(400.0, 600.0)]
        );
        assert_eq!(detail(5.0), (4, 4, 2.0));
        assert_eq!(detail(16.0), (16, 8, 2.0));
        assert_eq!(detail(512.0), (64, 256, 3.0));
        assert_eq!(detail(1024.0), (64, 512, 4.0));
    }

    #[test]
    fn wings_start_at_the_existing_size_and_panel_center() {
        let butterfly = LiveButterfly::new(3, 2_000_000_000);
        let size = Vec2::new(1920.0, 1080.0);
        let spawn = butterfly.flight(size, butterfly.started_ns);
        assert_eq!(spawn.center, Vec2::new(1440.0, 810.0));
        assert_eq!(spawn.depth, 12.0 / (1080.0 / 1728.0));
        assert_eq!(spawn.bank, Vec2::X);
        let phase = 3.0 / 8.0;
        assert_eq!(
            spawn.flaps,
            [0.0, 0.135].map(|offset| 32.0_f32.to_radians() * ((phase + offset) * TAU).sin())
        );
    }

    #[test]
    fn departure_descends_and_shrinks_at_the_seeded_rate() {
        let size = Vec2::new(3072.0, 1728.0);
        for seed in 0..128 {
            let butterfly = LiveButterfly::new(seed, 2_000_000_000);
            let start = butterfly.started_ns;
            let duration = butterfly.duration_ns;
            let mut previous = butterfly.flight(size, start);
            for step in 1..=40 {
                let sample = butterfly.flight(size, start + duration * step / 40);
                assert!(
                    sample.center.y < previous.center.y,
                    "seed {seed}, step {step}"
                );
                assert!(sample.depth > previous.depth);
                previous = sample;
            }
            assert_eq!(previous.center.y, EXIT_Y);
            assert_eq!(previous.depth, 12.0 * 64.0);
            assert!(!butterfly.has_departed(start + duration - 1));
            assert!(butterfly.has_departed(start + duration));
        }
        let first = LiveButterfly::new(0, 0);
        let second = LiveButterfly::new(2, 0);
        assert_ne!(first.duration_ns, second.duration_ns);
        let first = first.flight(size, 2_000_000_000);
        let second = second.flight(size, 2_000_000_000);
        assert_ne!(first.center.x, second.center.x);
        assert_ne!(first.bank, second.bank);
    }

    #[test]
    fn flapping_geometry_sheds_triangles_and_exits_below_the_viewport() {
        let butterfly = LiveButterfly::new(3, 2_000_000_000);
        let points = |extent, now| {
            let mut output = Vec::new();
            butterfly.triangles(extent, now, |triangle, outline| {
                let [a, b, c] = triangle;
                assert!((b - a).truncate().perp_dot((c - a).truncate()) > 0.0);
                assert!(triangle.iter().all(|point| point.is_finite()));
                assert_eq!(a.z, if outline { OUTLINE_DEPTH } else { FILL_DEPTH });
                output.push(triangle);
            });
            output
        };
        let extent = (1024, 720);
        let original = points(extent, 2_000_000_000);
        let flapped = points(extent, 2_800_000_000);
        assert_ne!(original, flapped);
        assert!(points(extent, 2_000_000_000 + butterfly.duration_ns / 2).len() < original.len());
        assert!(points((320, 240), 2_000_000_000).len() < original.len());
        assert!(points((7680, 4320), 2_000_000_000).len() <= MAX_TRIANGLES);
        assert!(
            points(
                (7680, 4320),
                2_000_000_000 + butterfly.duration_ns - 1_000_000
            )
            .is_empty()
        );
        assert!(points(extent, 2_000_000_000 + butterfly.duration_ns).is_empty());
    }
}
