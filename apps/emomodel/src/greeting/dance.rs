use anyhow::{Context, Result};
use glam::{EulerRot, Mat3, Quat, Vec2, Vec3};
use lico_graph::Graph;

#[path = "dance_graph.rs"]
mod dance_graph;

use dance_graph::Choreography;

use super::reel::PlacedPose;
use crate::{animation::Animation, mesh::BODY_MATERIAL};

const SOLE_CLEARANCE: f32 = 0.004;
const STEPS_PER_PHRASE: usize = 3;
const COUNTS_PER_PHRASE: usize = 4;
const FINGER_COUNT: usize = 4;

pub(super) struct Dance {
    animation: Animation,
    joints: Joints,
    rest_floor: f32,
    ankle_y: [f32; 2],
    choreography: Choreography,
}

struct Joints {
    hips: usize,
    spine: usize,
    chest: usize,
    head: usize,
    arms: [[usize; 3]; 2],
    legs: [[usize; 3]; 2],
    fingers: [[[usize; 2]; FINGER_COUNT]; 2],
}

impl Joints {
    fn read(asset: &gltf::Gltf) -> Result<Self> {
        let joint = |name: &str| {
            asset
                .nodes()
                .find(|node| node.name() == Some(name))
                .map(|node| node.index())
                .with_context(|| format!("missing dance joint {name}"))
        };
        let chain = |side, [upper, lower, end]: [&str; 3]| -> Result<_> {
            Ok([
                joint(&format!("J_Bip_{side}_{upper}"))?,
                joint(&format!("J_Bip_{side}_{lower}"))?,
                joint(&format!("J_Bip_{side}_{end}"))?,
            ])
        };
        let mut fingers = [[[0; 2]; FINGER_COUNT]; 2];
        for (hand, side) in fingers.iter_mut().zip(['L', 'R']) {
            for (joints, finger) in hand.iter_mut().zip(["Index", "Middle", "Ring", "Little"]) {
                joints[0] = joint(&format!("J_Bip_{side}_{finger}1"))?;
                joints[1] = joint(&format!("J_Bip_{side}_{finger}2"))?;
            }
        }
        Ok(Self {
            hips: joint("J_Bip_C_Hips")?,
            spine: joint("J_Bip_C_Spine")?,
            chest: joint("J_Bip_C_Chest")?,
            head: joint("J_Bip_C_Head")?,
            arms: [
                chain('L', ["UpperArm", "LowerArm", "Hand"])?,
                chain('R', ["UpperArm", "LowerArm", "Hand"])?,
            ],
            legs: [
                chain('L', ["UpperLeg", "LowerLeg", "Foot"])?,
                chain('R', ["UpperLeg", "LowerLeg", "Foot"])?,
            ],
            fingers,
        })
    }
}

fn rest_floor(asset: &gltf::Gltf) -> Result<f32> {
    let primitive = asset
        .meshes()
        .flat_map(|mesh| mesh.primitives())
        .find(|primitive| primitive.material().name() == Some(BODY_MATERIAL))
        .context("missing dance body mesh")?;
    let reader = primitive.reader(|_| asset.blob.as_deref());
    let positions = reader
        .read_positions()
        .context("missing dance body positions")?
        .collect::<Vec<_>>();
    reader
        .read_indices()
        .context("missing dance body indices")?
        .into_u32()
        .filter_map(|index| positions.get(index as usize).map(|p| p[1]))
        .min_by(f32::total_cmp)
        .context("missing dance sole")
}

#[derive(Clone, Copy)]
enum Arm {
    Carry,
    Frame,
    Reach,
    Line,
    Crown,
    Check,
    Lever,
    Rail,
}

#[derive(Clone, Copy)]
struct ArmShape {
    rotations: [Quat; 3],
    curl: f32,
}

