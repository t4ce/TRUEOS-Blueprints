use anyhow::{Context, Result, ensure};
use glam::{Mat4, Quat, Vec2, Vec3};

#[cfg(target_os = "trueos")]
use super::pupil::Stage;
use super::{heart_arm::PointingArm, reel::PlacedPose};
use crate::animation::{Animation, RigPose, SAMPLE_HZ};

const BUTTERFLY_CURVES: &str = include_str!("../../assets/butterfly-curve.json");
const ARC_SAMPLES: usize = 96;
const HOLD: usize = 12;
const RAISE: usize = 36;
const LOWER: usize = 52;
const DRAW_START: usize = HOLD + RAISE;
const STEP_START: usize = DRAW_START + 86;
const STEP_LAND: usize = STEP_START + 30;
const STEP_DISTANCE: f32 = 0.14;
const STEP_LIFT: f32 = 0.037;
const RETURN_START: usize = 12;
const RETURN_LAND: usize = 44;
const BUTTERFLY_WIDTH: f32 = 0.29 * 3.0;
const DRAW_SPEED: f32 = 0.52;
const STROKE_SUBDIVISIONS: usize = 3;
const STROKE_HALF_WIDTH: f32 = 0.003;

type Span = [Vec3; 4];

fn weighted([x, y, w]: [f32; 3]) -> Vec3 {
    Vec3::new(x * w, y * w, w)
}

struct Curve {
    spans: Vec<Span>,
    distance: Vec<f32>,
    closed: bool,
}

impl Curve {
    fn load() -> Result<[Self; 3]> {
        let [outline, right, left]: [Vec<[[f32; 3]; 4]>; 3] =
            serde_json::from_str(BUTTERFLY_CURVES).context("read authored butterfly curves")?;
        let read = |spans: Vec<[[f32; 3]; 4]>, closed| {
            Self::new(
                spans.into_iter().map(|span| span.map(weighted)).collect(),
                closed,
            )
        };
        Ok([
            read(outline, true)?,
            read(right, false)?,
            read(left, false)?,
        ])
    }

    fn new(spans: Vec<Span>, closed: bool) -> Result<Self> {
        let (Some(first), Some(last)) = (spans.first(), spans.last()) else {
            anyhow::bail!("empty drawing curve");
        };
        for span in &spans {
            ensure!(
                span.iter().all(|point| point.is_finite() && point.z > 0.0),
                "invalid drawing control point"
            );
        }
        ensure!(
            spans
                .windows(2)
                .all(|pair| evaluate(&pair[0], 1.0) == evaluate(&pair[1], 0.0))
                && (evaluate(first, 0.0) == evaluate(last, 1.0)) == closed,
            "drawing curve has unexpected joins"
        );

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
            "empty drawing path"
        );
        Ok(Self {
            spans,
            distance,
            closed,
        })
    }

    fn length(&self) -> f32 {
        self.distance[self.distance.len() - 1]
    }

    fn at(&self, progress: f32) -> Vec2 {
        let wanted = progress * self.length();
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
    let [a, b, c, d] = *span;
    let point = a * (u * u * u) + b * (3.0 * u * u * t) + c * (3.0 * u * t * t) + d * (t * t * t);
    point.truncate() / point.z
}

/// Ease onto and off each stroke, with steady speed between the ramps.
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

struct Stroke {
    start: usize,
    frames: usize,
    targets: Vec<Vec3>,
    ribbon: Vec<[Vec3; 2]>,
}

