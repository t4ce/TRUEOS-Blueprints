use anyhow::{Context, Result, ensure};
use glam::{Mat4, Quat, Vec3};
use gltf::animation::{Interpolation, Property, util::ReadOutputs};

const NOD_SECONDS: f32 = 1.40;
const NOD_KEY_COUNT: u32 = 85;
pub(super) const ARM_RECOVERY_SECONDS: f32 = 1.0;
pub(super) const SAMPLE_HZ: u32 = 45;

pub(super) struct Animation {
    duration: f32,
    nodes: Vec<Node>,
    base_pose: Vec<LocalPose>,
    local: Vec<LocalPose>,
    world: Vec<Mat4>,
    head_bind: Option<(usize, Mat4)>,
    channels: Vec<Channel>,
    sections: Vec<Section>,
    positions: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
}

pub(super) struct PoseSample<'a> {
    pub(super) positions: &'a [[f32; 3]],
    pub(super) normals: &'a [[f32; 3]],
    pub(super) joint_world: &'a [Mat4],
}

#[derive(Clone, Default)]
pub(super) struct RigPose {
    pub(super) joints: Vec<Mat4>,
    pub(super) offset: Vec3,
}

impl RigPose {
    pub(super) fn save(&mut self, joints: &[Mat4], offset: Vec3) {
        self.joints.clear();
        self.joints.extend_from_slice(joints);
        self.offset = offset;
    }
}

#[derive(PartialEq)]
struct Node {
    parent: Option<usize>,
    rest: LocalPose,
}

#[derive(Clone, Copy, PartialEq)]
struct LocalPose {
    translation: Vec3,
    rotation: Quat,
    scale: Vec3,
}

struct Channel {
    node: usize,
    times: Vec<f32>,
    values: Keyframes,
}

enum Keyframes {
    Translations(Vec<Vec3>),
    Rotations(Vec<Quat>),
}

struct Section {
    vertices: Vec<Vertex>,
    joints: Vec<Joint>,
}

#[derive(PartialEq)]
struct Vertex {
    position: Vec3,
    normal: Vec3,
    joints: [u16; 4],
    weights: [f32; 4],
}

struct Joint {
    node: usize,
    inverse_bind: Mat4,
    skin_matrix: Mat4,
}

impl Animation {
    pub(super) fn procedural(glb: &[u8], materials: &[&str]) -> Result<Self> {
        let asset = gltf::Gltf::from_slice(glb).context("parse procedural humanoid GLB")?;
        Self::load_rig(&asset, materials)
    }

    pub(super) fn apply_procedural(
        &mut self,
        root: usize,
        translation: Vec3,
        rotations: &[(usize, Quat)],
    ) {
        self.local.copy_from_slice(&self.base_pose);
        self.local[root].translation += translation;
        for &(joint, rotation) in rotations {
            self.local[joint].rotation *= rotation;
        }
        self.update_world(None);
    }

    pub(super) fn prepare(clip_glb: &[u8], avatar_glb: &[u8], materials: &[&str]) -> Result<Self> {
        let asset = gltf::Gltf::from_slice(clip_glb).context("parse animated humanoid GLB")?;
        let blob = asset
            .blob
            .as_deref()
            .context("animated GLB has no BIN chunk")?;
        let embedded = |buffer: gltf::Buffer<'_>| {
            matches!(buffer.source(), gltf::buffer::Source::Bin).then_some(blob)
        };
        let mut clips = asset.animations();
        let clip = clips.next().context("animated GLB has no Greeting clip")?;
        ensure!(
            clip.name() == Some("Greeting") && clips.next().is_none(),
            "expected exactly one Greeting clip"
        );
        let avatar = gltf::Gltf::from_slice(avatar_glb).context("parse embedded humanoid GLB")?;
        let mut animation = Self::load_rig(&avatar, materials)?;
        let clip_rig = Self::load_rig(&asset, materials)?;
        // Retargeted files can carry older positions and normals for the same skin.
        ensure!(
            animation.matches_rig(&clip_rig, |a, b| a.joints == b.joints
                && a.weights == b.weights),
            "Greeting differs from the embedded rig"
        );
        let mut properties = vec![[false; 2]; animation.nodes.len()];
        let mut duration = 0.0_f32;
        let mut channels = Vec::new();
        for channel in clip.channels() {
            ensure!(
                channel.sampler().interpolation() == Interpolation::Linear,
                "expected linear Greeting interpolation"
            );
            let reader = channel.reader(embedded);
            let times: Vec<_> = reader
                .read_inputs()
                .context("missing Greeting input times")?
                .collect();
            ensure!(
                times.first().is_some_and(|&time| time >= 0.0)
                    && times.iter().all(|time| time.is_finite())
                    && times.windows(2).all(|pair| pair[0] < pair[1]),
                "invalid Greeting input times"
            );
            duration = duration.max(*times.last().expect("checked nonempty times"));
            let target = channel.target();
            let node = target.node().index();
            let outputs = reader.read_outputs().context("missing Greeting outputs")?;
            let (property, values) = match (target.property(), outputs) {
                (Property::Translation, ReadOutputs::Translations(translations)) => {
                    let values: Vec<_> = translations.map(Vec3::from_array).collect();
                    ensure!(
                        values.len() == times.len() && values.iter().all(|value| value.is_finite()),
                        "invalid Greeting translation keys"
                    );
                    (0, Keyframes::Translations(values))
                }
                (Property::Rotation, ReadOutputs::Rotations(rotations)) => {
                    let values = rotations
                        .into_f32()
                        .map(|value| {
                            let rotation = Quat::from_array(value);
                            let length_squared = rotation.length_squared();
                            ensure!(
                                length_squared.is_finite() && length_squared > f32::EPSILON,
                                "invalid Greeting rotation key"
                            );
                            Ok(rotation.normalize())
                        })
                        .collect::<Result<Vec<_>>>()?;
                    ensure!(
                        values.len() == times.len(),
                        "invalid Greeting rotation count"
                    );
                    (1, Keyframes::Rotations(values))
                }
                _ => anyhow::bail!("expected skeletal Greeting translation or rotation"),
            };
            ensure!(
                !core::mem::replace(&mut properties[node][property], true),
                "duplicate Greeting channel"
            );
            channels.push(Channel {
                node,
                times,
                values,
            });
        }
        ensure!(duration > 0.0, "empty Greeting clip");

