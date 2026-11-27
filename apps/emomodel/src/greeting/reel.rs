use std::{fs, io::ErrorKind, path::Path};

use anyhow::{Context, Result, ensure};
use glam::{Mat4, Quat, Vec3};

use super::{foot_contact::FootProbe, hand_contact::HandProbe};
use crate::{
    animation::{Animation, SAMPLE_HZ},
    mesh::BODY_MATERIAL,
};

pub(super) const GROUND_Y: f32 = 0.0;
const SOLE_CLEARANCE: f32 = 0.004;
const CROUCH_SOLE_CLEARANCE: f32 = 0.002;
const CROUCH_FINGERTIP_CLEARANCE: f32 = 0.0035;
const MAX_CROUCH_DROP: f32 = 0.065;
const MAX_LANDING_DROP: f32 = 0.007;
const CROUCH_RELEASE: [f32; 2] = [1.42, 1.78];
const LANDING_ENTER: [f32; 2] = [2.51, 2.70];
const LANDING_EXIT: [f32; 2] = [3.72, 4.02];
const PLANT_HEIGHT: f32 = 0.026;
const MAX_PLANT_ADJUSTMENT: f32 = 0.10;

struct Action {
    title: &'static str,
    file: Option<&'static str>,
    transition: &'static str,
    transition_seconds: f32,
}

const ACTIONS: [Action; 8] = [
    Action {
        title: "Greeting",
        file: Some("02.glb"),
        transition: "Greeting-to-Nod",
        transition_seconds: 0.0,
    },
    Action {
        title: "Nod",
        file: None,
        transition: "Nod-to-Squat",
        transition_seconds: 1.05,
    },
    Action {
        title: "Squat",
        file: Some("07.glb"),
        transition: "Squat-to-Spin",
        transition_seconds: 0.48,
    },
    Action {
        title: "Spin",
        file: Some("05.glb"),
        transition: "Spin-to-Model Pose",
        transition_seconds: 0.58,
    },
    Action {
        title: "Model Pose",
        file: Some("06.glb"),
        transition: "Model Pose-to-Show Full Body",
        transition_seconds: 0.66,
    },
    Action {
        title: "Show Full Body",
        file: Some("01.glb"),
        transition: "Show Full Body-to-Shoot",
        transition_seconds: 0.48,
    },
    Action {
        title: "Shoot",
        file: Some("04.glb"),
        transition: "Shoot-to-Peace Sign",
        transition_seconds: 0.48,
    },
    Action {
        title: "Peace Sign",
        file: Some("03.glb"),
        transition: "Peace Sign-to-Greeting",
        transition_seconds: 1.50,
    },
];

#[derive(Clone, Copy)]
enum Part {
    Clip(usize),
    Bridge(usize),
}

struct Section {
    part: Part,
    start: usize,
    frames: usize,
}

#[derive(Clone, Copy)]
struct Endpoint {
    hips: Vec3,
    feet: [Vec3; 4],
    sole: f32,
}

#[derive(Clone, Copy)]
struct Anchor {
    point: usize,
    world: Vec3,
}

#[derive(Clone, Copy)]
struct FootPose {
    position: Vec3,
    rotation: Quat,
}

#[derive(Clone, Copy)]
struct ContactAdjustment {
    root_y: f32,
    ankle_lift: [f32; 2],
}

impl ContactAdjustment {
    fn apply(self, clip: &mut Animation, legs: [[usize; 3]; 2], mut offset: Vec3) -> Vec3 {
        for (leg, lift) in legs.into_iter().zip(self.ankle_lift) {
            if lift != 0.0 {
                let (_, rotation, position) =
                    clip.joint_transform(leg[2]).to_scale_rotation_translation();
                clip.place_foot(leg, position + Vec3::Y * lift, rotation);
            }
        }
        offset.y = self.root_y;
        offset
    }
}

struct WrapSteps {
    legs: [[usize; 3]; 2],
    from: [FootPose; 2],
    to: [FootPose; 2],
    from_offset: Vec3,
    to_offset: Vec3,
}