impl Arm {
    fn shape(self, sign: f32) -> ArmShape {
        // The vectors describe outward, up and forward (negative Z) relative to the chest.
        let (elbow, forearm, wrist, curl) = match self {
            Self::Carry => (
                [0.32, -0.92, -0.12],
                [0.28, -0.89, -0.35],
                [-7.0, 4.0],
                20.0,
            ),
            Self::Frame => (
                [0.85, -0.39, -0.19],
                [0.11, 0.30, -0.94],
                [-20.0, 8.0],
                19.0,
            ),
            Self::Reach => (
                [0.56, -0.62, -0.56],
                [0.52, 0.20, -0.83],
                [3.0, -16.0],
                12.0,
            ),
            Self::Line => ([0.91, 0.30, -0.27], [0.90, 0.32, -0.33], [-6.0, 3.0], 4.0),
            Self::Crown => (
                [0.56, 0.76, -0.33],
                [-0.31, 0.92, -0.24],
                [16.0, -9.0],
                12.0,
            ),
            Self::Check => (
                [0.40, -0.73, -0.49],
                [-0.23, 0.61, -0.76],
                [16.0, -12.0],
                24.0,
            ),
            Self::Lever => (
                [0.92, -0.26, -0.29],
                [-0.08, 0.93, -0.36],
                [24.0, 18.0],
                11.0,
            ),
            Self::Rail => (
                [0.99, -0.04, -0.12],
                [-0.05, 0.16, -0.97],
                [-22.0, -4.0],
                8.0,
            ),
        };
        let mirror = |[x, y, z]: [f32; 3]| Vec3::new(sign * x, y, z).normalize();
        let upper = mirror(elbow);
        let fore = mirror(forearm);
        let y = sign * upper.cross(fore).normalize();
        let orient = |direction: Vec3| {
            let x = sign * direction;
            Quat::from_mat3(&Mat3::from_cols(x, y, x.cross(y))).normalize()
        };
        let upper = orient(upper);
        ArmShape {
            rotations: [
                upper,
                upper.conjugate() * orient(fore),
                Quat::from_rotation_y(radians(sign * wrist[0]))
                    * Quat::from_rotation_z(radians(sign * wrist[1])),
            ],
            curl,
        }
    }
}

#[derive(Clone, Copy)]
struct Key {
    arms: [Arm; 2],
    turn: f32,
    inclination: f32,
    knee_depth: f32,
    gaze: f32,
}

const fn key(
    left: Arm,
    right: Arm,
    turn: f32,
    inclination: f32,
    knee_depth: f32,
    gaze: f32,
) -> Key {
    Key {
        arms: [left, right],
        turn,
        inclination,
        knee_depth,
        gaze,
    }
}

#[derive(Clone, Copy)]
enum StepStyle {
    Glide,
    Snap,
    Flick,
    Close,
}

#[derive(Clone, Copy)]
struct Step {
    side: usize,
    x: f32,
    z: f32,
    yaw: f32,
    style: StepStyle,
}

const fn step(side: usize, x: f32, z: f32, yaw: f32, style: StepStyle) -> Step {
    Step {
        side,
        x,
        z,
        yaw,
        style,
    }
}

struct Phrase {
    title: &'static str,
    keys: [Key; 4],
    steps: [Step; STEPS_PER_PHRASE],
    // How much of an interval an arm travels before it settles.
    reach: f32,
}

use Arm::*;
use StepStyle::*;

const L: usize = 0;
const R: usize = 1;