        animation.duration = duration;
        animation.channels = channels;
        Ok(animation)
    }

    pub(super) fn nod(glb: &[u8], materials: &[&str], greeting: Option<&mut Self>) -> Result<Self> {
        let asset = gltf::Gltf::from_slice(glb).context("parse Nod humanoid GLB")?;
        let mut animation = Self::load_rig(&asset, materials)?;
        let recovery = if let Some(greeting) = greeting {
            ensure!(
                animation.compatible_rig(greeting),
                "Greeting differs from the embedded rig"
            );
            greeting.sample_local(greeting.duration());
            animation.base_pose.copy_from_slice(&greeting.local);
            ARM_RECOVERY_SECONDS
        } else {
            0.0
        };
        animation.duration = recovery + NOD_SECONDS;

        // +X is the model's right, Y is up, and the avatar faces -Z.
        // Let the shoulders and upper arms lead, with the wrists settling last.
        let arm_progress: Vec<f32> = if recovery > 0.0 {
            (0..NOD_KEY_COUNT)
                .map(|key| key as f32 / (NOD_KEY_COUNT - 1) as f32)
                .collect()
        } else {
            vec![1.0]
        };
        let arm_times: Vec<f32> = arm_progress.iter().map(|t| t * recovery).collect();
        for (name, offset, start, end) in [
            ("J_Bip_L_Shoulder", Quat::IDENTITY, 0.0, 0.82),
            ("J_Bip_R_Shoulder", Quat::IDENTITY, 0.0, 0.82),
            (
                "J_Bip_L_UpperArm",
                Quat::from_rotation_z(74.0_f32.to_radians()),
                0.0,
                0.86,
            ),
            (
                "J_Bip_R_UpperArm",
                Quat::from_rotation_z(-74.0_f32.to_radians()),
                0.0,
                0.86,
            ),
            (
                "J_Bip_L_LowerArm",
                Quat::from_rotation_y(-12.0_f32.to_radians()),
                0.06,
                0.94,
            ),
            (
                "J_Bip_R_LowerArm",
                Quat::from_rotation_y(12.0_f32.to_radians()),
                0.06,
                0.94,
            ),
            ("J_Bip_L_Hand", Quat::IDENTITY, 0.14, 1.0),
            ("J_Bip_R_Hand", Quat::IDENTITY, 0.14, 1.0),
        ] {
            let node = joint_named(&asset, name)?;
            let from = animation.base_pose[node].rotation;
            let relaxed = animation.nodes[node].rest.rotation * offset;
            let values = arm_progress
                .iter()
                .map(|&progress| {
                    from.slerp(relaxed, smoothstep((progress - start) / (end - start)))
                })
                .collect();
            animation.channels.push(Channel {
                node,
                times: arm_times.clone(),
                values: Keyframes::Rotations(values),
            });
        }

        let times: Vec<f32> = (0..NOD_KEY_COUNT)
            .map(|key| recovery + NOD_SECONDS * (key as f32 / (NOD_KEY_COUNT - 1) as f32))
            .collect();
        // The head leads the movement. Give the neck and torso smaller
        // rotations so the dip doesn't look like a head turning on a hinge.
        for (name, degrees, lag) in [
            ("J_Bip_C_Head", -15.0_f32, 0.0),
            ("J_Bip_C_Neck", -6.0_f32, 0.03),
            ("J_Bip_C_Chest", -2.0_f32, 0.06),
            ("J_Bip_C_Spine", -1.0_f32, 0.09),
        ] {
            let node = joint_named(&asset, name)?;
            let neutral = animation.base_pose[node].rotation;
            let values = times
                .iter()
                .map(|&time| {
                    let amount = nod_amount(time - recovery - lag);
                    neutral * Quat::from_rotation_x(degrees.to_radians() * amount)
                })
                .collect();
            animation.channels.push(Channel {
                node,
                times: times.clone(),
                values: Keyframes::Rotations(values),
            });
        }
        Ok(animation)
    }

    fn load_rig(asset: &gltf::Gltf, materials: &[&str]) -> Result<Self> {
        let blob = asset
            .blob
            .as_deref()
            .context("humanoid GLB has no BIN chunk")?;
        let embedded = |buffer: gltf::Buffer<'_>| {
            matches!(buffer.source(), gltf::buffer::Source::Bin).then_some(blob)
        };
        let mut nodes = asset
            .nodes()
            .map(|node| {
                let (translation, rotation, scale) = node.transform().decomposed();
                let translation = Vec3::from_array(translation);
                let rotation = Quat::from_array(rotation);
                let scale = Vec3::from_array(scale);
                let length_squared = rotation.length_squared();
                ensure!(
                    translation.is_finite()
                        && length_squared.is_finite()
                        && length_squared > f32::EPSILON
                        && scale.is_finite(),
                    "invalid humanoid rest transform"
                );
                Ok(Node {
                    parent: None,
                    rest: LocalPose {
                        translation,
                        rotation: rotation.normalize(),
                        scale,
                    },
                })
            })
            .collect::<Result<Vec<_>>>()?;
        for parent in asset.nodes() {
            for child in parent.children() {
                // The baked asset preserves AvatarSample_B's parent-first node order.
                let parent_index = parent.index();
                ensure!(
                    parent_index < child.index()
                        && nodes[child.index()].parent.replace(parent_index).is_none(),
                    "invalid humanoid node hierarchy"
                );
            }
        }

        let mut sections = Vec::with_capacity(materials.len());
        for &name in materials {
            let (node, primitive) = asset
                .nodes()
                .find_map(|node| {
                    let primitive = node
                        .mesh()?
                        .primitives()
                        .find(|primitive| primitive.material().name() == Some(name))?;
                    Some((node, primitive))
                })
                .with_context(|| format!("missing skinned preview material {name}"))?;
            ensure!(
                primitive.mode() == gltf::mesh::Mode::Triangles,
                "expected skin triangles"
            );
            let skin = node.skin().context("preview mesh has no skin")?;
            let joint_count = skin.joints().len();
            let inverse_binds = skin
                .reader(embedded)
                .read_inverse_bind_matrices()
                .context("humanoid skin has no inverse bind matrices")?;
            ensure!(
                joint_count != 0 && inverse_binds.len() == joint_count,
                "humanoid inverse bind count mismatches its skin"
            );
            let joints = skin
                .joints()
                .zip(inverse_binds)
                .map(|(joint, matrix)| {
                    let inverse_bind = Mat4::from_cols_array_2d(&matrix);
                    ensure!(inverse_bind.is_finite(), "nonfinite inverse bind matrix");
                    Ok(Joint {
                        node: joint.index(),
                        inverse_bind,
                        skin_matrix: Mat4::IDENTITY,
                    })
                })
                .collect::<Result<Vec<_>>>()?;

            let reader = primitive.reader(embedded);
            let positions = reader.read_positions().context("missing skin positions")?;
            let normals = reader.read_normals().context("missing skin normals")?;
            let joint_indices = reader
                .read_joints(0)
                .context("missing skin joints")?
                .into_u16();
            let weights = reader
                .read_weights(0)
                .context("missing skin weights")?
                .into_f32();
            ensure!(
                positions.len() != 0
                    && positions.len() == normals.len()
                    && positions.len() == joint_indices.len()
                    && positions.len() == weights.len(),
                "inconsistent humanoid skin vertex attributes"
            );
            let vertices = positions
                .zip(normals)
                .zip(joint_indices)
                .zip(weights)
                .map(|(((position, normal), joints), weights)| {
                    let position = Vec3::from_array(position);
                    let normal = Vec3::from_array(normal);
                    let normal_length_squared = normal.length_squared();
                    let total: f32 = weights.iter().sum();
                    ensure!(
                        position.is_finite()
                            && normal_length_squared.is_finite()
                            && normal_length_squared > f32::EPSILON
                            && joints.iter().all(|&joint| usize::from(joint) < joint_count)
                            && weights
                                .iter()
                                .all(|&weight| weight.is_finite() && weight >= 0.0)
                            && total.is_finite()
                            && total > f32::EPSILON,
                        "invalid humanoid skin vertex"
                    );
                    Ok(Vertex {
                        position,
                        normal,
                        joints,
                        weights: weights.map(|weight| weight / total),
                    })
                })
                .collect::<Result<Vec<_>>>()?;
            sections.push(Section { vertices, joints });
        }

        let head_bind = asset
            .nodes()
            .find(|node| node.name() == Some("J_Bip_C_Head"))
            .and_then(|head| {
                let head = head.index();
                sections
                    .iter()
                    .flat_map(|part| &part.joints)
                    .find(|joint| joint.node == head)
                    .map(|joint| (head, joint.inverse_bind))
            });
        let base_pose: Vec<_> = nodes.iter().map(|node| node.rest).collect();
        let world = vec![Mat4::IDENTITY; nodes.len()];
        let vertex_count = sections.iter().map(|part| part.vertices.len()).sum();
        Ok(Self {
            duration: 0.0,
            nodes,
            local: base_pose.clone(),
            base_pose,
            world,
            head_bind,
            channels: Vec::new(),
            sections,
            positions: vec![[0.0; 3]; vertex_count],
            normals: vec![[0.0; 3]; vertex_count],
        })
    }

    pub(super) fn duration(&self) -> f32 {
        self.duration
    }

    pub(super) fn compatible_rig(&self, other: &Self) -> bool {
        self.matches_rig(other, Vertex::eq)
    }

    fn matches_rig(&self, other: &Self, vertices_match: impl Fn(&Vertex, &Vertex) -> bool) -> bool {
        self.nodes == other.nodes
            && self.sections.len() == other.sections.len()
            && self
                .sections
                .iter()
                .zip(&other.sections)
                .all(|(left, right)| {
                    left.vertices.len() == right.vertices.len()
                        && left
                            .vertices
                            .iter()
                            .zip(&right.vertices)
                            .all(|(a, b)| vertices_match(a, b))
                        && left.joints.len() == right.joints.len()
                        && left
                            .joints
                            .iter()
                            .zip(&right.joints)
                            .all(|(a, b)| a.node == b.node && a.inverse_bind == b.inverse_bind)
                })
    }

    pub(super) fn apply(&mut self, elapsed_seconds: f32) {
        self.sample_local(elapsed_seconds);
        self.update_world(None);
    }

    pub(super) fn blend_pose(
        &mut self,
        from: &RigPose,
        to_joints: &[Mat4],
        to_offset: Vec3,
        weight: f32,
    ) {
        for (index, node) in self.nodes.iter().enumerate() {
            let local = |joints: &[Mat4], offset: Vec3| {
                node.parent.map_or_else(
                    || Mat4::from_translation(offset) * joints[index],
                    |parent| joints[parent].inverse() * joints[index],
                )
            };
            let (from_scale, from_rotation, from_translation) =
                local(&from.joints, from.offset).to_scale_rotation_translation();
            let (to_scale, to_rotation, to_translation) =
                local(to_joints, to_offset).to_scale_rotation_translation();
            self.local[index] = LocalPose {
                translation: from_translation.lerp(to_translation, weight),
                rotation: from_rotation
                    .normalize()
                    .slerp(to_rotation.normalize(), weight),
                scale: from_scale.lerp(to_scale, weight),
            };
        }
        self.update_world(None);
    }

    #[cfg(any(target_os = "trueos", test))]
    pub(super) fn bind_positions(
        &self,
        section: usize,
    ) -> impl ExactSizeIterator<Item = Vec3> + '_ {
        self.sections[section]
            .vertices
            .iter()
            .map(|vertex| vertex.position)
    }

    pub(super) fn apply_bind_normals(
        &mut self,
        section: usize,
        normals: &[[f32; 3]],
    ) -> Result<()> {
        let part = self
            .sections
            .get_mut(section)
            .context("missing normal map mesh section")?;
        ensure!(
            part.vertices.len() == normals.len(),
            "normal map vertex count changed"
        );
        for (vertex, &normal) in part.vertices.iter_mut().zip(normals) {
            vertex.normal = Vec3::from_array(normal);
        }
        Ok(())
    }

    pub(super) fn head_from_bind(&self) -> Option<Mat4> {
        self.head_bind.map(|(head, bind)| self.world[head] * bind)
    }

    pub(super) fn joint_position(&self, node: usize) -> Vec3 {
        self.world[node].w_axis.truncate()
    }

    pub(super) fn joint_transform(&self, node: usize) -> Mat4 {
        self.world[node]
    }

    pub(super) fn orient_joint(&mut self, joint: usize, rotation: Quat) {
        self.place_joint(joint, self.joint_position(joint), rotation);
    }

    pub(super) fn place_joint(&mut self, joint: usize, position: Vec3, rotation: Quat) {
        let world = self.world[joint];
        let (scale, _, _) = world.to_scale_rotation_translation();
        let target = Mat4::from_scale_rotation_translation(scale, rotation, position);
        self.transform_joint_tree(joint, target * world.inverse());
    }

    /// Pose a subtree from its rest rotations in parent-first order.
    pub(super) fn apply_joint_offsets(&mut self, offsets: &[(usize, Quat)]) {
        for &(index, offset) in offsets {
            let node = &self.nodes[index];
            let pose = node.rest;
            let local = Mat4::from_scale_rotation_translation(
                pose.scale,
                pose.rotation * offset,
                pose.translation,
            );
            let target = node
                .parent
                .map_or(local, |parent| self.world[parent] * local);
            self.transform_joint_tree(index, target * self.world[index].inverse());
        }
    }

    pub(super) fn skin_point(&self, section: usize, index: usize) -> Vec3 {
        let part = &self.sections[section];
        let vertex = &part.vertices[index];
        vertex
            .joints
            .iter()
            .zip(vertex.weights)
            .filter(|(_, weight)| *weight > 0.0)
            .map(|(&joint, weight)| {
                let joint = &part.joints[usize::from(joint)];
                (self.world[joint.node] * joint.inverse_bind).transform_point3(vertex.position)
                    * weight
            })
            .sum()
    }

    /// Set one foot's world-space placement with a two-bone leg solve. The
    /// authored pose provides the leg lengths and the side of the knee bend.
    pub(super) fn place_foot(&mut self, joints: [usize; 3], target: Vec3, rotation: Quat) {
        let [thigh, shin, foot] = joints;
        let [hip, knee, ankle] = joints.map(|joint| self.joint_position(joint));
        let upper = knee - hip;
        let lower = ankle - knee;
        let upper_len = upper.length();
        let lower_len = lower.length();
        let forward = (ankle - hip).normalize();
        let bend = upper - forward * upper.dot(forward);
        let Some([new_knee, new_ankle]) =
            two_bone_positions(hip, target, [upper_len, lower_len], bend)
        else {
            return;
        };

        let rotate_upper = Quat::from_rotation_arc(upper.normalize(), (new_knee - hip).normalize());
        self.transform_joint_tree(thigh, around(hip, rotate_upper));
        let rotate_lower = Quat::from_rotation_arc(
            (rotate_upper * lower).normalize(),
            (new_ankle - new_knee).normalize(),
        );
        self.transform_joint_tree(shin, around(new_knee, rotate_lower));
        self.place_joint(foot, new_ankle, rotation);
    }

    fn transform_joint_tree(&mut self, root: usize, transform: Mat4) {
        for index in root..self.nodes.len() {
            let mut node = Some(index);
            while let Some(child) = node.filter(|&child| child > root) {
                node = self.nodes[child].parent;
            }
            if node == Some(root) {
                self.world[index] = transform * self.world[index];
            }
        }
    }

    #[cfg(test)]
    pub(super) fn sample_with_normals(&mut self, elapsed_seconds: f32) -> PoseSample<'_> {
        self.apply(elapsed_seconds);
        self.skin()
    }

    /// Blend joint-local transforms before skinning. Check rig compatibility once
    /// before baking a sequence between the two actions.
    pub(super) fn apply_blended(
        &mut self,
        elapsed_seconds: f32,
        other: &mut Self,
        other_seconds: f32,
        weight: f32,
    ) {
        self.sample_local(elapsed_seconds);
        other.sample_local(other_seconds);
        let weight = if weight.is_nan() {
            0.0
        } else {
            weight.clamp(0.0, 1.0)
        };
        self.update_world(Some((&other.local, weight)));
    }

    #[cfg(test)]
    fn sample_blended_with_normals(
        &mut self,
        elapsed_seconds: f32,
        other: &mut Self,
        other_seconds: f32,
        weight: f32,
    ) -> PoseSample<'_> {
        self.apply_blended(elapsed_seconds, other, other_seconds, weight);
        self.skin()
    }

    fn sample_local(&mut self, elapsed_seconds: f32) {
        let time = if elapsed_seconds.is_nan() {
            0.0
        } else {
            elapsed_seconds.clamp(0.0, self.duration)
        };
        self.local.copy_from_slice(&self.base_pose);
        for channel in &self.channels {
            let (from, to, blend) = keyframe_interval(&channel.times, time);
            let pose = &mut self.local[channel.node];
            match &channel.values {
                Keyframes::Translations(values) => {
                    pose.translation = values[from].lerp(values[to], blend);
                }
                Keyframes::Rotations(values) => {
                    pose.rotation = values[from].slerp(values[to], blend);
                }
            }
        }
    }

    fn update_world(&mut self, blend: Option<(&[LocalPose], f32)>) {
        for (index, (node, &pose)) in self.nodes.iter().zip(&self.local).enumerate() {
            let pose = match blend {
                None | Some((_, 0.0)) => pose,
                Some((other, 1.0)) => other[index],
                Some((other, weight)) => {
                    let other = other[index];
                    LocalPose {
                        translation: pose.translation.lerp(other.translation, weight),
                        rotation: pose.rotation.slerp(other.rotation, weight),
                        scale: pose.scale.lerp(other.scale, weight),
                    }
                }
            };
            let local =
                Mat4::from_scale_rotation_translation(pose.scale, pose.rotation, pose.translation);
            self.world[index] = node
                .parent
                .map_or(local, |parent| self.world[parent] * local);
        }
    }

    pub(super) fn skin_from_world(&mut self, world: &[Mat4]) -> Result<PoseSample<'_>> {
        ensure!(
            self.world.len() == world.len(),
            "posed rig node count changed"
        );
        self.world.copy_from_slice(world);
        Ok(self.skin())
    }

    pub(super) fn skin(&mut self) -> PoseSample<'_> {
        let mut destination = self.positions.iter_mut().zip(&mut self.normals);
        for part in &mut self.sections {
            for joint in &mut part.joints {
                // This maps bind-space vertices straight into world space.
                joint.skin_matrix = self.world[joint.node] * joint.inverse_bind;
            }
            for vertex in &part.vertices {
                let mut position = Vec3::ZERO;
                let mut normal = Vec3::ZERO;
                for (joint, weight) in vertex.joints.into_iter().zip(vertex.weights) {
                    if weight > 0.0 {
                        let matrix = part.joints[usize::from(joint)].skin_matrix;
                        position += matrix.transform_point3(vertex.position) * weight;
                        // AvatarSample_B's joints and inverse binds are rigid transforms.
                        normal += matrix.transform_vector3(vertex.normal) * weight;
                    }
                }
                let (skinned_position, skinned_normal) =
                    destination.next().expect("sized in prepare");
                *skinned_position = position.to_array();
                *skinned_normal = normal
                    .try_normalize()
                    .unwrap_or_else(|| vertex.normal.normalize())
                    .to_array();
            }
        }
        PoseSample {
            positions: &self.positions,
            normals: &self.normals,
            joint_world: &self.world,
        }
    }
}