impl WrapSteps {
    fn apply(&self, clip: &mut Animation, at: f32, weight: f32) -> Vec3 {
        let offset = self.from_offset.lerp(self.to_offset, weight);
        for (leg, (start, end, rise)) in [(0.08, 0.46, 0.052), (0.48, 0.88, 0.040)]
            .into_iter()
            .enumerate()
        {
            let t = ((at - start) / (end - start)).clamp(0.0, 1.0);
            let progress = t * t * (3.0 - 2.0 * t);
            let lift = (std::f32::consts::PI * progress).sin().max(0.0) * rise;
            let from = self.from[leg];
            let to = self.to[leg];
            clip.place_foot(
                self.legs[leg],
                from.position.lerp(to.position, progress) + Vec3::Y * lift - offset,
                from.rotation.slerp(to.rotation, progress),
            );
        }
        offset
    }
}

pub(super) struct PlacedPose<'a> {
    pub(super) positions: &'a [[f32; 3]],
    pub(super) normals: &'a [[f32; 3]],
    pub(super) head_from_bind: Mat4,
    pub(super) joint_world: &'a [Mat4],
    pub(super) world_offset: Vec3,
}

pub(super) struct Reel {
    clips: Vec<Animation>,
    sections: Vec<Section>,
    frames: usize,
    offsets: [Vec3; ACTIONS.len()],
    anchors: [Option<Anchor>; ACTIONS.len()],
    probe: FootProbe,
    soles: [Vec<usize>; 2],
    greeting_contacts: Vec<Option<ContactAdjustment>>,
    wrap: WrapSteps,
    world_positions: Vec<[f32; 3]>,
}

impl Reel {
    pub(super) fn load(
        root: &Path,
        glb: &[u8],
        materials: &[&str],
        body_start: usize,
    ) -> Result<Option<Self>> {
        match fs::metadata(root) {
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
            result => ensure!(
                result
                    .with_context(|| format!("read motion directory {}", root.display()))?
                    .is_dir(),
                "motion path {} is not a directory",
                root.display()
            ),
        }

        let mut clips = Vec::with_capacity(ACTIONS.len());
        for action in &ACTIONS {
            let clip = match action.file {
                None => Animation::nod(glb, materials, clips.last_mut())?,
                Some(file) => {
                    let path = root.join(file);
                    let bytes = fs::read(&path).with_context(|| {
                        format!("read {} from {}", action.title, path.display())
                    })?;
                    // Each retargeted file has the single-clip Greeting GLB envelope.
                    Animation::prepare(&bytes, glb, materials)
                        .with_context(|| format!("load reel action {}", action.title))?
                }
            };
            if let Some(first) = clips.first() {
                ensure!(
                    clip.compatible_rig(first),
                    "reel action {} uses a different avatar rig",
                    action.title
                );
            }
            clips.push(clip);
        }

        let probe = FootProbe::new(glb, body_start)?;
        let (hips, legs, soles) = reference_vertices(glb, body_start)?;
        let mut starts = Vec::with_capacity(clips.len());
        let mut ends = Vec::with_capacity(clips.len());
        for clip in &mut clips {
            starts.push(endpoint(clip, 0.0, hips, &probe, &soles));
            ends.push(endpoint(clip, clip.duration(), hips, &probe, &soles));
        }
        let (offsets, anchors) = placement(&starts, &ends);
        let hands = HandProbe::new(glb, body_start)?;
        let greeting_contacts = greeting_contacts(&mut clips[0], offsets[0], &soles, &hands);
        let last = ACTIONS.len() - 1;
        let from_offset = cleared_offset(offsets[last], ends[last].sole);
        let final_seconds = clips[last].duration();
        clips[last].apply(final_seconds);
        let from = foot_poses(&clips[last], legs, from_offset);
        clips[0].apply(0.0);
        let to_offset = match greeting_contacts.first().copied().flatten() {
            Some(contact) => contact.apply(&mut clips[0], legs, offsets[0]),
            None => cleared_offset(offsets[0], starts[0].sole),
        };
        let wrap = WrapSteps {
            legs,
            from,
            to: foot_poses(&clips[0], legs, to_offset),
            from_offset,
            to_offset,
        };
        let mut sections = Vec::with_capacity(ACTIONS.len() * 2 - 1);
        let mut frames = 0;
        for (index, (action, clip)) in ACTIONS.iter().zip(&clips).enumerate() {
            let clip_frames = clip_frame_count(clip);
            sections.push(Section {
                part: Part::Clip(index),
                start: frames,
                frames: clip_frames,
            });
            frames += clip_frames;
            let bridge_frames = (action.transition_seconds * SAMPLE_HZ as f32).round() as usize;
            if bridge_frames > 0 {
                sections.push(Section {
                    part: Part::Bridge(index),
                    start: frames,
                    frames: bridge_frames,
                });
                frames += bridge_frames;
            }
        }
        Ok(Some(Self {
            clips,
            sections,
            frames,
            offsets,
            anchors,
            probe,
            soles,
            greeting_contacts,
            wrap,
            world_positions: Vec::new(),
        }))
    }