// Eight movement phrases. The fourth count is an upper-body accent
// and a suspended weight transfer. In 3/4 the step and line on beat three own
// the cadence, so no step is omitted or doubled when a meter changes.
const PHRASES: [Phrase; 8] = [
    Phrase {
        title: "Tango frame",
        keys: [
            key(Frame, Check, -12.0, 3.0, 0.010, 12.0),
            key(Rail, Check, -4.0, -3.0, 0.006, -8.0),
            key(Frame, Lever, 11.0, -4.0, 0.013, 9.0),
            key(Check, Frame, 6.0, 1.0, 0.006, 10.0),
        ],
        steps: [
            step(L, -0.17, -0.09, 11.0, Glide),
            step(R, 0.11, -0.04, -12.0, Close),
            step(L, -0.09, -0.05, 7.0, Close),
        ],
        reach: 0.53,
    },
    Phrase {
        title: "Corte",
        keys: [
            key(Line, Check, -23.0, 9.0, 0.032, 15.0),
            key(Reach, Carry, -17.0, 7.0, 0.020, 22.0),
            key(Lever, Rail, 10.0, -5.0, 0.013, -15.0),
            key(Check, Lever, 6.0, 3.0, 0.012, 9.0),
        ],
        steps: [
            step(R, 0.22, 0.02, -23.0, Snap),
            step(L, -0.06, -0.11, 9.0, Glide),
            step(R, 0.15, -0.02, -11.0, Close),
        ],
        reach: 0.36,
    },
    Phrase {
        title: "Ocho",
        keys: [
            key(Check, Crown, 24.0, -7.0, 0.010, -23.0),
            key(Crown, Frame, -18.0, 5.0, 0.015, 20.0),
            key(Frame, Crown, 18.0, -6.0, 0.010, -22.0),
            key(Rail, Frame, 9.0, -2.0, 0.006, -7.0),
        ],
        steps: [
            step(L, -0.12, 0.13, 18.0, Glide),
            step(R, 0.16, -0.15, -19.0, Glide),
            step(L, -0.07, -0.02, 11.0, Close),
        ],
        reach: 0.76,
    },
    Phrase {
        title: "Robot lock",
        keys: [
            key(Lever, Rail, -11.0, -4.0, 0.007, 11.0),
            key(Rail, Lever, 15.0, 4.0, 0.010, -13.0),
            key(Lever, Lever, 0.0, 0.0, 0.021, 5.0),
            key(Check, Rail, -7.0, 4.0, 0.007, -17.0),
        ],
        steps: [
            step(R, 0.14, 0.07, -10.0, Snap),
            step(L, -0.16, 0.08, 10.0, Snap),
            step(R, 0.09, 0.04, -11.0, Close),
        ],
        reach: 0.32,
    },
    Phrase {
        title: "Tango hook",
        keys: [
            key(Line, Lever, -19.0, 5.0, 0.021, 19.0),
            key(Reach, Crown, 8.0, -6.0, 0.014, -9.0),
            key(Check, Line, 22.0, -8.0, 0.018, -20.0),
            key(Frame, Check, 9.0, 4.0, 0.011, -7.0),
        ],
        steps: [
            step(L, -0.055, 0.18, 7.0, Flick),
            step(R, 0.19, -0.12, -18.0, Glide),
            step(L, -0.12, 0.01, 12.0, Snap),
        ],
        reach: 0.50,
    },
    Phrase {
        title: "Promenade",
        keys: [
            key(Carry, Line, 22.0, -5.0, 0.013, -12.0),
            key(Frame, Reach, 11.0, 3.0, 0.008, 12.0),
            key(Line, Frame, -16.0, 7.0, 0.022, 20.0),
            key(Reach, Check, -9.0, 2.0, 0.009, 12.0),
        ],
        steps: [
            step(R, 0.23, -0.11, -17.0, Glide),
            step(L, -0.21, -0.085, 13.0, Glide),
            step(R, 0.08, 0.015, -8.0, Close),
        ],
        reach: 0.74,
    },
    Phrase {
        title: "Clockwork ripple",
        keys: [
            key(Rail, Check, 9.0, 3.0, 0.006, -10.0),
            key(Lever, Crown, -10.0, -4.0, 0.014, 14.0),
            key(Crown, Lever, 9.0, 4.0, 0.009, -16.0),
            key(Check, Check, -3.0, 0.0, 0.019, 7.0),
        ],
        steps: [
            step(L, -0.085, 0.105, 8.0, Snap),
            step(R, 0.11, 0.15, -10.0, Snap),
            step(L, -0.12, 0.045, 11.0, Close),
        ],
        reach: 0.42,
    },
    Phrase {
        title: "Suspended finish",
        keys: [
            key(Crown, Line, 18.0, -6.0, 0.014, -21.0),
            key(Line, Crown, -17.0, 5.0, 0.020, 17.0),
            key(Frame, Reach, -4.0, 3.0, 0.006, 13.0),
            key(Check, Frame, -11.0, 0.0, 0.008, 9.0),
        ],
        steps: [
            step(R, 0.18, 0.015, -14.0, Glide),
            step(L, -0.085, 0.04, 9.0, Close),
            step(R, 0.09, 0.05, -10.0, Close),
        ],
        reach: 0.65,
    },
];

const MOVE_COUNT: usize = PHRASES.len() * STEPS_PER_PHRASE;

#[derive(Clone, Copy)]
struct Count {
    phrase: usize,
    key: usize,
    t: f32,
}

impl Count {
    fn at(choreography: &Choreography, elapsed_seconds: f64) -> Option<Self> {
        let (count, t) = choreography.step(elapsed_seconds, PHRASES.len() * COUNTS_PER_PHRASE)?;
        Some(Self {
            phrase: count / COUNTS_PER_PHRASE,
            key: count % COUNTS_PER_PHRASE,
            t,
        })
    }