impl Stroke {
    fn new(
        curve: &Curve,
        center: Vec3,
        size: f32,
        ground_offset: Vec3,
        start: usize,
        frames: usize,
    ) -> Self {
        let count = frames * STROKE_SUBDIVISIONS;
        let targets: Vec<_> = (0..=count)
            .map(|i| {
                let progress = trace_progress(i as f32 / count as f32);
                drawing_point(curve.at(progress), center, size)
            })
            .collect();
        let ribbon = targets
            .iter()
            .enumerate()
            .map(|(i, &point)| {
                let direction = if curve.closed && (i == 0 || i == count) {
                    targets[1] - targets[count - 1]
                } else {
                    targets[(i + 1).min(count)] - targets[i.saturating_sub(1)]
                };
                let normal = direction.cross(Vec3::Z).normalize_or_zero() * STROKE_HALF_WIDTH;
                [
                    point + ground_offset + normal,
                    point + ground_offset - normal,
                ]
            })
            .collect();
        Self {
            start,
            frames,
            targets,
            ribbon,
        }
    }

    fn end(&self) -> usize {
        self.start + self.frames
    }

    fn first_point(&self) -> Vec3 {
        self.targets[0]
    }

    fn last_point(&self) -> Vec3 {
        self.targets[self.targets.len() - 1]
    }

    fn visible(&self, frame: usize) -> Option<&[[Vec3; 2]]> {
        let drawn = frame.saturating_sub(self.start).min(self.frames);
        (drawn > 0).then(|| &self.ribbon[..=drawn * STROKE_SUBDIVISIONS])
    }
}

fn drawing_point(point: Vec2, center: Vec3, size: f32) -> Vec3 {
    // The avatar faces -Z, so screen right is world -X.
    center + size * Vec3::new(-point.x, point.y, 0.0)
}

struct PlantedFoot {
    joints: [usize; 3],
    position: Vec3,
    rotation: Quat,
}

struct SideStep {
    spread: f32,
    lift: f32,
    unload: f32,
    follow: f32,
}

impl SideStep {
    fn at(frame: usize, draw_end: usize) -> Self {
        let ease = |start: usize, end: usize| {
            smooth((frame.saturating_sub(start) as f32 / (end - start) as f32).min(1.0))
        };
        let lift_off = draw_end + RETURN_START;
        let land = draw_end + RETURN_LAND;
        let outward = ease(STEP_START, STEP_LAND);
        let inward = ease(lift_off, land);
        let arc = |t: f32| 16.0 * t * t * (1.0 - t) * (1.0 - t);
        // The pelvis unloads the moving foot before the swing and follows after landing.
        let unload_out =
            ease(STEP_START - 12, STEP_START) * (1.0 - ease(STEP_LAND, STEP_LAND + 14));
        let unload_back = ease(draw_end + 2, lift_off) * (1.0 - ease(land, draw_end + LOWER));
        Self {
            spread: outward - inward,
            lift: STEP_LIFT * (arc(outward) + arc(inward)),
            unload: unload_out + unload_back,
            follow: ease(STEP_LAND, STEP_LAND + 40) * (1.0 - ease(draw_end - 79, draw_end)),
        }
    }

    fn foot_offset(&self) -> Vec3 {
        // The avatar faces -Z, so its right foot moves toward screen left.
        Vec3::new(STEP_DISTANCE * self.spread, self.lift, 0.0)
    }
}

struct Stance {
    hips: usize,
    hip_position: Vec3,
    hip_rotation: Quat,
    spine: usize,
    shoulder_x: f32,
    feet: [PlantedFoot; 2],
}

impl Stance {
    fn new(asset: &gltf::Gltf, animation: &Animation, arm: &PointingArm) -> Result<Self> {
        let joint = |name: &str| {
            asset
                .nodes()
                .find(|node| node.name() == Some(name))
                .map(|node| node.index())
                .with_context(|| format!("missing butterfly joint {name}"))
        };
        let foot = |side: char| -> Result<PlantedFoot> {
            let joints = [
                joint(&format!("J_Bip_{side}_UpperLeg"))?,
                joint(&format!("J_Bip_{side}_LowerLeg"))?,
                joint(&format!("J_Bip_{side}_Foot"))?,
            ];
            let (_, rotation, position) = animation
                .joint_transform(joints[2])
                .to_scale_rotation_translation();
            Ok(PlantedFoot {
                joints,
                position,
                rotation,
            })
        };
        let hips = joint("J_Bip_C_Hips")?;
        let (_, hip_rotation, hip_position) = animation
            .joint_transform(hips)
            .to_scale_rotation_translation();
        Ok(Self {
            hips,
            hip_position,
            hip_rotation,
            spine: joint("J_Bip_C_Spine")?,
            shoulder_x: arm.shoulder(animation).x,
            feet: [foot('L')?, foot('R')?],
        })
    }

