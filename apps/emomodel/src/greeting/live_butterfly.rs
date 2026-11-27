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
const FOCAL_LENGTH: f32 = 8_000.0;
const FLAP_NS: u64 = 4_000_000_000;
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
    phase: f32,
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
        Self {
            artwork,
            fill: fill_color(seed, artwork),
            fill_curves,
            outline_curves,
            started_ns,
            phase: (seed & 7) as f32 / 8.0,
        }
    }

    pub(super) fn fill(&self) -> [u8; 4] {
        self.fill
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
        let [lo, hi] = panel(size);
        let center = (lo + hi) * 0.5;
        let fit = (size.max_element() / 3072.0)
            .min(size.min_element() / 1728.0)
            .min(1.0);
        let depth = 6.0 * (1 << TIER) as f32 / fit;
        let [bottom, half_span, top] = WING_BOUNDS[self.artwork];
        let span = (2.0 * half_span).max(top - bottom) * (FOCAL_LENGTH / depth);
        let (fill_steps, outline_steps, width) = detail(span);
        let phase =
            (now_ns.saturating_sub(self.started_ns) % FLAP_NS) as f32 / FLAP_NS as f32 + self.phase;
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
        for (side, phase_offset) in [(1.0, 0.0), (-1.0, 0.135)] {
            let flap = (32.0_f32.to_radians()
                * ((phase + phase_offset) * std::f32::consts::TAU).sin())
            .sin_cos();
            let (sin, cos) = flap;
            let project = |point: Vec2| {
                center
                    + Vec2::new(side * cos * point.x, point.y) * FOCAL_LENGTH
                        / (depth - point.x * sin)
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

fn panel(size: Vec2) -> [Vec2; 2] {
    let mut rest = [Vec2::ZERO, size];
    let mut panel = rest;
    // Bfly takes the singleton half at each split and recurses into the rest.
    for _ in 0..=TIER {
        panel = rest;
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
            panel(Vec2::new(1920.0, 1080.0)),
            [Vec2::new(960.0, 540.0), Vec2::new(1920.0, 1080.0)]
        );
        assert_eq!(
            panel(Vec2::new(800.0, 1200.0)),
            [Vec2::ZERO, Vec2::new(400.0, 600.0)]
        );
        assert_eq!(detail(5.0), (4, 4, 2.0));
        assert_eq!(detail(16.0), (16, 8, 2.0));
        assert_eq!(detail(512.0), (64, 256, 3.0));
        assert_eq!(detail(1024.0), (64, 512, 4.0));
    }

    #[test]
    fn flap_deforms_the_wings_without_changing_the_triangle_budget() {
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
        assert_eq!(original.len(), flapped.len());
        assert_ne!(original, flapped);
        assert_eq!(original, points(extent, 6_000_000_000));
        assert!(points((320, 240), 2_000_000_000).len() < original.len());
        assert!(points((7680, 4320), 2_000_000_000).len() <= MAX_TRIANGLES);
    }
}