    fn upper_keys(self) -> (Key, Key) {
        let keys = &PHRASES[self.phrase].keys;
        let past = if self.key == 0 {
            PHRASES[previous(self.phrase, PHRASES.len())].keys[COUNTS_PER_PHRASE - 1]
        } else {
            keys[self.key - 1]
        };
        (past, keys[self.key])
    }
}

struct StepPose {
    feet: [Vec2; 2],
    yaw: [f32; 2],
    lift: [f32; 2],
    pitch: [f32; 2],
    root: Vec2,
}

impl StepPose {
    fn planted() -> Self {
        Self {
            feet: [Vec2::new(-0.11, 0.0), Vec2::new(0.11, 0.0)],
            yaw: [7.0, -7.0],
            lift: [0.0; 2],
            pitch: [0.0; 2],
            root: Vec2::ZERO,
        }
    }
}

fn previous(index: usize, len: usize) -> usize {
    (index + len - 1) % len
}

fn land_at(index: usize) -> Step {
    PHRASES[index / STEPS_PER_PHRASE].steps[index % STEPS_PER_PHRASE]
}

fn footsteps(count: Count) -> StepPose {
    let step_index = count.phrase * STEPS_PER_PHRASE + count.key;
    let swing = (count.key < STEPS_PER_PHRASE).then(|| land_at(step_index));
    let past = [L, R].map(|side| {
        (1..=MOVE_COUNT)
            .map(|behind| land_at((step_index + MOVE_COUNT - behind) % MOVE_COUNT))
            .find(|step| step.side == side)
            .expect("both feet step in the dance")
    });
    let mut feet = past.map(|step| Vec2::new(step.x, step.z));
    let mut yaw = past.map(|step| step.yaw);
    let mut lift = [0.0; 2];
    let mut pitch = [0.0; 2];
    let mut weight = 0.0;
    if let Some(step) = swing {
        let (start, end, height, toe, hook) = match step.style {
            Glide => (0.05, 0.94, 0.044, 6.0, 0.0),
            Snap => (0.10, 0.64, 0.043, 7.0, 0.0),
            Flick => (0.03, 0.91, 0.078, 11.0, 0.036),
            Close => (0.18, 0.76, 0.031, 5.0, 0.0),
        };
        let p = ease(count.t, start, end);
        let arc = 4.0 * p * (1.0 - p);
        let side = step.side;
        feet[side] = feet[side].lerp(Vec2::new(step.x, step.z), p);
        feet[side].x += (if side == L { hook } else { -hook }) * arc;
        yaw[side] = mix(yaw[side], step.yaw, p);
        lift[side] = height * arc;
        pitch[side] = toe * arc * (1.0 - 2.0 * p);
        weight = 0.32 * arc;
    }
    let midpoint = (feet[L] + feet[R]) * 0.5;
    // Transfer weight to the support leg while the other foot leaves the floor.
    let root = swing.map_or(midpoint, |step| midpoint.lerp(feet[1 - step.side], weight));
    StepPose {
        feet,
        yaw,
        lift,
        pitch,
        root,
    }
}

impl Dance {
    pub(super) fn new(glb: &[u8], materials: &[&str]) -> Result<Self> {
        let asset = gltf::Gltf::from_slice(glb).context("parse dance avatar")?;
        let joints = Joints::read(&asset)?;
        let rest_floor = rest_floor(&asset)?;
        let mut animation = Animation::procedural(glb, materials)?;
        animation.apply(0.0);
        let ankle_y = joints
            .legs
            .map(|leg| animation.joint_position(leg[2]).y - rest_floor + SOLE_CLEARANCE);
        Ok(Self {
            animation,
            joints,
            rest_floor,
            ankle_y,
            choreography: Choreography::default(),
        })
    }

    pub(super) fn apply_bind_normals(
        &mut self,
        section: usize,
        normals: &[[f32; 3]],
    ) -> Result<()> {
        self.animation.apply_bind_normals(section, normals)
    }

    pub(super) fn set_graph(&mut self, graph: &Graph) {
        self.choreography = Choreography::new(graph);
    }

    pub(super) fn framing_bounds() -> ([f32; 3], [f32; 3]) {
        ([-0.83, 0.0, -0.70], [0.79, 1.72, 0.36])
    }

