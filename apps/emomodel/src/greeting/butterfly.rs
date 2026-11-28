use anyhow::{Context, Result, ensure};
use glam::{Mat4, Quat, Vec2, Vec3};

#[cfg(target_os = "trueos")]
use super::pupil::Stage;
use super::{
    heart_arm::PointingArm,
    live_butterfly::{Birth, BirthAnchor, Curve, WING_BOUNDS, artwork_geometry},
    reel::PlacedPose,
};
use crate::animation::{Animation, RigPose, SAMPLE_HZ};

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

struct Trace {
    spans: Vec<Curve>,
    distance: Vec<f32>,
    closed: bool,
}

impl Trace {
    fn load(artwork: usize) -> Result<[Self; 4]> {
        let (mut right, mut inset, hinge) = artwork_geometry(artwork);
        let mirror_and_reverse = |mut curve: Curve| {
            curve.points.reverse();
            for point in &mut curve.points {
                point.x = -point.x;
            }
            curve
        };
        let left = right
            .iter()
            .rev()
            .copied()
            .map(mirror_and_reverse)
            .collect::<Vec<_>>();
        right.extend(left);

        // Finish the first inset on the hinge side and start the second there.
        if inset.points[0].x < inset.points[3].x {
            inset.points.reverse();
        }
        Ok([
            Self::new(right, true)?,
            Self::new(vec![inset], false)?,
            Self::new(vec![mirror_and_reverse(inset)], false)?,
            Self::new(vec![hinge], false)?,
        ])
    }