    fn shift(&self, animation: &mut Animation, lead_x: f32, engagement: f32, step: &SideStep) {
        let ahead = lead_x - self.shoulder_x;
        let outward = smooth(((ahead - 0.04) / 0.42).clamp(0.0, 1.0));
        let across = smooth(((-ahead - 0.04) / 0.34).clamp(0.0, 1.0));
        let displacement = Vec3::new(0.0, -0.004, -0.006)
            + Vec3::new(0.031 * (1.0 - step.unload), -0.008, -0.022) * outward
            + Vec3::new(-0.018, -0.006, -0.009) * across;
        let support = Vec3::new(
            0.10 * step.follow - 0.043 * step.unload,
            -0.018 * step.spread - 0.005 * step.unload * (1.0 + step.spread) - 0.11 * step.lift,
            0.0,
        );
        let turn = Quat::from_rotation_y(0.04 * (across - outward) * engagement);
        animation.place_joint(
            self.hips,
            self.hip_position + displacement * engagement + support,
            turn * self.hip_rotation,
        );
        let [inside, stepping] = &self.feet;
        animation.place_foot(inside.joints, inside.position, inside.rotation);
        animation.place_foot(
            stepping.joints,
            stepping.position + step.foot_offset(),
            stepping.rotation,
        );
    }

    fn draw(
        &self,
        animation: &mut Animation,
        arm: &PointingArm,
        target: Vec3,
        lead_x: f32,
        step: &SideStep,
    ) {
        animation.apply(0.0);
        self.shift(animation, lead_x, 1.0, step);

        let shoulder_x = arm.shoulder(animation).x;
        let needed = smooth(((target.x - shoulder_x - 0.18) / 0.25).clamp(0.0, 1.0));
        let lead = smooth(((lead_x - shoulder_x - 0.10) / 0.36).clamp(0.0, 1.0));
        let weight = needed + (1.0 - needed) * 0.75 * lead;
        if weight > 0.0 {
            let lean = Quat::from_rotation_arc(Vec3::Y, Vec3::new(0.09, 1.0, -0.22).normalize());
            let (_, rest, _) = animation
                .joint_transform(self.spine)
                .to_scale_rotation_translation();
            animation.orient_joint(self.spine, Quat::IDENTITY.slerp(lean, weight) * rest);
        }
        arm.place(animation, target);
    }
}

pub(super) struct Butterfly {
    animation: Animation,
    arm: PointingArm,
    stance: Stance,
    rest: RigPose,
    opening: RigPose,
    closing: RigPose,
    strokes: [Stroke; 3],
    positions: Vec<[f32; 3]>,
    offset: Vec3,
}

