use anyhow::{Context, Result};
use glam::{Mat3, Quat, Vec3};

use super::hand_contact::HandProbe;
use crate::animation::{Animation, two_bone_positions};

pub(super) struct PointingArm {
    joints: [usize; 3],
    bone_frames: [Mat3; 2],
    lengths: [f32; 2],
    finger_offsets: Vec<(usize, Quat)>,
    body_section: usize,
    tip_vertex: usize,
    tip_in_hand: Vec3,
}

impl PointingArm {
    /// Calibrate from the upright pose with its rest finger rotations.
    pub(super) fn new(glb: &[u8], animation: &Animation, body_section: usize) -> Result<Self> {
        let asset = gltf::Gltf::from_slice(glb).context("parse heart gesture rig")?;
        let joint = |name: &str| {
            asset
                .nodes()
                .find(|node| node.name() == Some(name))
                .map(|node| node.index())
                .with_context(|| format!("missing heart gesture joint {name}"))
        };
        let joints = [
            joint("J_Bip_R_UpperArm")?,
            joint("J_Bip_R_LowerArm")?,
            joint("J_Bip_R_Hand")?,
        ];
        let [shoulder, elbow, wrist] = joints.map(|node| animation.joint_position(node));
        let bone_frames = [
            frame(
                animation
                    .joint_transform(joints[0])
                    .inverse()
                    .transform_vector3(elbow - shoulder),
                Vec3::Y,
            ),
            frame(
                animation
                    .joint_transform(joints[1])
                    .inverse()
                    .transform_vector3(wrist - elbow),
                Vec3::Y,
            ),
        ];
        let mut finger_offsets = Vec::with_capacity(15);
        for (finger, curl) in [
            ("Thumb", [22.0_f32, 35.0, 16.0]),
            ("Index", [0.0, 0.0, 0.0]),
            ("Middle", [57.0, 65.0, 26.0]),
            ("Ring", [62.0, 70.0, 29.0]),
            ("Little", [67.0, 73.0, 32.0]),
        ] {
            for (segment, bend) in curl.into_iter().enumerate() {
                let name = format!("J_Bip_R_{finger}{}", segment + 1);
                let rotation = Quat::from_rotation_z(-bend.to_radians());
                let rotation = if finger == "Thumb" && segment == 0 {
                    rotation * Quat::from_rotation_y(-22.0_f32.to_radians())
                } else {
                    rotation
                };
                finger_offsets.push((joint(&name)?, rotation));
            }
        }
        let tip_vertex = HandProbe::new(glb, 0)?.right_index_vertex();
        let tip_in_hand = animation
            .joint_transform(joints[2])
            .inverse()
            .transform_point3(animation.skin_point(body_section, tip_vertex));
        Ok(Self {
            joints,
            bone_frames,
            lengths: [shoulder.distance(elbow), elbow.distance(wrist)],
            finger_offsets,
            body_section,
            tip_vertex,
            tip_in_hand,
        })
    }

    pub(super) fn shoulder(&self, animation: &Animation) -> Vec3 {
        animation.joint_position(self.joints[0])
    }

    pub(super) fn tip(&self, animation: &Animation) -> Vec3 {
        animation.skin_point(self.body_section, self.tip_vertex)
    }

    pub(super) fn place(&self, animation: &mut Animation, target: Vec3) {
        let shoulder = self.shoulder(animation);
        let reach = target - shoulder;
        // Aim into the drawing plane and let the wrist follow the shoulder's reach.
        let aim = Vec3::new(1.3 * reach.x - 0.23, 1.7 * reach.y + 0.24, -1.0);
        let hand = frame(aim, Vec3::X + Vec3::Y);
        let wrist = target - hand * self.tip_in_hand;
        let pole = Vec3::new(0.55, -0.76, 0.10);
        let Some([elbow, wrist]) = two_bone_positions(shoulder, wrist, self.lengths, pole) else {
            return;
        };
        let upper = elbow - shoulder;
        let lower = wrist - elbow;
        let hinge = upper.cross(lower);
        for (index, direction) in [upper, lower].into_iter().enumerate() {
            let rotation = frame(direction, hinge) * self.bone_frames[index].transpose();
            animation.orient_joint(self.joints[index], Quat::from_mat3(&rotation).normalize());
        }
        animation.orient_joint(self.joints[2], Quat::from_mat3(&hand).normalize());
        animation.apply_joint_offsets(&self.finger_offsets);
    }
}

fn frame(along: Vec3, up: Vec3) -> Mat3 {
    let x = along.normalize();
    let z = x.cross(up).normalize();
    Mat3::from_cols(x, z.cross(x), z)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{HUMANOID_GLB, mesh::PREVIEW_MATERIALS};

    #[test]
    fn places_the_skinned_index_tip_within_the_heart_envelope() -> Result<()> {
        let mut pose = Animation::nod(HUMANOID_GLB, &PREVIEW_MATERIALS, None)?;
        pose.apply(0.0);
        let arm = PointingArm::new(HUMANOID_GLB, &pose, 1)?;
        let center = arm.shoulder(&pose) + Vec3::new(0.05, 0.0, -0.40);
        let tip_index = pose.bind_positions(0).len() + arm.tip_vertex;
        let asset = gltf::Gltf::from_slice(HUMANOID_GLB)?;
        let joint = |name| {
            asset
                .nodes()
                .find(|node| node.name() == Some(name))
                .unwrap()
                .index()
        };
        let fixed_joints = ["J_Bip_L_Hand", "J_Bip_R_Foot", "J_Bip_C_Head"].map(joint);
        let fixed_transforms = fixed_joints.map(|joint| pose.joint_transform(joint));
        let index = ["J_Bip_R_Index1", "J_Bip_R_Index2", "J_Bip_R_Index3"].map(joint);
        let middle = ["J_Bip_R_Middle1", "J_Bip_R_Middle2", "J_Bip_R_Middle3"].map(joint);
        for x in [-0.22, 0.0, 0.22] {
            for y in [-0.20, 0.0, 0.20] {
                let target = center + Vec3::new(x, y, 0.0);
                pose.apply(0.0);
                arm.place(&mut pose, target);
                let tip = arm.tip(&pose);
                assert!(tip.distance(target) < 0.00002, "{tip:?} vs {target:?}");
                assert_eq!(
                    fixed_joints.map(|joint| pose.joint_transform(joint)),
                    fixed_transforms
                );
                let straightness = |joints: [usize; 3]| {
                    let [a, b, c] = joints.map(|joint| pose.joint_position(joint));
                    (b - a).normalize().dot((c - b).normalize())
                };
                assert!(straightness(index) > 0.99);
                assert!(straightness(middle) < 0.5);
                assert!(
                    Vec3::from_array(pose.skin().positions[tip_index]).distance(tip) < 0.000001
                );
            }
        }
        Ok(())
    }
}
