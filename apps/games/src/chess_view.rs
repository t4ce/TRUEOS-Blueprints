use trueos_picasso::cam::{Camera, Projection, Quaternion};

pub const STEP: f32 = 1.35;
pub const CENTER_X: f32 = -2.425;
const DISTANCE: f32 = 35.0;
const RAD: f32 = core::f32::consts::PI / 180.0;

/// The chessboard lies on the XZ plane, with its near edge at row seven.
pub struct ChessView {
    pub yaw: i32,
    pub elevation: i32,
}

impl ChessView {
    pub const fn new() -> Self {
        Self {
            yaw: 0,
            elevation: 60,
        }
    }

    pub fn turn(&mut self, degrees: i32) {
        self.yaw = (self.yaw + degrees).rem_euclid(360);
    }

    pub fn tilt(&mut self, degrees: i32) {
        self.elevation = (self.elevation + degrees).clamp(30, 75);
    }

    pub fn cell(row: usize, col: usize) -> [f32; 3] {
        [
            CENTER_X + (col as f32 - 3.5) * STEP,
            0.0,
            (row as f32 - 3.5) * STEP,
        ]
    }

    fn basis(&self) -> ([f32; 3], [f32; 3], [f32; 3]) {
        let sy = libm::sinf(self.yaw as f32 * RAD);
        let cy = libm::cosf(self.yaw as f32 * RAD);
        let se = libm::sinf(self.elevation as f32 * RAD);
        let ce = libm::cosf(self.elevation as f32 * RAD);
        let right = [cy, 0.0, -sy];
        let up = [-sy * se, ce, -cy * se];
        let forward = [-sy * ce, -se, -cy * ce];
        (right, up, forward)
    }

    fn position(&self) -> [f32; 3] {
        let (right, _, forward) = self.basis();
        // Keep the board's centre at the left-hand playfield, including after rotation.
        let target = [CENTER_X - right[0] * CENTER_X, 0.0, -right[2] * CENTER_X];
        [
            target[0] - forward[0] * DISTANCE,
            -forward[1] * DISTANCE,
            target[2] - forward[2] * DISTANCE,
        ]
    }

    pub fn screen_point(&self, x: f32, y: f32) -> [f32; 3] {
        let (right, up, forward) = self.basis();
        let eye = self.position();
        [
            eye[0] + forward[0] * DISTANCE + right[0] * x + up[0] * y,
            eye[1] + forward[1] * DISTANCE + right[1] * x + up[1] * y,
            eye[2] + forward[2] * DISTANCE + right[2] * x + up[2] * y,
        ]
    }

    pub fn camera(&self, scene_width: f32, scene_height: f32) -> Camera {
        let yaw = Quaternion::from_axis_angle([0.0, 1.0, 0.0], self.yaw as f32 * RAD);
        let tilt = Quaternion::from_axis_angle([1.0, 0.0, 0.0], -(self.elevation as f32) * RAD);
        Camera {
            position: self.position(),
            rotation: (yaw * tilt).normalized(),
            projection: Projection::Orthographic {
                xmag: scene_width,
                ymag: scene_height,
                znear: 0.1,
                zfar: 100.0,
            },
        }
    }

    pub fn pick(
        &self,
        local_x: i32,
        local_y: i32,
        width: u32,
        height: u32,
        scene_width: f32,
        scene_height: f32,
    ) -> Option<(usize, usize)> {
        if width == 0 || height == 0 {
            return None;
        }
        let (right, up, forward) = self.basis();
        let screen_x = (local_x as f32 / width as f32 - 0.5) * scene_width;
        let screen_y = (0.5 - local_y as f32 / height as f32) * scene_height;
        let position = self.position();
        let origin = [
            position[0] + right[0] * screen_x + up[0] * screen_y,
            position[1] + right[1] * screen_x + up[1] * screen_y,
            position[2] + right[2] * screen_x + up[2] * screen_y,
        ];
        let t = -origin[1] / forward[1];
        if t < 0.0 {
            return None;
        }
        let x = origin[0] + forward[0] * t;
        let z = origin[2] + forward[2] * t;
        let col = (x - (CENTER_X - 4.0 * STEP)) / STEP;
        let row = (z + 4.0 * STEP) / STEP;
        if (0.0..8.0).contains(&row) && (0.0..8.0).contains(&col) {
            Some((row as usize, col as usize))
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chess_board_cells_are_picked_after_rotation_and_tilt() {
        for yaw in [0, 45, 90, 180, 270] {
            for elevation in [30, 45, 60, 75] {
                let view = ChessView { yaw, elevation };
                let (right, up, forward) = view.basis();
                let rotation = view.camera(21.96, 25.0).rotation;
                for (basis, axis) in [
                    (right, [1.0, 0.0, 0.0]),
                    (up, [0.0, 1.0, 0.0]),
                    (forward, [0.0, 0.0, -1.0]),
                ] {
                    let actual = rotation.rotate(axis);
                    for i in 0..3 {
                        assert!((actual[i] - basis[i]).abs() < 0.0001);
                    }
                }
                let eye = view.position();
                for (width, height, scene_width, scene_height) in
                    [(650, 740, 21.96, 25.0), (1920, 1080, 44.44, 25.0)]
                {
                    for row in 0..8 {
                        for col in 0..8 {
                            let p = ChessView::cell(row, col);
                            let delta = [p[0] - eye[0], p[1] - eye[1], p[2] - eye[2]];
                            let sx =
                                delta[0] * right[0] + delta[1] * right[1] + delta[2] * right[2];
                            let sy = delta[0] * up[0] + delta[1] * up[1] + delta[2] * up[2];
                            let x = ((sx / scene_width + 0.5) * width as f32) as i32;
                            let y = ((0.5 - sy / scene_height) * height as f32) as i32;
                            assert_eq!(
                                view.pick(x, y, width, height, scene_width, scene_height),
                                Some((row, col)),
                                "yaw={yaw} elevation={elevation} size={width}x{height}"
                            );
                        }
                    }
                }
            }
        }
    }
}