impl Butterfly {
    pub(super) fn new(glb: &[u8], materials: &[&str], body_end: usize) -> Result<Self> {
        let [outline, inner_right, inner_left] = Curve::load()?;
        let asset = gltf::Gltf::from_slice(glb).context("parse butterfly rig")?;
        let mut animation = Animation::nod(glb, materials, None)?;
        animation.apply(0.0);
        let arm = PointingArm::new(glb, &animation, 1)?;
        let stance = Stance::new(&asset, &animation, &arm)?;
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
        // Keep the drawing plane centered where the heart was.
        let center = arm.shoulder(&animation) + Vec3::new(0.05, -0.07, -0.40);
        let butterfly_stroke = |curve: &Curve, previous: Option<&Stroke>| {
            let start_point = drawing_point(curve.at(0.0), center, BUTTERFLY_WIDTH);
            let start = if let Some(previous) = previous {
                let travel = previous.last_point().distance(start_point);
                let move_frames = if travel > 0.001 {
                    (travel * SAMPLE_HZ as f32 / 0.75).round().max(12.0) as usize
                } else {
                    1
                };
                previous.end() + move_frames
            } else {
                DRAW_START
            };
            let frames = (curve.length() * BUTTERFLY_WIDTH * SAMPLE_HZ as f32 / DRAW_SPEED)
                .round()
                .max(12.0) as usize;
            Stroke::new(curve, center, BUTTERFLY_WIDTH, offset, start, frames)
        };
        let outline = butterfly_stroke(&outline, None);
        let inner_right = butterfly_stroke(&inner_right, Some(&outline));
        let inner_left = butterfly_stroke(&inner_left, Some(&inner_right));
        let strokes = [outline, inner_right, inner_left];
        let first = strokes[0].start;
        let last = strokes[2].end();
        stance.draw(
            &mut animation,
            &arm,
            strokes[0].first_point(),
            lead_x(&strokes, first),
            &SideStep::at(first, last),
        );
        ensure!(
            arm.tip(&animation).distance(strokes[0].first_point()) < 0.001,
            "avatar cannot reach the first stroke"
        );
        opening.save(animation.skin().joint_world, Vec3::ZERO);
        stance.draw(
            &mut animation,
            &arm,
            strokes[2].last_point(),
            lead_x(&strokes, last),
            &SideStep::at(last, last),
        );
        let mut closing = RigPose::default();
        closing.save(animation.skin().joint_world, Vec3::ZERO);
        Ok(Self {
            animation,
            arm,
            stance,
            rest,
            opening,
            closing,
            strokes,
            positions: Vec::new(),
            offset,
        })
    }

    pub(super) fn frame_count(&self) -> usize {
        self.strokes[2].end() + LOWER + 1
    }

    pub(super) fn drawing_complete(&self, frame: usize) -> bool {
        frame >= self.strokes[2].end()
    }

