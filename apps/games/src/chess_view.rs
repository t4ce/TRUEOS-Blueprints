use trueos_picasso::cam::{Camera, Projection, Quaternion};

pub const STEP: f32 = 0.90;
pub const TILE_SCALE: f32 = 0.41;
const BASE_DISTANCE: f32 = 16.0;
const BASE_YFOV: f32 = 55.0 * RAD;
const RAD: f32 = core::f32::consts::PI / 180.0;

/// A perspective view of the chessboard on the XZ plane.
pub struct ChessView {
    pub yaw: i32,
    pub elevation: i32,
    pub zoom: f32,
}

impl ChessView {
    pub const fn new() -> Self {
        Self {
            yaw: 0,
            elevation: 60,
            zoom: 1.0,
        }
    }

    pub fn turn(&mut self, degrees: i32) {
        self.yaw = (self.yaw + degrees).rem_euclid(360);
    }

    pub fn tilt(&mut self, degrees: i32) {
        self.elevation = (self.elevation + degrees).clamp(30, 75);
    }

    pub fn wheel_zoom(&mut self, wheel: i32) -> bool {
        if wheel == 0 {
            return false;
        }
        let old = self.zoom;
        self.zoom = (self.zoom + wheel.signum() as f32 * 0.1).clamp(0.5, 1.5);
        self.zoom != old
    }

    pub fn cell(row: usize, col: usize) -> [f32; 3] {
        [(col as f32 - 3.5) * STEP, 0.0, (row as f32 - 3.5) * STEP]
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

    fn distance(width: u32, height: u32) -> f32 {
        let aspect = width.max(1) as f32 / height.max(1) as f32;
        BASE_DISTANCE * (1.12 / aspect).max(1.0)
    }

    fn position(&self, width: u32, height: u32) -> [f32; 3] {
        let (_, _, forward) = self.basis();
        let distance = Self::distance(width, height);
        [
            -forward[0] * distance,
            -forward[1] * distance,
            -forward[2] * distance,
        ]
    }

    fn half_fov_tangent(&self) -> f32 {
        libm::tanf(BASE_YFOV * 0.5) / self.zoom
    }

    pub fn camera(&self, width: u32, height: u32) -> Camera {
        let yaw = Quaternion::from_axis_angle([0.0, 1.0, 0.0], self.yaw as f32 * RAD);
        let tilt = Quaternion::from_axis_angle([1.0, 0.0, 0.0], -(self.elevation as f32) * RAD);
        Camera {
            position: self.position(width, height),
            rotation: (yaw * tilt).normalized(),
            projection: Projection::Perspective {
                yfov: 2.0 * libm::atanf(self.half_fov_tangent()),
                aspect_ratio: None,
                znear: 0.1,
                zfar: Some(100.0),
            },
        }
    }

    pub fn pick(
        &self,
        local_x: i32,
        local_y: i32,
        width: u32,
        height: u32,
    ) -> Option<(usize, usize)> {
        if width == 0
            || height == 0
            || local_x < 0
            || local_y < 0
            || local_x >= width as i32
            || local_y >= height as i32
        {
            return None;
        }
        let (right, up, forward) = self.basis();
        let aspect = width as f32 / height as f32;
        let half_tan = self.half_fov_tangent();
        let sx = (2.0 * (local_x as f32 + 0.5) / width as f32 - 1.0) * aspect * half_tan;
        let sy = (1.0 - 2.0 * (local_y as f32 + 0.5) / height as f32) * half_tan;
        let direction = [
            forward[0] + right[0] * sx + up[0] * sy,
            forward[1] + right[1] * sx + up[1] * sy,
            forward[2] + right[2] * sx + up[2] * sy,
        ];
        if direction[1] >= -0.0001 {
            return None;
        }
        let eye = self.position(width, height);
        let t = (TILE_SCALE - eye[1]) / direction[1];
        if t <= 0.0 {
            return None;
        }
        let x = eye[0] + direction[0] * t;
        let z = eye[2] + direction[2] * t;
        let col = (x + 4.0 * STEP) / STEP;
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

    fn project(view: &ChessView, point: [f32; 3], width: u32, height: u32) -> (i32, i32) {
        let camera = view
            .camera(width, height)
            .retained(width, height, [0.0; 16]);
        let p = [point[0], point[1], point[2], 1.0];
        let clip: [f32; 4] = core::array::from_fn(|r| {
            (0..4)
                .map(|c| camera.view_projection[c * 4 + r] * p[c])
                .sum()
        });
        (
            ((clip[0] / clip[3] * 0.5 + 0.5) * width as f32) as i32,
            ((0.5 - clip[1] / clip[3] * 0.5) * height as f32) as i32,
        )
    }

    #[test]
    fn perspective_projection_and_picking_agree_at_all_angles_and_zooms() {
        for yaw in [0, 45, 90, 180, 270] {
            for elevation in [30, 45, 60, 75] {
                for zoom in [0.5, 1.0, 1.5] {
                    let view = ChessView {
                        yaw,
                        elevation,
                        zoom,
                    };
                    for (width, height) in [(650, 740), (1920, 1080)] {
                        for row in 0..8 {
                            for col in 0..8 {
                                let mut point = ChessView::cell(row, col);
                                point[1] = TILE_SCALE;
                                let (x, y) = project(&view, point, width, height);
                                if (0..width as i32).contains(&x) && (0..height as i32).contains(&y)
                                {
                                    assert_eq!(
                                        view.pick(x, y, width, height),
                                        Some((row, col)),
                                        "yaw={yaw} elevation={elevation} zoom={zoom} {width}x{height}"
                                    );
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}
