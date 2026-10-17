#![no_std]
extern crate alloc;

use alloc::vec::Vec;
use trueos_picasso::cam::{Camera, Projection, Quaternion};

// CPU work and upload happen once. Each vertex is one native POINT_LIST stamp.
pub const PARTICLE_COUNT: u32 = 230_400;
pub const VERTEX_STRIDE: usize = 24;

/// Native SF PointWidth presets in U8.3 units (one unit = 1/8 pixel).
/// Covers fractional widths and the complete field's largest nonzero value.
pub const POINT_WIDTHS_RAW: [u32; 12] = [8, 16, 32, 64, 128, 256, 512, 1024, 2047, 1, 2, 4];

pub fn next_point_width_raw(current: u32) -> u32 {
    let index = POINT_WIDTHS_RAW
        .iter()
        .position(|&raw| raw == current)
        .unwrap_or(0);
    POINT_WIDTHS_RAW[(index + 1) % POINT_WIDTHS_RAW.len()]
}

/// Four independent pointer channels. Source identity is assigned by the app.
pub const POINTER_COUNT: usize = 4;

#[derive(Clone, Copy, Debug)]
pub struct PointerInput {
    pub active: bool,
    pub position: [f32; 2],
    pub phase: f32,
    pub buttons: u32,
    pub pressed: u32,
    pub released: u32,
    pub wheel: i32,
}

impl Default for PointerInput {
    fn default() -> Self {
        Self {
            active: false,
            position: [0.5; 2],
            phase: 0.0,
            buttons: 0,
            pressed: 0,
            released: 0,
            wheel: 0,
        }
    }
}

impl PointerInput {
    pub fn report(
        &mut self,
        local: [i32; 2],
        extent: [u32; 2],
        buttons: u32,
        pressed: u32,
        released: u32,
        wheel: i16,
    ) {
        self.active = true;
        for axis in 0..2 {
            self.position[axis] = (local[axis] as f32 / extent[axis].max(1) as f32).clamp(0.0, 1.0);
        }
        self.buttons = buttons;
        self.pressed |= pressed;
        self.released |= released;
        self.wheel = self.wheel.saturating_add(i32::from(wheel));
    }

    /// Integrate speed rather than multiplying elapsed time by a changing speed.
    pub fn advance(&mut self, seconds: f32) {
        let speed = if self.active {
            self.position[0] * 2.0
        } else {
            1.0
        };
        self.phase += seconds * speed;
    }

    /// Dedicated MAXPIX layout: inverse-view-projection columns are pointer
    /// [x, y, integrated motion time, active]; previous-view-projection columns
    /// are [held, pressed, released, wheel]. Neither matrix is used by Maxpix.
    pub fn pack(&self) -> ([f32; 4], [f32; 4]) {
        (
            [
                self.position[0],
                self.position[1],
                self.phase,
                u32::from(self.active) as f32,
            ],
            [
                self.buttons as f32,
                self.pressed as f32,
                self.released as f32,
                self.wheel as f32,
            ],
        )
    }

    pub fn published(&mut self) {
        self.pressed = 0;
        self.released = 0;
        self.wheel = 0;
    }
}

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
    fn native_point_width_cycle_covers_fractional_and_maximum_widths() {
        let mut width = 8;
        for expected in POINT_WIDTHS_RAW
            .iter()
            .cycle()
            .skip(1)
            .take(POINT_WIDTHS_RAW.len())
        {
            width = next_point_width_raw(width);
            assert_eq!(width, *expected);
            assert!((1..=2047).contains(&width));
        }
        assert_eq!(width, 8);
    }

    #[test]
    fn pointer_speed_integrates_without_retiming_existing_motion() {
        let mut pointer = PointerInput::default();
        pointer.advance(3.0);
        pointer.report([0, 50], [100, 100], 1, 1, 0, 0);
        pointer.advance(2.0);
        assert_eq!(pointer.phase, 3.0); // left edge pauses, without jumping back
        pointer.report([100, 100], [100, 100], 0, 0, 1, 2);
        pointer.advance(2.0);
        assert_eq!(pointer.phase, 7.0); // right edge runs at 2x
        let (state, buttons) = pointer.pack();
        assert_eq!(state, [1.0, 1.0, 7.0, 1.0]);
        assert_eq!(buttons, [0.0, 1.0, 1.0, 2.0]); // quick click retained
        pointer.published();
        assert_eq!(pointer.pack().1, [0.0; 4]);
    }

    #[test]
    fn four_pointer_channels_keep_independent_state() {
        let mut pointers = [PointerInput::default(); POINTER_COUNT];
        for (slot, pointer) in pointers.iter_mut().enumerate() {
            pointer.report(
                [slot as i32 * 25, -10],
                [100, 0],
                1 << slot,
                1 << slot,
                0,
                0,
            );
            pointer.advance(2.0);
        }
        for (slot, pointer) in pointers.iter().enumerate() {
            assert_eq!(pointer.phase, slot as f32);
            assert_eq!(pointer.position[1], 0.0);
            assert_eq!(pointer.pack().1[0], (1 << slot) as f32);
        }
    }

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
