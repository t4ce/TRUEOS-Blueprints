use anyhow::{Context, Result, ensure};
use glam::{Mat4, Vec2, Vec3};

#[cfg(target_os = "trueos")]
use super::pupil::Stage;
use super::{heart_arm::PointingArm, reel::PlacedPose};
use crate::animation::{Animation, RigPose};

const CURVE: &str = include_str!("../../assets/heart-curve.json");
const ARC_SAMPLES: usize = 96;
const HOLD: usize = 12;
const RAISE: usize = 36;
const TRACE: usize = 162;
const LOWER: usize = 38;
const DRAW_START: usize = HOLD + RAISE;
const DRAW_END: usize = DRAW_START + TRACE;
const END: usize = DRAW_END + LOWER;
const STROKE_SUBDIVISIONS: usize = 3;
const STROKE_HALF_WIDTH: f32 = 0.003;

type Span = [[f32; 3]; 3];

struct Curve {
    spans: Vec<Span>,
    distance: Vec<f32>,
}

impl Curve {
    fn load() -> Result<Self> {
        let spans: Vec<Span> = serde_json::from_str(CURVE).context("read authored heart curve")?;
        ensure!(!spans.is_empty(), "empty heart curve");
        for (index, span) in spans.iter().enumerate() {
            ensure!(
                span.iter().all(|&[x, y, weight]| {
                    x.is_finite() && y.is_finite() && weight.is_finite() && weight > 0.0
                }),
                "invalid heart control point"
            );
            ensure!(
                span[2][..2] == spans[(index + 1) % spans.len()][0][..2],
                "open heart curve at span {index}"
            );
        }

        let mut distance = Vec::with_capacity(spans.len() * ARC_SAMPLES + 1);
        distance.push(0.0);
        for span in &spans {
            let mut previous = evaluate(span, 0.0);
            for sample in 1..=ARC_SAMPLES {
                let point = evaluate(span, sample as f32 / ARC_SAMPLES as f32);
                distance.push(distance.last().unwrap() + point.distance(previous));
                previous = point;
            }
        }
        ensure!(
            distance.last().is_some_and(|length| *length > 0.0),
            "empty heart path"
        );
        Ok(Self { spans, distance })
    }

    fn at(&self, progress: f32) -> Vec2 {
        let wanted = progress * self.distance[self.distance.len() - 1];
        let upper = self.distance.partition_point(|&length| length <= wanted);
        if upper == self.distance.len() {
            return evaluate(self.spans.last().unwrap(), 1.0);
        }
        let lower = upper - 1;
        let interval = self.distance[upper] - self.distance[lower];
        let fraction = (wanted - self.distance[lower]) / interval;
        let t = ((lower % ARC_SAMPLES) as f32 + fraction) / ARC_SAMPLES as f32;
        evaluate(&self.spans[lower / ARC_SAMPLES], t)
    }
}

fn evaluate(span: &Span, t: f32) -> Vec2 {
    let u = 1.0 - t;
    let mut point = Vec2::ZERO;
    let mut total = 0.0;
    for (&[x, y, weight], basis) in span.iter().zip([u * u, 2.0 * u * t, t * t]) {
        let contribution = weight * basis;
        point += Vec2::new(x, y) * contribution;
        total += contribution;
    }
    point / total
}

/// Ease onto and off the closed contour, with steady speed between the ramps.
fn trace_progress(t: f32) -> f32 {
    const RAMP: f32 = 0.12;
    let distance = |t: f32| {
        let s = t / RAMP;
        RAMP * s * s * s * (1.0 - 0.5 * s) / (1.0 - RAMP)
    };
    if t < RAMP {
        distance(t)
    } else if t > 1.0 - RAMP {
        1.0 - distance(1.0 - t)
    } else {
        (t - 0.5 * RAMP) / (1.0 - RAMP)
    }
}

fn smooth(t: f32) -> f32 {
    t * t * (3.0 - 2.0 * t)
}