    pub(super) fn phrase(&self, elapsed_seconds: f64) -> &'static str {
        if let Some(count) = Count::at(&self.choreography, elapsed_seconds) {
            PHRASES[count.phrase].title
        } else if self.choreography.suspended {
            "Suspended line"
        } else {
            "Planted isolations"
        }
    }

    pub(super) fn sample(&mut self, elapsed_seconds: f64) -> Result<PlacedPose<'_>> {
        let elapsed_seconds = elapsed_seconds.max(0.0);
        let count = Count::at(&self.choreography, elapsed_seconds);
        let (from, to, reach, progress, pose) = if let Some(count) = count {
            let (from, to) = count.upper_keys();
            (
                from,
                to,
                PHRASES[count.phrase].reach,
                count.t,
                footsteps(count),
            )
        } else {
            let from = key(Carry, Carry, 0.0, 0.0, 0.0, 0.0);
            let to = if self.choreography.suspended {
                key(Line, Reach, 13.0, -3.0, 0.014, -11.0)
            } else {
                key(Frame, Check, -9.0, 2.0, 0.018, 10.0)
            };
            (
                from,
                to,
                0.77,
                (elapsed_seconds as f32 / 1.8).clamp(0.0, 1.0),
                StepPose::planted(),
            )
        };
        let reach = (reach + self.choreography.sweep).clamp(0.52, 0.87);
        let expression = self.choreography.expression(elapsed_seconds);
        let body = ease(progress, 0.0, 0.51 + 0.27 * reach);
        let yaw = radians(mix(from.turn, to.turn, body) + 8.0 * expression.lead);
        let roll = radians(mix(from.inclination, to.inclination, body) + 5.0 * expression.chest);
        let knee_depth = mix(from.knee_depth, to.knee_depth, body);
        let gaze = radians(mix(from.gaze, to.gaze, ease(progress, 0.0, 0.26)));
        let root = Vec3::new(
            pose.root.x,
            -self.rest_floor - 0.036 - knee_depth - 0.015 * expression.grounded,
            pose.root.y - 0.028,
        );
        let joints = &self.joints;
        let mut rotations = [(0, Quat::IDENTITY); 32];
        rotations[..4].copy_from_slice(&[
            (
                joints.hips,
                Quat::from_euler(EulerRot::YXZ, yaw, radians(1.0), roll * 0.27),
            ),
            (
                joints.spine,
                Quat::from_euler(EulerRot::YXZ, -yaw * 0.16, radians(2.0), roll * 0.42),
            ),
            (
                joints.chest,
                Quat::from_euler(
                    EulerRot::YXZ,
                    -yaw * 0.21,
                    radians(-3.0 + 7.0 * expression.chest + 2.6 * expression.grounded),
                    roll * 0.31,
                ),
            ),
            (
                joints.head,
                Quat::from_euler(
                    EulerRot::YXZ,
                    gaze - yaw * 0.45,
                    radians(-2.0 - 5.2 * expression.grounded + 3.5 * expression.lead),
                    -roll * 0.68,
                ),
            ),
        ]);
        let mut cursor = 4;
        for side in [L, R] {
            let sign = if side == L { -1.0 } else { 1.0 };
            let bend = Quat::from_rotation_x(radians(16.0));
            rotations[cursor..cursor + 3].copy_from_slice(&[
                (joints.legs[side][0], bend),
                (joints.legs[side][1], Quat::from_rotation_x(radians(-31.0))),
                (joints.legs[side][2], bend),
            ]);
            cursor += 3;
            let a = from.arms[side].shape(sign);
            let b = to.arms[side].shape(sign);
            // Shoulders lead the frame. Elbows, wrists and fingers lock on in succession.
            for (i, node) in joints.arms[side].into_iter().enumerate() {
                let delay = i as f32 * 0.047;
                let t = ease(progress, delay, delay + reach);
                let accent = match i {
                    0 => Quat::from_rotation_y(radians(sign * 12.0 * expression.lead)),
                    1 => Quat::from_rotation_z(radians(sign * 14.0 * expression.chest)),
                    _ => Quat::from_rotation_z(radians(sign * 18.0 * expression.wrist)),
                };
                rotations[cursor] = (node, a.rotations[i].slerp(b.rotations[i], t) * accent);
                cursor += 1;
            }
            let curl = mix(
                a.curl,
                b.curl,
                ease(progress, 0.13, (reach + 0.18).min(1.0)),
            ) + 8.0 * expression.wrist;
            for (finger, [first, middle]) in joints.fingers[side].into_iter().enumerate() {
                let angle = radians(-sign * curl * (0.76 + 0.11 * finger as f32));
                rotations[cursor] = (first, Quat::from_rotation_z(angle));
                rotations[cursor + 1] = (middle, Quat::from_rotation_z(angle * 0.55));
                cursor += 2;
            }
        }
        self.animation
            .apply_procedural(joints.hips, root, &rotations[..cursor]);
        for side in [L, R] {
            let horizontal = pose.feet[side];
            let target = Vec3::new(
                horizontal.x,
                self.ankle_y[side] + pose.lift[side],
                horizontal.y,
            );
            let rotation = Quat::from_rotation_y(radians(pose.yaw[side]))
                * Quat::from_rotation_x(radians(pose.pitch[side]));
            self.animation
                .place_foot(joints.legs[side], target, rotation);
        }
        let head_from_bind = self
            .animation
            .head_from_bind()
            .context("missing dance head joint")?;
        let pose = self.animation.skin();
        Ok(PlacedPose {
            positions: pose.positions,
            normals: pose.normals,
            joint_world: pose.joint_world,
            world_offset: Vec3::ZERO,
            head_from_bind,
        })
    }
}