pub(super) fn two_bone_positions(
    root: Vec3,
    target: Vec3,
    [upper, lower]: [f32; 2],
    bend: Vec3,
) -> Option<[Vec3; 2]> {
    let axis = (target - root).try_normalize()?;
    let distance = target
        .distance(root)
        .clamp((upper - lower).abs() + 1e-5, upper + lower - 1e-5);
    let pole = (bend - axis * bend.dot(axis))
        .try_normalize()
        .unwrap_or_else(|| axis.any_orthonormal_vector());
    let along = (upper * upper - lower * lower + distance * distance) / (2.0 * distance);
    let height = (upper * upper - along * along).max(0.0).sqrt();
    Some([root + axis * along + pole * height, root + axis * distance])
}

fn around(point: Vec3, rotation: Quat) -> Mat4 {
    Mat4::from_rotation_translation(rotation, point - rotation * point)
}

fn joint_named(asset: &gltf::Gltf, name: &str) -> Result<usize> {
    asset
        .nodes()
        .find(|node| node.name() == Some(name))
        .map(|node| node.index())
        .with_context(|| format!("missing Nod joint {name}"))
}

fn nod_amount(time: f32) -> f32 {
    smoothstep((time - 0.14) / 0.42) - smoothstep((time - 0.66) / 0.48)
}