pub(super) struct Heart {
    animation: Animation,
    arm: PointingArm,
    rest: RigPose,
    opening: RigPose,
    targets: Vec<Vec3>,
    stroke: Vec<[Vec3; 2]>,
    positions: Vec<[f32; 3]>,
    offset: Vec3,
}

impl Heart {
    pub(super) fn new(glb: &[u8], materials: &[&str], body_end: usize) -> Result<Self> {
        let curve = Curve::load()?;
        let mut animation = Animation::nod(glb, materials, None)?;
        animation.apply(0.0);
        let arm = PointingArm::new(glb, &animation, 1)?;
        let mut rest = RigPose::default();
        let mut opening = RigPose::default();
        let surface = animation.skin();
        let (lowest, highest) = surface.positions[..body_end]
            .iter()
            .fold((f32::INFINITY, f32::NEG_INFINITY), |(low, high), p| {
                (low.min(p[1]), high.max(p[1]))
            });
        let height = highest - lowest;
        ensure!(
            height.is_finite() && height > 0.0,
            "invalid standing avatar height"
        );
        let offset = Vec3::Y * -lowest;
        rest.save(surface.joint_world, Vec3::ZERO);
        let shoulder = arm.shoulder(&animation);
        let center = shoulder + Vec3::new(0.05, -0.07, -0.40);
        let size = height * 0.25;
        let sample_count = TRACE * STROKE_SUBDIVISIONS;
        let mut targets: Vec<_> = (0..=sample_count)
            .map(|i| {
                let point = curve.at(trace_progress(i as f32 / sample_count as f32));
                // The avatar faces -Z, so screen right is world -X.
                center + size * Vec3::new(-point.x, point.y, 0.0)
            })
            .collect();
        targets[sample_count] = targets[0];
        arm.place(&mut animation, targets[0]);
        ensure!(
            arm.tip(&animation).distance(targets[0]) < 0.001,
            "avatar cannot reach the heart's starting point"
        );
        opening.save(animation.skin().joint_world, Vec3::ZERO);

        let stroke = targets
            .iter()
            .enumerate()
            .map(|(i, &point)| {
                let index = i % sample_count;
                let previous = (index + sample_count - 1) % sample_count;
                let direction = targets[index + 1] - targets[previous];
                let normal = direction.cross(Vec3::Z).normalize_or_zero() * STROKE_HALF_WIDTH;
                [point + offset + normal, point + offset - normal]
            })
            .collect();
        Ok(Self {
            animation,
            arm,
            rest,
            opening,
            targets,
            stroke,
            positions: Vec::new(),
            offset,
        })
    }

    pub(super) fn frame_count(&self) -> usize {
        END + 1
    }