    fn new(spans: Vec<Curve>, closed: bool) -> Result<Self> {
        let (Some(first), Some(last)) = (spans.first(), spans.last()) else {
            anyhow::bail!("empty drawing curve");
        };
        for span in &spans {
            ensure!(
                span.points.iter().all(|point| point.is_finite()),
                "invalid drawing control point"
            );
        }
        ensure!(
            spans
                .windows(2)
                .all(|pair| pair[0].points[3].abs_diff_eq(pair[1].points[0], 0.000_001))
                && first.points[0].abs_diff_eq(last.points[3], 0.000_001) == closed,
            "drawing curve has unexpected joins"
        );

        let mut distance = Vec::with_capacity(spans.len() * ARC_SAMPLES + 1);
        distance.push(0.0);
        for span in &spans {
            let mut previous = span.points[0];
            for sample in 1..=ARC_SAMPLES {
                let point = span.sample(sample as f32 / ARC_SAMPLES as f32);
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
            return self.spans.last().unwrap().points[3];
        }
        let lower = upper - 1;
        let interval = self.distance[upper] - self.distance[lower];
        let fraction = (wanted - self.distance[lower]) / interval;
        let t = ((lower % ARC_SAMPLES) as f32 + fraction) / ARC_SAMPLES as f32;
        self.spans[lower / ARC_SAMPLES].sample(t)
    }
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
        curve: &Trace,
        draw_plane: Mat4,
        ground_offset: Vec3,
        start: usize,
        frames: usize,
    ) -> Self {
        let count = frames * STROKE_SUBDIVISIONS;
        let targets: Vec<_> = (0..=count)
            .map(|i| {
                let progress = trace_progress(i as f32 / count as f32);
                draw_plane.transform_point3(curve.at(progress).extend(0.0))
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
        // Keep some weight on the inside foot when the drawing arm crosses back.
        let support = Vec3::new(
            0.10 * step.follow * (1.0 - 0.65 * across) - 0.043 * step.unload,
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
    strokes: [Stroke; 4],
    draw_to_world: Mat4,
    positions: Vec<[f32; 3]>,
    offset: Vec3,
}

impl Butterfly {
    pub(super) fn new(
        glb: &[u8],
        materials: &[&str],
        body_end: usize,
        artwork: usize,
    ) -> Result<Self> {
        let [outline, inner_right, inner_left, hinge] = Trace::load(artwork)?;
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
        let [bottom, half_span, top] = WING_BOUNDS[artwork];
        let scale = BUTTERFLY_WIDTH / (2.0 * half_span);
        // The avatar faces -Z, so screen right is world -X.
        let draw_plane = Mat4::from_scale_rotation_translation(
            Vec3::new(-scale, scale, 1.0),
            Quat::IDENTITY,
            center - Vec3::Y * ((bottom + top) * 0.5 * scale),
        );
        let butterfly_stroke = |curve: &Trace, previous: Option<&Stroke>| {
            let start_point = draw_plane.transform_point3(curve.at(0.0).extend(0.0));
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
            let frames = (curve.length() * scale * SAMPLE_HZ as f32 / DRAW_SPEED)
                .round()
                .max(12.0) as usize;
            Stroke::new(curve, draw_plane, offset, start, frames)
        };
        let outline = butterfly_stroke(&outline, None);
        let inner_right = butterfly_stroke(&inner_right, Some(&outline));
        let inner_left = butterfly_stroke(&inner_left, Some(&inner_right));
        let hinge = butterfly_stroke(&hinge, Some(&inner_left));
        let strokes = [outline, inner_right, inner_left, hinge];
        let first = strokes[0].start;
        let last = strokes[3].end();
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
            strokes[3].last_point(),
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
            draw_to_world: Mat4::from_translation(offset) * draw_plane,
            positions: Vec::new(),
            offset,
        })
    }

    pub(super) fn frame_count(&self) -> usize {
        self.draw_end() + LOWER + 1
    }

    pub(super) fn drawing_complete(&self, frame: usize) -> bool {
        frame >= self.draw_end()
    }

    fn draw_end(&self) -> usize {
        self.strokes[3].end()
    }

    pub(super) fn birth(
        &self,
        frame: usize,
        view_projection: Mat4,
        viewport: (u32, u32),
    ) -> Option<Birth> {
        let (width, height) = viewport;
        if !self.drawing_complete(frame) || width.min(height) == 0 {
            return None;
        }

        // Keep both the exact drawing plane and its screen bounds for flight.
        let mut min = Vec3::splat(f32::INFINITY);
        let mut max = Vec3::splat(f32::NEG_INFINITY);
        let mut min_world_x = f32::INFINITY;
        let mut max_world_x = f32::NEG_INFINITY;
        for &point in &self.strokes[0].targets {
            let world = point + self.offset;
            let clip = view_projection.transform_point3(world);
            min = min.min(clip);
            max = max.max(clip);
            min_world_x = min_world_x.min(world.x);
            max_world_x = max_world_x.max(world.x);
        }
        let midpoint = (min + max) * 0.5;
        Some(Birth {
            center: midpoint.truncate() * 0.5 + Vec2::splat(0.5),
            width_pixels: (max.x - min.x) * 0.5 * width as f32,
            depth: midpoint.z,
            draw_to_clip: view_projection * self.draw_to_world,
            stroke_width: 2.0 * STROKE_HALF_WIDTH / self.draw_to_world.x_axis.truncate().length(),
            anchor: BirthAnchor {
                draw_to_world: self.draw_to_world,
                center: view_projection.inverse().transform_point3(midpoint),
                width: Vec3::X * (max_world_x - min_world_x),
            },
        })
    }

    pub(super) fn drawing_light(&self, frame: usize) -> Option<(Vec3, f32)> {
        let drawing_frames = frame.checked_sub(self.strokes[0].start)?;
        let target = self.target(frame.min(self.draw_end()))?;
        let ramp = smooth((2.0 * drawing_frames as f32 / SAMPLE_HZ as f32).min(1.0));
        Some((target + self.offset, ramp))
    }

    pub(super) fn ink_tip(&self, frame: usize) -> Option<Vec3> {
        let stroke = self
            .strokes
            .iter()
            .find(|stroke| (stroke.start..=stroke.end()).contains(&frame))?;
        Some(stroke.targets[(frame - stroke.start) * STROKE_SUBDIVISIONS] + self.offset)
    }

    pub(super) fn phase(&self, frame: usize) -> &'static str {
        match frame {
            0..HOLD => "Butterfly ready",
            HOLD..DRAW_START => "Butterfly lift",
            _ if frame >= self.frame_count() - 1 => "Butterfly complete",
            _ if frame > self.draw_end() => "Butterfly lower",
            _ => self
                .strokes
                .iter()
                .zip([
                    "Butterfly outline",
                    "Butterfly right inset",
                    "Butterfly left inset",
                    "Butterfly hinge",
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
        let step = SideStep::at(frame, self.draw_end());
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
                (&self.closing, smooth(t), body, self.draw_end())
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

fn drawing_target(strokes: &[Stroke; 4], frame: usize) -> Option<Vec3> {
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

fn lead_x(strokes: &[Stroke; 4], frame: usize) -> f32 {
    let start = strokes[0].start;
    let end = strokes[3].end();
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
    fn live_birth_matches_both_artworks_through_the_camera_pullback() -> Result<()> {
        use super::super::live_butterfly::LiveButterfly;

        let glb = crate::HUMANOID_GLB;
        let rig = Animation::procedural(glb, &MATERIALS)?;
        let skin_end = rig.bind_positions(0).len() + rig.bind_positions(1).len();

        let initial_projection = Mat4::from_scale(Vec3::new(0.8, 0.85, 0.25))
            * Mat4::from_rotation_y(0.37)
            * Mat4::from_translation(Vec3::new(0.0, -0.93, 1.4));
        let pullback = Mat4::from_scale_rotation_translation(
            Vec3::new(0.8, 0.8, 1.0),
            Quat::IDENTITY,
            Vec3::new(0.0, -0.2, 0.05),
        );
        for artwork in [0, 1] {
            let butterfly = Butterfly::new(glb, &MATERIALS, skin_end, artwork)?;
            let complete = butterfly.draw_end();
            for (viewport, projection) in [
                ((640, 560), initial_projection),
                ((560, 640), pullback * initial_projection),
            ] {
                let pixels = Vec2::new(viewport.0 as f32, viewport.1 as f32);
                assert!(
                    butterfly
                        .birth(complete - 1, projection, viewport)
                        .is_none()
                );
                let birth = butterfly.birth(complete, projection, viewport).unwrap();
                let expected_center = birth.center * pixels;
                let expected_width = birth.width_pixels;
                let clip_to_birth = birth.draw_to_clip.inverse();
                let original = butterfly
                    .birth(complete, initial_projection, viewport)
                    .unwrap();
                let mut live = LiveButterfly::new(artwork as u32, 0, original, 0);
                live.update_projection(projection, viewport, 750_000_000);
                let mut min = Vec2::splat(f32::INFINITY);
                let mut max = Vec2::splat(f32::NEG_INFINITY);
                let mut live_outline = Vec::new();
                let pixel = |clip: Vec3| (clip.truncate() * 0.5 + Vec2::splat(0.5)) * pixels;
                live.triangles(viewport, 750_000_000, |triangle, outline| {
                    if outline {
                        live_outline.push(triangle.map(pixel));
                        for clip in triangle {
                            let point = pixel(clip);
                            min = min.min(point);
                            max = max.max(point);
                            let local = clip_to_birth.transform_point3(clip);
                            assert!(local.z.abs() < 0.01, "{local:?}");
                        }
                    }
                });
                assert!(((min + max) * 0.5).distance(expected_center) < 2.5);
                assert!((max.x - min.x - expected_width).abs() < 4.0);
                for stroke in &butterfly.strokes {
                    for &target in stroke.targets.iter().step_by(8) {
                        let drawn = pixel(projection.transform_point3(target + butterfly.offset));
                        assert!(
                            live_outline.iter().any(|triangle| {
                                (0..3).all(|i| {
                                    let p = triangle[i];
                                    let edge = triangle[(i + 1) % 3] - p;
                                    edge.perp_dot(drawn - p) >= -edge.length()
                                })
                            }),
                            "artwork {artwork} loses the drawn line at pixel {drawn:?}"
                        );
                    }
                }
            }
        }
        Ok(())
    }

    #[test]
    fn drawing_light_follows_the_trace_and_holds_the_endpoint() -> Result<()> {
        let glb = crate::HUMANOID_GLB;
        let rig = Animation::procedural(glb, &MATERIALS)?;
        let skin_end = rig.bind_positions(0).len() + rig.bind_positions(1).len();
        let butterfly = Butterfly::new(glb, &MATERIALS, skin_end, 0)?;
        let start = butterfly.strokes[0].start;
        assert_eq!(butterfly.drawing_light(start - 1), None);
        assert_eq!(
            butterfly.drawing_light(start),
            Some((butterfly.strokes[0].first_point() + butterfly.offset, 0.0)),
        );
        let middle = start + SAMPLE_HZ as usize / 4;
        let (target, light) = butterfly.drawing_light(middle).unwrap();
        assert_eq!(target, butterfly.target(middle).unwrap() + butterfly.offset);
        assert!(light > 0.0 && light < 1.0);
        let last = butterfly.strokes[3].last_point() + butterfly.offset;
        assert_eq!(
            butterfly.drawing_light(butterfly.frame_count() + 1),
            Some((last, 1.0)),
        );
        Ok(())
    }

    #[test]
    fn ink_tip_tracks_the_ribbon_and_goes_out_between_strokes() -> Result<()> {
        let glb = crate::HUMANOID_GLB;
        let rig = Animation::procedural(glb, &MATERIALS)?;
        let skin_end = rig.bind_positions(0).len() + rig.bind_positions(1).len();
        for artwork in [0, 1] {
            let butterfly = Butterfly::new(glb, &MATERIALS, skin_end, artwork)?;
            assert_eq!(butterfly.ink_tip(DRAW_START - 1), None);
            for stroke in &butterfly.strokes {
                for frame in stroke.start + 1..=stroke.end() {
                    let [a, b] = *stroke.visible(frame).unwrap().last().unwrap();
                    assert!(
                        butterfly
                            .ink_tip(frame)
                            .unwrap()
                            .abs_diff_eq((a + b) * 0.5, 1e-6)
                    );
                }
            }
            assert_eq!(butterfly.ink_tip(butterfly.strokes[0].end() + 1), None);
            assert_eq!(butterfly.ink_tip(butterfly.draw_end() + 1), None);
        }
        Ok(())
    }

    #[test]
    fn drawing_traces_preserve_each_artworks_outline_and_inner_join() -> Result<()> {
        for (artwork, [bottom, half_span, top]) in WING_BOUNDS.into_iter().enumerate() {
            let [outline, right, left, hinge] = Trace::load(artwork)?;
            assert_eq!(outline.at(0.0), outline.at(1.0));
            assert_eq!(right.at(1.0), left.at(0.0));
            assert!(hinge.at(0.0).x.abs() < 0.000_001);
            assert!(hinge.at(1.0).x.abs() < 0.000_001);
            let mut low = Vec2::splat(f32::INFINITY);
            let mut high = Vec2::splat(f32::NEG_INFINITY);
            for i in 0..=2048 {
                let p = outline.at(i as f32 / 2048.0);
                low = low.min(p);
                high = high.max(p);
            }
            assert!(
                low.abs_diff_eq(Vec2::new(-half_span, bottom), 0.000_02),
                "{low:?}"
            );
            assert!(
                high.abs_diff_eq(Vec2::new(half_span, top), 0.000_02),
                "{high:?}"
            );
        }
        Ok(())
    }

    #[test]
    fn sidestep_lands_before_the_far_wing_and_keeps_the_index_on_the_curve() -> Result<()> {
        let glb = crate::HUMANOID_GLB;
        let rig = Animation::procedural(glb, &MATERIALS)?;
        let skin_end = rig.bind_positions(0).len() + rig.bind_positions(1).len();
        for artwork in [0, 1] {
            check_stance(glb, skin_end, artwork)?;
        }
        Ok(())
    }

    fn check_stance(glb: &[u8], skin_end: usize, artwork: usize) -> Result<()> {
        let mut butterfly = Butterfly::new(glb, &MATERIALS, skin_end, artwork)?;
        let [inside, stepping] = butterfly.stance.feet.each_ref().map(|foot| foot.joints[2]);
        let draw_end = butterfly.draw_end();
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
        assert_eq!(result.len(), 4);
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