fn smoothstep(value: f32) -> f32 {
    let t = value.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn keyframe_interval(times: &[f32], time: f32) -> (usize, usize, f32) {
    let next = times.partition_point(|&key| key <= time);
    if next == 0 || next == times.len() {
        let key = next.saturating_sub(1);
        return (key, key, 0.0);
    }
    let previous = next - 1;
    let blend = (time - times[previous]) / (times[next] - times[previous]);
    (previous, next, blend)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mesh::PREVIEW_MATERIALS;

    #[test]
    fn static_avatar_does_not_supply_a_greeting_clip() {
        let avatar = crate::HUMANOID_GLB;
        assert!(Animation::prepare(avatar, avatar, &PREVIEW_MATERIALS).is_err());
    }

    #[test]
    fn greeting_uses_canonical_geometry_and_rejects_a_changed_bind() {
        use gltf::{Semantic, binary::Glb};
        use serde_json::{Value, json};

        let avatar_bytes = crate::HUMANOID_GLB;
        let avatar = gltf::Gltf::from_slice(avatar_bytes).unwrap();
        let materials = [crate::mesh::BODY_MATERIAL];
        let (body, primitive) = avatar
            .nodes()
            .find_map(|node| {
                let primitive = node
                    .mesh()?
                    .primitives()
                    .find(|part| part.material().name() == Some(crate::mesh::BODY_MATERIAL))?;
                Some((node, primitive))
            })
            .unwrap();
        let offset =
            |accessor: gltf::Accessor<'_>| accessor.view().unwrap().offset() + accessor.offset();
        let hips = joint_named(&avatar, "J_Bip_C_Hips").unwrap();
        let canonical = Animation::load_rig(&avatar, &materials).unwrap();
        let hip_start = canonical.nodes[hips].rest.translation;
        let hip_end = hip_start + Vec3::X * 0.1;

        let mut clip = Glb::from_slice(avatar_bytes).unwrap();
        let mut document: Value = serde_json::from_slice(&clip.json).unwrap();
        let bin = clip.bin.as_mut().unwrap().to_mut();
        let old_x = offset(primitive.get(&Semantic::Positions).unwrap());
        let x = f32::from_le_bytes(bin[old_x..old_x + 4].try_into().unwrap());
        bin[old_x..old_x + 4].copy_from_slice(&(x + 0.005).to_le_bytes());

        let view = document["bufferViews"].as_array().unwrap().len();
        let times = document["accessors"].as_array().unwrap().len();
        let motion_start = bin.len();
        bin.extend(
            [0.0_f32, 1.0]
                .into_iter()
                .chain(hip_start.to_array())
                .chain(hip_end.to_array())
                .flat_map(f32::to_le_bytes),
        );
        document["buffers"][0]["byteLength"] = json!(bin.len());
        document["bufferViews"].as_array_mut().unwrap().push(json!({
            "buffer": 0, "byteOffset": motion_start, "byteLength": bin.len() - motion_start
        }));
        document["accessors"].as_array_mut().unwrap().extend([
            json!({"bufferView": view, "componentType": 5126, "count": 2, "type": "SCALAR", "min": [0.0], "max": [1.0]}),
            json!({"bufferView": view, "byteOffset": 8, "componentType": 5126, "count": 2, "type": "VEC3"}),
        ]);
        document["animations"] = json!([{
            "name": "Greeting",
            "samplers": [{"input": times, "output": times + 1}],
            "channels": [{"sampler": 0, "target": {"node": hips, "path": "translation"}}]
        }]);
        clip.json = serde_json::to_vec(&document).unwrap().into();

        let clip_bytes = clip.to_vec().unwrap();
        let mut greeting = Animation::prepare(&clip_bytes, avatar_bytes, &materials).unwrap();
        assert!(greeting.compatible_rig(&canonical));
        greeting.apply(1.0);
        assert_eq!(greeting.local[hips].translation, hip_end);

        let wrong_bind = offset(body.skin().unwrap().inverse_bind_matrices().unwrap()) + 12 * 4;
        clip.bin.as_mut().unwrap().to_mut()[wrong_bind..wrong_bind + 4]
            .copy_from_slice(&10.0_f32.to_le_bytes());
        let clip_bytes = clip.to_vec().unwrap();
        assert!(matches!(
            Animation::prepare(&clip_bytes, avatar_bytes, &materials),
            Err(error) if error.to_string() == "Greeting differs from the embedded rig"
        ));
    }

    #[test]
    fn built_in_nod_moves_the_head_and_hair_and_returns_to_its_relaxed_pose() {
        let avatar = gltf::Gltf::from_slice(crate::HUMANOID_GLB).unwrap();
        let head = joint_named(&avatar, "J_Bip_C_Head").unwrap();
        let hips = joint_named(&avatar, "J_Bip_C_Hips").unwrap();
        let mut animation = Animation::nod(crate::HUMANOID_GLB, &PREVIEW_MATERIALS, None).unwrap();
        let neutral = animation.sample_with_normals(0.0).positions.to_vec();
        let neutral_attachment = animation.head_from_bind().unwrap();
        let torso = animation.world[hips].transform_point3(Vec3::ZERO);
        let neutral_head = animation.world[head].transform_vector3(Vec3::NEG_Z);
        for (hand, arm) in [
            ("J_Bip_L_Hand", "J_Bip_L_UpperArm"),
            ("J_Bip_R_Hand", "J_Bip_R_UpperArm"),
        ] {
            let hand = joint_named(&avatar, hand).unwrap();
            let arm = joint_named(&avatar, arm).unwrap();
            let hand = animation.world[hand].transform_point3(Vec3::ZERO);
            let arm = animation.world[arm].transform_point3(Vec3::ZERO);
            assert!(hand.y < arm.y - 0.4);
        }

        let body_start = 4_171;
        let hair_start = body_start + 8_535;
        assert_eq!(neutral.len(), hair_start + 9_263);
        let dip = animation.sample_with_normals(0.64);
        assert!(
            dip.positions
                .iter()
                .flatten()
                .all(|value| value.is_finite())
        );
        assert!(
            dip.positions[hair_start..]
                .iter()
                .zip(&neutral[hair_start..])
                .any(
                    |(&dipped, &rest)| Vec3::from_array(dipped).distance(Vec3::from_array(rest))
                        > 0.03
                )
        );
        let mut body = Animation::nod(crate::HUMANOID_GLB, &PREVIEW_MATERIALS[1..2], None).unwrap();
        let body_dip = body.sample_with_normals(0.64);
        assert_eq!(body_dip.positions, &dip.positions[body_start..hair_start]);
        assert_eq!(body_dip.normals, &dip.normals[body_start..hair_start]);
        assert_eq!(animation.world[hips].transform_point3(Vec3::ZERO), torso);
        let dipped_head = animation.world[head].transform_vector3(Vec3::NEG_Z);
        assert!(dipped_head.y < neutral_head.y - 0.3);
        assert_ne!(animation.head_from_bind().unwrap(), neutral_attachment);
        assert_eq!(
            animation
                .sample_with_normals(animation.duration())
                .positions,
            neutral.as_slice()
        );
        assert_eq!(animation.head_from_bind().unwrap(), neutral_attachment);
    }

    #[test]
    fn greeting_recovery_keeps_the_landing_stance_and_lowers_the_arms() {
        let glb = crate::HUMANOID_GLB;
        let avatar = gltf::Gltf::from_slice(glb).unwrap();
        let materials = [crate::mesh::BODY_MATERIAL];
        let mut greeting = Animation::load_rig(&avatar, &materials).unwrap();
        let hips = joint_named(&avatar, "J_Bip_C_Hips").unwrap();
        let feet = ["J_Bip_L_Foot", "J_Bip_R_Foot"].map(|name| joint_named(&avatar, name).unwrap());
        let elbow = joint_named(&avatar, "J_Bip_R_LowerArm").unwrap();
        let wrist = joint_named(&avatar, "J_Bip_R_Hand").unwrap();
        let rest = greeting.nodes[hips].rest.translation;
        greeting.duration = 1.0;
        greeting.channels.push(Channel {
            node: hips,
            times: vec![0.0, greeting.duration()],
            values: Keyframes::Translations(vec![rest, rest + Vec3::new(0.1, -0.06, -0.3)]),
        });
        greeting.channels.push(Channel {
            node: wrist,
            times: vec![0.0],
            values: Keyframes::Rotations(vec![Quat::from_rotation_x(0.7)]),
        });
        let landing_mesh = greeting
            .sample_with_normals(greeting.duration())
            .positions
            .to_vec();
        let landing_stance = [hips, feet[0], feet[1]].map(|node| greeting.world[node]);
        let landing_elbow = greeting.local[elbow].rotation;
        let landing_wrist = greeting.local[wrist].rotation;

        let mut nod = Animation::nod(glb, &materials, Some(&mut greeting)).unwrap();
        let recovered_start = nod.sample_with_normals(0.0);
        assert!(
            recovered_start
                .positions
                .iter()
                .zip(&landing_mesh)
                .all(|(&a, &b)| { Vec3::from_array(a).abs_diff_eq(Vec3::from_array(b), 1.0e-6) })
        );
        nod.sample_local(0.12);
        assert!(nod.local[elbow].rotation.angle_between(landing_elbow) > 0.001);
        assert!(nod.local[wrist].rotation.abs_diff_eq(landing_wrist, 1.0e-6));
        nod.sample_local(ARM_RECOVERY_SECONDS);
        assert!(
            nod.local[wrist]
                .rotation
                .dot(nod.nodes[wrist].rest.rotation)
                .abs()
                > 0.99999
        );
        for sample in 0..=100 {
            nod.sample_local(nod.duration() * sample as f32 / 100.0);
            nod.update_world(None);
            assert_eq!(
                [hips, feet[0], feet[1]].map(|node| nod.world[node]),
                landing_stance
            );
        }
    }

    fn two_joint_animation() -> Animation {
        let start = LocalPose {
            translation: Vec3::ZERO,
            rotation: Quat::IDENTITY,
            scale: Vec3::ONE,
        };
        let hinged = LocalPose {
            translation: Vec3::Y,
            ..start
        };
        Animation {
            duration: 2.0,
            nodes: vec![
                Node {
                    parent: None,
                    rest: start,
                },
                Node {
                    parent: Some(0),
                    rest: hinged,
                },
            ],
            base_pose: vec![start, hinged],
            local: vec![start, hinged],
            world: vec![Mat4::IDENTITY; 2],
            head_bind: None,
            channels: vec![
                Channel {
                    node: 0,
                    times: vec![0.0, 2.0],
                    values: Keyframes::Translations(vec![Vec3::ZERO, Vec3::X * 2.0]),
                },
                Channel {
                    node: 1,
                    times: vec![0.0, 2.0],
                    values: Keyframes::Rotations(vec![
                        Quat::IDENTITY,
                        Quat::from_rotation_z(core::f32::consts::FRAC_PI_2),
                    ]),
                },
            ],
            sections: vec![Section {
                vertices: vec![Vertex {
                    position: Vec3::X + Vec3::Y,
                    normal: Vec3::X,
                    joints: [0, 1, 0, 0],
                    weights: [0.25, 0.75, 0.0, 0.0],
                }],
                joints: vec![
                    Joint {
                        node: 0,
                        inverse_bind: Mat4::IDENTITY,
                        skin_matrix: Mat4::IDENTITY,
                    },
                    Joint {
                        node: 1,
                        inverse_bind: Mat4::from_translation(-Vec3::Y),
                        skin_matrix: Mat4::IDENTITY,
                    },
                ],
            }],
            positions: vec![[0.0; 3]],
            normals: vec![[0.0; 3]],
        }
    }

    #[test]
    fn sample_interpolates_local_keys_before_blending_world_joint_positions() {
        let mut animation = two_joint_animation();
        assert_eq!(animation.duration(), 2.0);
        assert_eq!(
            animation.sample_with_normals(-1.0).positions,
            &[[1.0, 1.0, 0.0]]
        );
        let actual = Vec3::from_array(animation.sample_with_normals(1.0).positions[0]);
        let amount = 0.75 * core::f32::consts::FRAC_1_SQRT_2;
        let expected = Vec3::new(1.25 + amount, 1.0 + amount, 0.0);
        assert!(actual.abs_diff_eq(expected, 1.0e-5));
        let final_position = Vec3::from_array(animation.sample_with_normals(3.0).positions[0]);
        assert!(final_position.abs_diff_eq(Vec3::new(2.25, 1.75, 0.0), 1.0e-5));
        assert_eq!(
            animation.sample_with_normals(0.0).positions,
            &[[1.0, 1.0, 0.0]]
        );
    }

    #[test]
    fn skins_normals_as_weighted_directions_and_preserves_authored_splits() {
        let mut animation = two_joint_animation();
        let vertex = &animation.sections[0].vertices[0];
        let split = Vertex {
            position: vertex.position,
            normal: Vec3::Z,
            joints: vertex.joints,
            weights: vertex.weights,
        };
        animation.sections[0].vertices.push(split);
        animation.positions.push([0.0; 3]);
        animation.normals.push([0.0; 3]);

        let rest = animation.sample_with_normals(0.0);
        assert_eq!(rest.normals, &[Vec3::X.to_array(), Vec3::Z.to_array()]);

        let posed = animation.sample_with_normals(2.0);
        assert_eq!(posed.positions[0], posed.positions[1]);
        let expected = Vec3::new(0.25, 0.75, 0.0).normalize();
        assert!(Vec3::from_array(posed.normals[0]).abs_diff_eq(expected, 1.0e-5));
        assert!(Vec3::from_array(posed.normals[1]).abs_diff_eq(Vec3::Z, 1.0e-5));
    }

    #[test]
    fn bind_normal_detail_preserves_the_animated_positions() {
        let mut animation = two_joint_animation();
        let reference = animation.sample_with_normals(1.0).positions.to_vec();
        assert!(animation.apply_bind_normals(0, &[]).is_err());
        animation
            .apply_bind_normals(0, &[Vec3::Z.to_array()])
            .unwrap();
        let detailed = animation.sample_with_normals(1.0);
        assert_eq!(detailed.positions, reference);
        assert_eq!(detailed.normals, &[Vec3::Z.to_array()]);
    }

    #[test]
    fn crossfade_matches_both_endpoints_without_polluting_either_action() {
        let mut first = two_joint_animation();
        let mut second = two_joint_animation();
        assert!(first.compatible_rig(&second));
        let owned = |sample: PoseSample<'_>| (sample.positions.to_vec(), sample.normals.to_vec());
        let start = owned(first.sample_with_normals(0.0));
        let middle = owned(first.sample_with_normals(1.0));
        let end = owned(second.sample_with_normals(2.0));
        assert_eq!(
            owned(first.sample_blended_with_normals(0.0, &mut second, 2.0, 0.0)),
            start
        );
        assert_eq!(
            owned(first.sample_blended_with_normals(0.0, &mut second, 2.0, 0.5)),
            middle
        );
        assert_eq!(
            owned(first.sample_blended_with_normals(0.0, &mut second, 2.0, 1.0)),
            end
        );
        assert_eq!(owned(first.sample_with_normals(0.0)), start);
        assert_eq!(owned(second.sample_with_normals(2.0)), end);
        assert!(first.compatible_rig(&second));
        second.sections[0].vertices[0].normal = Vec3::Y;
        assert!(!first.compatible_rig(&second));
        second.sections[0].vertices[0].normal = Vec3::X;
        assert!(first.compatible_rig(&second));
        second.sections[0].joints[1].inverse_bind = Mat4::IDENTITY;
        assert!(!first.compatible_rig(&second));
    }
}