    pub(super) fn frame_count(&self) -> usize {
        self.frames
    }

    pub(super) fn bind_positions(
        &self,
        section: usize,
    ) -> impl ExactSizeIterator<Item = Vec3> + '_ {
        self.clips[0].bind_positions(section)
    }

    pub(super) fn phase(&self, frame: usize) -> &'static str {
        match self.section(frame % self.frames).part {
            Part::Clip(index) => ACTIONS[index].title,
            Part::Bridge(index) => ACTIONS[index].transition,
        }
    }

    pub(super) fn apply_bind_normals(
        &mut self,
        section: usize,
        normals: &[[f32; 3]],
    ) -> Result<()> {
        for clip in &mut self.clips {
            clip.apply_bind_normals(section, normals)?;
        }
        Ok(())
    }

    pub(super) fn sample(&mut self, frame: usize) -> Result<PlacedPose<'_>> {
        let frame = frame % self.frames;
        let Section {
            part,
            start,
            frames,
        } = *self.section(frame);
        let local = frame - start;
        let (clip, mut offset, anchor, sole_clearance) = match part {
            Part::Clip(index) => {
                let clip = &mut self.clips[index];
                let at = local as f32 / (frames - 1).max(1) as f32;
                clip.apply(at * clip.duration());
                let mut offset = self.offsets[index];
                let contact = if index == 0 {
                    self.greeting_contacts.get(local).copied().flatten()
                } else {
                    None
                };
                if let Some(contact) = contact {
                    offset = contact.apply(clip, self.wrap.legs, offset);
                }
                (
                    clip,
                    offset,
                    None,
                    contact.is_none().then_some(SOLE_CLEARANCE),
                )
            }
            Part::Bridge(index) => {
                let next = (index + 1) % ACTIONS.len();
                // The adjacent clips own the endpoints, including frame zero
                // at the wrap. The transition stores only the frames between.
                let at = (local + 1) as f32 / (frames + 1) as f32;
                let weight = if next == 0 {
                    // Ease the descent toward the opening crouch and let the
                    // cycle arrive with zero transition velocity at both ends.
                    at * at * at * (at * (6.0 * at - 15.0) + 10.0)
                } else {
                    at * at * (3.0 - 2.0 * at)
                };
                let (source, target) = if next == 0 {
                    let (first, later) = self.clips.split_first_mut().expect("loaded reel clips");
                    (&mut later[index - 1], first)
                } else {
                    let (prior, following) = self.clips.split_at_mut(next);
                    (&mut prior[index], &mut following[0])
                };
                source.apply_blended(source.duration(), target, 0.0, weight);
                let offset = if next == 0 {
                    self.wrap.apply(source, at, weight)
                } else {
                    self.offsets[index].lerp(self.offsets[next], weight)
                };
                // The planted foot can dip slightly during the ankle roll.
                // Use the opening pose's clearance through the closing steps.
                let clearance = if next == 0 {
                    CROUCH_SOLE_CLEARANCE
                } else {
                    SOLE_CLEARANCE
                };
                (source, offset, self.anchors[index], Some(clearance))
            }
        };
        let head = clip.head_from_bind().context("missing reel head joint")?;
        let pose = clip.skin();
        if let Some(anchor) = anchor {
            let point = ground_points(&self.probe, pose.positions)[anchor.point];
            offset += flat(anchor.world - point - offset);
        }
        if let Some(clearance) = sole_clearance {
            let [left, right] = sole_heights(pose.positions, &self.soles);
            offset.y = offset.y.max(GROUND_Y + clearance - left.min(right));
        }
        self.world_positions.clear();
        self.world_positions.extend(
            pose.positions
                .iter()
                .map(|&p| (Vec3::from_array(p) + offset).to_array()),
        );
        Ok(PlacedPose {
            positions: &self.world_positions,
            normals: pose.normals,
            head_from_bind: Mat4::from_translation(offset) * head,
            joint_world: pose.joint_world,
            world_offset: offset,
        })
    }

    fn section(&self, frame: usize) -> &Section {
        let next = self
            .sections
            .partition_point(|section| section.start <= frame);
        &self.sections[next - 1]
    }
}