    pub(super) fn phase(&self, frame: usize) -> &'static str {
        match frame {
            0..HOLD => "Butterfly ready",
            HOLD..DRAW_START => "Butterfly lift",
            _ if frame >= self.frame_count() - 1 => "Butterfly complete",
            _ if frame > self.strokes[2].end() => "Butterfly lower",
            _ => self
                .strokes
                .iter()
                .zip([
                    "Butterfly outline",
                    "Butterfly right inset",
                    "Butterfly left inset",
                ])
                .find_map(|(stroke, name)| {
                    (stroke.start..=stroke.end())
                        .contains(&frame)
                        .then_some(name)
                })
                .unwrap_or("Move to next stroke"),
        }
    }

    #[cfg(target_os = "trueos")]
    pub(super) fn pupil_stage(&self, frame: usize) -> Stage {
        match frame {
            0..HOLD => Stage::Neutral,
            _ if frame >= self.frame_count() - 1 => Stage::Neutral,
            HOLD..DRAW_START => Stage::Attentive,
            _ if self
                .strokes
                .iter()
                .any(|stroke| (stroke.start..=stroke.end()).contains(&frame)) =>
            {
                Stage::Wide
            }
            _ => Stage::Engaged,
        }
    }

    pub(super) fn strokes(&self, frame: usize) -> impl Iterator<Item = &[[Vec3; 2]]> {
        self.strokes
            .iter()
            .filter_map(move |stroke| stroke.visible(frame))
    }

    fn target(&self, frame: usize) -> Option<Vec3> {
        drawing_target(&self.strokes, frame)
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
        let step = SideStep::at(frame, self.strokes[2].end());
        if let Some(target) = self.target(frame) {
            self.stance.draw(
                &mut self.animation,
                &self.arm,
                target,
                lead_x(&self.strokes, frame),
                &step,
            );
        } else {
            let (key, arm_weight, body_weight, endpoint) = if frame < DRAW_START {
                let t = frame.saturating_sub(HOLD) as f32 / RAISE as f32;
                let arm = smooth(((t - 0.18) / 0.82).max(0.0));
                let body = smooth((t / 0.78).min(1.0));
                (&self.opening, arm, body, self.strokes[0].start)
            } else {
                let t = (self.frame_count() - 1).saturating_sub(frame) as f32 / LOWER as f32;
                let body = smooth((t / 0.77).min(1.0));
                (&self.closing, smooth(t), body, self.strokes[2].end())
            };
            self.animation
                .blend_pose(&self.rest, &key.joints, Vec3::ZERO, arm_weight);
            if body_weight > 0.0 {
                self.stance.shift(
                    &mut self.animation,
                    lead_x(&self.strokes, endpoint),
                    body_weight,
                    &step,
                );
            }
        }

        let head = self
            .animation
            .head_from_bind()
            .context("missing butterfly pose head")?;
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

fn drawing_target(strokes: &[Stroke; 3], frame: usize) -> Option<Vec3> {
    let next = strokes.partition_point(|stroke| stroke.start <= frame);
    let current = strokes.get(next.checked_sub(1)?)?;
    if frame <= current.end() {
        return Some(current.targets[(frame - current.start) * STROKE_SUBDIVISIONS]);
    }
    let following = strokes.get(next)?;
    let t = (frame - current.end()) as f32 / (following.start - current.end()) as f32;
    // Pull the fingertip a little off the drawing plane during travel.
    let lift = 0.035 * 16.0 * t * t * (1.0 - t) * (1.0 - t);
    Some(
        current
            .last_point()
            .lerp(following.first_point(), smooth(t))
            + Vec3::Z * lift,
    )
}

fn lead_x(strokes: &[Stroke; 3], frame: usize) -> f32 {
    let start = strokes[0].start;
    let end = strokes[2].end();
    // Average around a point roughly 220 ms ahead so the weight starts moving
    // early and doesn't pivot abruptly when the fingertip rounds a corner.
    [(-4, 0.15), (4, 0.20), (12, 0.40), (20, 0.25)]
        .into_iter()
        .map(|(offset, weight)| {
            let sample = frame.saturating_add_signed(offset).clamp(start, end);
            drawing_target(strokes, sample).unwrap().x * weight
        })
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    const MATERIALS: [&str; 2] = [crate::mesh::FACE_MATERIAL, crate::mesh::BODY_MATERIAL];

    #[test]
    fn authored_butterfly_preserves_its_outline_and_inner_join() -> Result<()> {
        let [outline, right, left] = Curve::load()?;
        assert_eq!(outline.at(0.0), outline.at(1.0));
        assert_eq!(right.at(1.0), left.at(0.0));
        let mut low = Vec2::splat(f32::INFINITY);
        let mut high = Vec2::splat(f32::NEG_INFINITY);
        for i in 0..=2048 {
            let p = outline.at(i as f32 / 2048.0);
            low = low.min(p);
            high = high.max(p);
        }
        let size = high - low;
        assert!((size.x - 1.0).abs() < 0.000_02, "{size:?}");
        assert!((size.y - 0.500_735).abs() < 0.000_02, "{size:?}");
        Ok(())
    }

    #[test]
    fn sidestep_lands_before_the_far_wing_and_keeps_the_index_on_the_curve() -> Result<()> {
        let glb = crate::HUMANOID_GLB;
        let rig = Animation::procedural(glb, &MATERIALS)?;
        let skin_end = rig.bind_positions(0).len() + rig.bind_positions(1).len();
        let mut butterfly = Butterfly::new(glb, &MATERIALS, skin_end)?;
        let [inside, stepping] = butterfly.stance.feet.each_ref().map(|foot| foot.joints[2]);
        let draw_end = butterfly.strokes[2].end();
        assert!(!butterfly.drawing_complete(draw_end - 1));
        assert!(butterfly.drawing_complete(draw_end));
        let spine = butterfly.stance.spine;
        let rest = butterfly.sample(0)?;
        let rest_joints = rest.joint_world.to_vec();
        let rest_center =
            butterfly.arm.shoulder(&butterfly.animation) + Vec3::new(0.05, -0.07, -0.40);
        assert_eq!(butterfly.strokes(DRAW_START).count(), 0);
        let hips = butterfly.stance.hips;
        let rest_hips = rest_joints[hips].w_axis.truncate();
        let far_reach = (DRAW_START..=butterfly.strokes[0].end())
            .find(|&frame| butterfly.target(frame).unwrap().x > rest_center.x + 0.31)
            .unwrap();
        assert!(STEP_LAND < far_reach);
        for frame in 0..butterfly.frame_count() {
            let expected = butterfly.target(frame);
            let pose = butterfly.sample(frame)?;
            let step = SideStep::at(frame, draw_end);
            let mut expected_step = rest_joints[stepping];
            expected_step.w_axis += step.foot_offset().extend(0.0);
            assert!(
                pose.joint_world[inside].abs_diff_eq(rest_joints[inside], 0.000_02),
                "support foot drifted at frame {frame}: {:?} vs {:?}",
                pose.joint_world[inside].w_axis,
                rest_joints[inside].w_axis
            );
            assert!(
                pose.joint_world[stepping].abs_diff_eq(expected_step, 0.000_02),
                "stepping foot missed at frame {frame}: {:?} vs {:?}",
                pose.joint_world[stepping].w_axis,
                expected_step.w_axis
            );
            if frame == (STEP_START + STEP_LAND) / 2 {
                assert!(
                    pose.joint_world[stepping].w_axis.y > rest_joints[stepping].w_axis.y + 0.03
                );
            }
            if frame == STEP_LAND {
                assert!(
                    pose.joint_world[stepping].w_axis.x > rest_joints[stepping].w_axis.x + 0.13
                );
            }
            if frame == HOLD + 6 {
                assert!(pose.joint_world[hips].w_axis.y < rest_hips.y - 0.000_2);
            }
            if frame == far_reach - 8 {
                assert!(pose.joint_world[hips].w_axis.x > rest_hips.x + 0.075);
            }
            if frame == far_reach {
                let upright = rest_joints[spine].transform_vector3(Vec3::Y).normalize();
                let leaned = pose.joint_world[spine]
                    .transform_vector3(Vec3::Y)
                    .normalize();
                assert!((leaned.x - upright.x).abs() < 0.12);
            }
            if let Some(expected) = expected {
                let error = butterfly.arm.tip(&butterfly.animation).distance(expected);
                assert!(
                    error < 0.000_5,
                    "frame {frame} missed {expected:?} by {error} m"
                );
            }
        }
        let mut low = Vec3::splat(f32::INFINITY);
        let mut high = Vec3::splat(f32::NEG_INFINITY);
        for &point in &butterfly.strokes[0].targets {
            low = low.min(point);
            high = high.max(point);
        }
        assert!((high.x - low.x - 0.87).abs() < 0.000_5);
        assert!(((low + high) * 0.5).abs_diff_eq(rest_center, 0.000_5));
        for (i, pair) in butterfly.strokes.windows(2).enumerate() {
            let [previous, next] = pair else {
                unreachable!()
            };
            assert_eq!(butterfly.strokes(previous.end()).count(), i + 1);
            assert!(
                butterfly
                    .strokes(previous.end())
                    .eq(butterfly.strokes(next.start))
            );
        }
        let result: Vec<_> = butterfly.strokes(usize::MAX).collect();
        assert_eq!(result.len(), 3);
        for (index, stroke) in result.into_iter().enumerate() {
            assert_eq!(stroke.first() == stroke.last(), index == 0);
        }
        let end = butterfly.sample(butterfly.frame_count() - 1)?;
        for (a, b) in end.joint_world.iter().zip(rest_joints) {
            assert!(a.abs_diff_eq(b, 0.000_02));
        }
        Ok(())
    }
}