fn radians(degrees: f32) -> f32 {
    degrees.to_radians()
}

fn ease(t: f32, start: f32, end: f32) -> f32 {
    let u = ((t - start) / (end - start)).clamp(0.0, 1.0);
    u * u * u * (u * (6.0 * u - 15.0) + 10.0)
}

fn mix(a: f32, b: f32, amount: f32) -> f32 {
    a + (b - a) * amount
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_foot_stays_planted_and_phrase_boundaries_match() {
        for phrase in 0..PHRASES.len() {
            for key in 0..COUNTS_PER_PHRASE {
                let count = |t| Count { phrase, key, t };
                let prior = if key == 0 {
                    Count {
                        phrase: previous(phrase, PHRASES.len()),
                        key: COUNTS_PER_PHRASE - 1,
                        t: 1.0,
                    }
                } else {
                    Count {
                        phrase,
                        key: key - 1,
                        t: 1.0,
                    }
                };
                let start = footsteps(count(0.0));
                let end = footsteps(prior);
                assert_eq!(start.feet, end.feet);
                assert_eq!(start.yaw, end.yaw);
                let moving = footsteps(count(0.45));
                assert!(moving.lift.iter().filter(|&&y| y > 0.0).count() <= 1);
                if key < STEPS_PER_PHRASE {
                    let support = 1 - PHRASES[phrase].steps[key].side;
                    assert_eq!(start.feet[support], moving.feet[support]);
                }
            }
        }
    }

    #[test]
    fn rig_support_does_not_slide_when_dsp_accents_shift_weight() {
        use lico_graph::{Node, NodeKind, Pattern};

        let graph = Graph {
            nodes: vec![
                Node {
                    kind: NodeKind::Kick,
                    inputs: vec![],
                    parameters: vec![],
                },
                Node {
                    kind: NodeKind::Output,
                    inputs: vec![0],
                    parameters: vec![],
                },
            ],
            patterns: vec![Pattern {
                node: 0,
                period_ns: 1_300_000_000,
                phase_ns: 0,
                hits: vec![0, 330_000_000, 870_000_000],
            }],
            ..Graph::default()
        };
        let mut dance = Dance::new(crate::HUMANOID_GLB, &crate::mesh::PREVIEW_MATERIALS).unwrap();
        dance.set_graph(&graph);
        let ankles = dance.joints.legs.map(|leg| leg[2]);
        let step = PHRASES[0].steps[0].side;
        let at_start = dance.sample(0.0).unwrap();
        let start = ankles.map(|node| at_start.joint_world[node].w_axis.truncate());
        let moving = dance.sample(0.17).unwrap();
        let after = ankles.map(|node| moving.joint_world[node].w_axis.truncate());

        assert!(start[1 - step].distance(after[1 - step]) < 0.0001);
        assert!(after[step].y - start[step].y > 0.010);
    }
}