fn reference_vertices(
    glb: &[u8],
    body_start: usize,
) -> Result<(usize, [[usize; 3]; 2], [Vec<usize>; 2])> {
    let asset = gltf::Gltf::from_slice(glb).context("parse the reel's reference rig")?;
    let joint = |name| {
        asset
            .nodes()
            .find(|node| node.name() == Some(name))
            .map(|node| node.index())
            .with_context(|| format!("missing reel joint {name}"))
    };
    let hips = joint("J_Bip_C_Hips")?;
    let legs = [
        [
            joint("J_Bip_L_UpperLeg")?,
            joint("J_Bip_L_LowerLeg")?,
            joint("J_Bip_L_Foot")?,
        ],
        [
            joint("J_Bip_R_UpperLeg")?,
            joint("J_Bip_R_LowerLeg")?,
            joint("J_Bip_R_Foot")?,
        ],
    ];
    let toes = [joint("J_Bip_L_ToeBase")?, joint("J_Bip_R_ToeBase")?];
    let (node, primitive) = asset
        .nodes()
        .find_map(|node| {
            let primitive = node
                .mesh()?
                .primitives()
                .find(|primitive| primitive.material().name() == Some(BODY_MATERIAL))?;
            Some((node, primitive))
        })
        .context("missing reel body mesh")?;
    let slots: Vec<_> = node
        .skin()
        .context("missing reel body skin")?
        .joints()
        .map(|node| {
            legs.iter()
                .zip(toes)
                .position(|(leg, toe)| leg.contains(&node.index()) || toe == node.index())
        })
        .collect();
    let reader = primitive.reader(|buffer| {
        matches!(buffer.source(), gltf::buffer::Source::Bin)
            .then_some(asset.blob.as_deref())
            .flatten()
    });
    let bind: Vec<_> = reader
        .read_positions()
        .context("missing reel body positions")?
        .collect();
    let joints: Vec<_> = reader
        .read_joints(0)
        .context("missing reel body joint indices")?
        .into_u16()
        .collect();
    ensure!(
        bind.len() == joints.len(),
        "inconsistent reel body attributes"
    );
    let mut soles = [Vec::new(), Vec::new()];
    for index in reader
        .read_indices()
        .context("missing reel body indices")?
        .into_u32()
        .map(|index| index as usize)
    {
        let position = bind.get(index).context("invalid reel body index")?;
        let side = slots
            .get(usize::from(joints[index][0]))
            .context("invalid reel joint slot")?;
        if position[1] < 0.10
            && let Some(side) = *side
        {
            soles[side].push(body_start + index);
        }
    }
    for sole in &mut soles {
        sole.sort_unstable();
        sole.dedup();
        ensure!(!sole.is_empty(), "missing reel sole vertices");
    }
    Ok((hips, legs, soles))
}

fn foot_poses(clip: &Animation, legs: [[usize; 3]; 2], offset: Vec3) -> [FootPose; 2] {
    legs.map(|[_, _, foot]| {
        let (_, rotation, position) = clip.joint_transform(foot).to_scale_rotation_translation();
        FootPose {
            position: position + offset,
            rotation,
        }
    })
}

fn endpoint(
    clip: &mut Animation,
    t: f32,
    hips: usize,
    probe: &FootProbe,
    soles: &[Vec<usize>; 2],
) -> Endpoint {
    clip.apply(t);
    let hips = clip.joint_position(hips);
    let pose = clip.skin();
    let [left, right] = sole_heights(pose.positions, soles);
    Endpoint {
        hips,
        feet: ground_points(probe, pose.positions),
        sole: left.min(right),
    }
}

fn ground_points(probe: &FootProbe, positions: &[[f32; 3]]) -> [Vec3; 4] {
    let [
        [(left_heel, _), (left_toe, _)],
        [(right_heel, _), (right_toe, _)],
    ] = probe.sample(positions).map(|foot| foot.points());
    [left_heel, left_toe, right_heel, right_toe]
}

fn sole_heights(positions: &[[f32; 3]], soles: &[Vec<usize>; 2]) -> [f32; 2] {
    soles.each_ref().map(|sole| {
        sole.iter()
            .map(|&index| positions[index][1])
            .fold(f32::INFINITY, f32::min)
    })
}

fn clip_frame_count(clip: &Animation) -> usize {
    (clip.duration() * SAMPLE_HZ as f32).round() as usize + 1
}