    pub(super) fn phase(&self, frame: usize) -> &'static str {
        match frame {
            0..HOLD => "Heart ready",
            HOLD..DRAW_START => "Heart lift",
            DRAW_START..=DRAW_END => "Heart trace",
            END.. => "Heart complete",
            _ => "Heart lower",
        }
    }

    #[cfg(target_os = "trueos")]
    pub(super) fn pupil_stage(&self, frame: usize) -> Stage {
        match frame {
            0..HOLD | END.. => Stage::Neutral,
            HOLD..DRAW_START => Stage::Attentive,
            DRAW_START..=DRAW_END => Stage::Wide,
            _ => Stage::Engaged,
        }
    }

    pub(super) fn stroke(&self, frame: usize) -> &[[Vec3; 2]] {
        if frame <= DRAW_START {
            return &[];
        }
        let end = frame.min(DRAW_END) - DRAW_START;
        &self.stroke[..=end * STROKE_SUBDIVISIONS]
    }

    pub(super) fn apply_bind_normals(
        &mut self,
        section: usize,
        normals: &[[f32; 3]],
    ) -> Result<()> {
        self.animation.apply_bind_normals(section, normals)
    }

    #[cfg(target_os = "trueos")]
    pub(super) fn bind_positions(
        &self,
        section: usize,
    ) -> impl ExactSizeIterator<Item = Vec3> + '_ {
        self.animation.bind_positions(section)
    }

    pub(super) fn sample(&mut self, frame: usize) -> Result<PlacedPose<'_>> {
        if (DRAW_START..=DRAW_END).contains(&frame) {
            self.animation.apply(0.0);
            self.arm.place(
                &mut self.animation,
                self.targets[(frame - DRAW_START) * STROKE_SUBDIVISIONS],
            );
        } else {
            let weight = if frame < DRAW_START {
                smooth(frame.saturating_sub(HOLD) as f32 / RAISE as f32)
            } else {
                smooth(END.saturating_sub(frame) as f32 / LOWER as f32)
            };
            self.animation
                .blend_pose(&self.rest, &self.opening.joints, Vec3::ZERO, weight);
        }

        let head = self
            .animation
            .head_from_bind()
            .context("missing heart pose head")?;
        let pose = self.animation.skin();
        self.positions.clear();
        self.positions.extend(
            pose.positions
                .iter()
                .map(|&[x, y, z]| [x, y + self.offset.y, z]),
        );
        Ok(PlacedPose {
            positions: &self.positions,
            normals: pose.normals,
            head_from_bind: Mat4::from_translation(self.offset) * head,
            joint_world: pose.joint_world,
            world_offset: self.offset,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MATERIALS: [&str; 2] = [crate::mesh::FACE_MATERIAL, crate::mesh::BODY_MATERIAL];

    #[test]
    fn authored_curve_closes_at_the_notch_with_its_original_aspect() -> Result<()> {
        let curve = Curve::load()?;
        assert_eq!(curve.at(0.0), curve.at(1.0));
        assert!(curve.at(0.002).x > curve.at(0.0).x);
        let mut low = Vec2::splat(f32::INFINITY);
        let mut high = Vec2::splat(f32::NEG_INFINITY);
        for i in 0..=2048 {
            let p = curve.at(i as f32 / 2048.0);
            low = low.min(p);
            high = high.max(p);
        }
        let size = high - low;
        assert!((size.x - 1.072_386).abs() < 0.000_02, "{size:?}");
        assert!((size.y - 1.0).abs() < 0.000_02, "{size:?}");
        Ok(())
    }

    #[test]
    fn skinned_index_traces_a_quarter_height_heart_once() -> Result<()> {
        let glb = crate::HUMANOID_GLB;
        let rig = Animation::procedural(glb, &MATERIALS)?;
        let skin_end = rig.bind_positions(0).len() + rig.bind_positions(1).len();
        let mut heart = Heart::new(glb, &MATERIALS, skin_end)?;
        let rest = heart.sample(0)?;
        let standing_height = rest.positions.iter().map(|p| p[1]).fold(0.0, f32::max);
        let head = rest.head_from_bind;
        let rest_joints = rest.joint_world.to_vec();
        assert!(heart.stroke(DRAW_START).is_empty());
        let mut max_error = 0.0_f32;
        for frame in DRAW_START..=DRAW_END {
            let pose = heart.sample(frame)?;
            assert_eq!(pose.head_from_bind, head);
            let expected = heart.targets[(frame - DRAW_START) * STROKE_SUBDIVISIONS];
            max_error = max_error.max(heart.arm.tip(&heart.animation).distance(expected));
        }
        assert!(max_error < 0.000_5, "maximum fingertip error {max_error} m");
        let mut low = f32::INFINITY;
        let mut high = f32::NEG_INFINITY;
        for point in &heart.targets {
            low = low.min(point.y);
            high = high.max(point.y);
        }
        assert!((high - low - 0.25 * standing_height).abs() < 0.000_5);
        let completed = heart.stroke(DRAW_END);
        assert_eq!(completed.first(), completed.last());
        assert_eq!(completed, heart.stroke(usize::MAX));
        let end = heart.sample(END)?;
        for (a, b) in end.joint_world.iter().zip(rest_joints) {
            assert!(a.abs_diff_eq(b, 0.000_02));
        }
        Ok(())
    }
}
