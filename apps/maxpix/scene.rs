#![no_std]
extern crate alloc;

use alloc::vec::Vec;
use trueos_picasso::cam::{Camera, Projection, Quaternion};

// CPU work and upload happen once. Each vertex is one native POINT_LIST stamp.
pub const PARTICLE_COUNT: u32 = 10_000;
pub const VERTEX_STRIDE: usize = 24;

pub fn camera() -> Camera {
    let yaw = Quaternion::from_axis_angle([0.0, 1.0, 0.0], -3.0 * core::f32::consts::FRAC_PI_4);
    let pitch = Quaternion::from_axis_angle([1.0, 0.0, 0.0], 0.615_479_7);
    Camera {
        position: [-10.0, -10.0, -10.0],
        rotation: (yaw * pitch).normalized(),
        projection: Projection::Perspective {
            yfov: core::f32::consts::FRAC_PI_3,
            znear: 0.1,
            zfar: Some(100.0),
            aspect_ratio: None,
        },
    }
}

pub fn particle_mesh(count: u32) -> (Vec<u8>, Vec<u8>) {
    let mut vertices = Vec::with_capacity(count as usize * VERTEX_STRIDE);
    let mut indices = Vec::with_capacity(count as usize * 4);
    let mut rng = 0x4d41_5850u32;
    let mut unit = || {
        rng ^= rng << 13;
        rng ^= rng >> 17;
        rng ^= rng << 5;
        (rng >> 8) as f32 / 16_777_216.0
    };
    for index in 0..count {
        // Phase, height fraction and radial factor; the second Float3 is offset.
        let row = [
            unit() * core::f32::consts::TAU,
            unit(),
            0.65 + unit() * 0.35,
            0.0,
            0.0,
            0.0,
        ];
        for value in row {
            vertices.extend_from_slice(&value.to_le_bytes());
        }
        indices.extend_from_slice(&index.to_le_bytes());
    }
    (vertices, indices)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn seeds_fit_shader_domain_and_have_one_index_per_point() {
        let (vertices, indices) = particle_mesh(PARTICLE_COUNT);
        assert_eq!(vertices.len(), PARTICLE_COUNT as usize * VERTEX_STRIDE);
        for (index, row) in vertices.chunks_exact(VERTEX_STRIDE).enumerate() {
            let values: Vec<f32> = row
                .chunks_exact(4)
                .map(|b| f32::from_le_bytes(b.try_into().unwrap()))
                .collect();
            assert!(values.iter().all(|v| v.is_finite()));
            assert!((0.0..core::f32::consts::TAU).contains(&values[0]));
            assert!((0.0..1.0).contains(&values[1]));
            assert!((0.65..=1.0).contains(&values[2]));
            assert_eq!(&values[3..], &[0.0; 3]);
            assert_eq!(
                u32::from_le_bytes(indices[index * 4..index * 4 + 4].try_into().unwrap()),
                index as u32
            );
        }
    }
    #[test]
    fn picasso_camera_faces_origin() {
        let camera = camera();
        let forward = camera.rotation.rotate([0.0, 0.0, -1.0]);
        for component in forward {
            assert!((component - 0.577_350_26).abs() < 1e-6);
        }
        assert_eq!(camera.position, [-10.0; 3]);
    }
}