fn greeting_contacts(
    clip: &mut Animation,
    offset: Vec3,
    soles: &[Vec<usize>; 2],
    hands: &HandProbe,
) -> Vec<Option<ContactAdjustment>> {
    let tick = clip.duration() / (clip_frame_count(clip) - 1).max(1) as f32;
    (0..clip_frame_count(clip))
        .map(|frame| frame as f32 * tick)
        .take_while(|&time| time <= LANDING_EXIT[1])
        .map(|time| {
            let crouch = 1.0 - eased_range(time, CROUCH_RELEASE);
            let landing =
                eased_range(time, LANDING_ENTER) * (1.0 - eased_range(time, LANDING_EXIT));
            if crouch == 0.0 && landing == 0.0 {
                return None;
            }
            clip.apply(time);
            let pose = clip.skin();
            let heights = sole_heights(pose.positions, soles);
            let [left, right] = heights;
            let root = cleared_offset(offset, left.min(right)).y;
            if crouch > 0.0 {
                let fingertip = hands
                    .sample(pose.positions)
                    .into_iter()
                    .map(|(point, _)| point.y + root)
                    .fold(f32::INFINITY, f32::min);
                // The arms in the first crouch are nearly straight. Lower the
                // body to the fingertips and solve the legs toward the soles.
                let drop = (fingertip - CROUCH_FINGERTIP_CLEARANCE).clamp(0.0, MAX_CROUCH_DROP);
                let ankle_lift = heights.map(|sole| {
                    let foot_drop = (sole + root - CROUCH_SOLE_CLEARANCE).clamp(0.0, drop);
                    (drop - foot_drop) * crouch
                });
                Some(ContactAdjustment {
                    root_y: root - drop * crouch,
                    ankle_lift,
                })
            } else {
                let drop = (left.min(right) + root - SOLE_CLEARANCE).clamp(0.0, MAX_LANDING_DROP);
                Some(ContactAdjustment {
                    root_y: root - drop * landing,
                    ankle_lift: [0.0; 2],
                })
            }
        })
        .collect()
}

fn eased_range(time: f32, [start, end]: [f32; 2]) -> f32 {
    let t = ((time - start) / (end - start)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn cleared_offset(mut offset: Vec3, sole: f32) -> Vec3 {
    offset.y = offset.y.max(SOLE_CLEARANCE - sole);
    offset
}

fn flat(point: Vec3) -> Vec3 {
    Vec3::new(point.x, 0.0, point.z)
}

fn placement(
    starts: &[Endpoint],
    ends: &[Endpoint],
) -> ([Vec3; ACTIONS.len()], [Option<Anchor>; ACTIONS.len()]) {
    let mut offsets = [Vec3::ZERO; ACTIONS.len()];
    let mut anchors = [None; ACTIONS.len()];
    // Center the opening take. Nod starts at the exact Greeting landing.
    offsets[0] = -flat(starts[0].hips) + Vec3::Y * (SOLE_CLEARANCE - ends[0].sole);
    offsets[1] = offsets[0];
    for index in 2..starts.len() {
        let prior = ends[index - 1];
        let next = starts[index];
        let prior_offset = offsets[index - 1];
        let near_floor = SOLE_CLEARANCE - next.sole.min(ends[index].sole);
        let hips_shift = flat(prior.hips + prior_offset - next.hips);
        let anchor = prior
            .feet
            .into_iter()
            .zip(next.feet)
            .enumerate()
            .filter_map(|(point, (prior, next))| {
                let world = prior + prior_offset;
                let shift = flat(world - next);
                let error = (shift - hips_shift).length();
                (world.y < PLANT_HEIGHT
                    && next.y + near_floor < PLANT_HEIGHT
                    && error <= MAX_PLANT_ADJUSTMENT)
                    .then_some((
                        Anchor { point, world },
                        shift,
                        error + world.y.max(0.0) + (next.y + near_floor).max(0.0),
                    ))
            })
            .min_by(|a, b| a.2.total_cmp(&b.2));
        let shift = anchor.map_or(hips_shift, |(anchor, shift, _)| {
            anchors[index - 1] = Some(anchor);
            shift
        });
        offsets[index] = shift + Vec3::Y * near_floor;
    }
    // The wrap returns to the opening world placement rather than accumulating
    // the trajectory's offset each time through the reel.
    (offsets, anchors)
}
