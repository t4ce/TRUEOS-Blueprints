use trueos_picasso::cam::{Camera, Projection, Quaternion};

// Previous closest distance (0.15 radii), moved outward by five wheel ticks:
// 0.15 * exp(5 * 0.12). Shared by wheel clamping, reframing and the intro.
const MIN_DISTANCE_RADIUS: f32 = 0.27331782;

const BASE_ELEVATION: f32 = 70.0 * core::f32::consts::PI / 180.0;
const HORIZONTAL_TILT: f32 = 8.0 * core::f32::consts::PI / 180.0;
const VERTICAL_TILT: f32 = 15.0 * core::f32::consts::PI / 180.0;

/// A bounded hover tilt around the scene target, using Picasso's camera.
pub struct OrbitCamera {
    yaw: f32,
    elevation: f32,
    distance: f32,
    radius: f32,
    target: [f32; 3],
}

impl OrbitCamera {
    pub fn new(radius: f32) -> Self {
        let radius = if radius.is_finite() && radius > 0.0 {
            radius
        } else {
            1.0
        };
        Self {
            yaw: 0.0,
            elevation: BASE_ELEVATION,
            distance: radius * 1.95,
            radius,
            target: [0.0; 3],
        }
    }

    /// Reframe a changed scene while retaining the user's orbit and relative zoom.
    pub fn set_scene(&mut self, target: [f32; 3], radius: f32) {
        let radius = if radius.is_finite() && radius > 0.0 {
            radius
        } else {
            1.0
        };
        self.distance = (self.distance * (radius / self.radius))
            .clamp(radius * MIN_DISTANCE_RADIUS, radius * 20.0);
        self.radius = radius;
        self.target = target;
    }

    pub fn camera(&self) -> Camera {
        let yaw = Quaternion::from_axis_angle([0.0, 1.0, 0.0], self.yaw);
        let tilt = Quaternion::from_axis_angle([1.0, 0.0, 0.0], -self.elevation);
        let rotation = (yaw * tilt).normalized();
        let offset = rotation.rotate([0.0, 0.0, self.distance]);
        Camera {
            position: core::array::from_fn(|axis| self.target[axis] + offset[axis]),
            rotation,
            projection: Projection::Perspective {
                yfov: core::f32::consts::FRAC_PI_4,
                znear: self.radius * 0.001,
                zfar: Some(self.radius * 22.0),
                aspect_ratio: None,
            },
        }
    }

    pub fn retained(
        &self,
        width: u32,
        height: u32,
        previous_view_projection: [f32; 16],
    ) -> trueos::vgpu::RetainedCamera {
        // The retained material renderer compensates its Naga clip Y in the
        // viewport and selects the matching front faces for glTF winding.
        self.camera()
            .retained(width, height, previous_view_projection)
    }

    /// Animate only the view distance; wheel input still adjusts the destination.
    pub fn retained_intro(
        &self,
        width: u32,
        height: u32,
        previous: [f32; 16],
        progress: f32,
    ) -> trueos::vgpu::RetainedCamera {
        let mut camera = self.camera();
        let distance = self.radius * MIN_DISTANCE_RADIUS
            + (self.distance - self.radius * MIN_DISTANCE_RADIUS) * progress.clamp(0., 1.);
        for axis in 0..3 {
            camera.position[axis] = self.target[axis]
                + (camera.position[axis] - self.target[axis]) * distance / self.distance;
        }
        camera.retained(width, height, previous)
    }

    /// Normalized, equally shared cursor input gives bounded absolute tilt.
    /// Downward input lowers elevation (the inverse of the former drag pitch).
    pub fn hover_tilt(&mut self, position: [f32; 2]) -> bool {
        let yaw = -position[0].clamp(-1.0, 1.0) * HORIZONTAL_TILT;
        let elevation = BASE_ELEVATION - position[1].clamp(-1.0, 1.0) * VERTICAL_TILT;
        let changed = self.yaw != yaw || self.elevation != elevation;
        self.yaw = yaw;
        self.elevation = elevation;
        changed
    }

    // Retain broad camera angles only for the GLB winding/projection tests.
    #[cfg(test)]
    fn orbit(&mut self, dx: i32, dy: i32) {
        const SENSITIVITY: f32 = 0.006;
        const MAX_ELEVATION: f32 = core::f32::consts::FRAC_PI_2 - 0.02;
        self.yaw = libm::fmodf(self.yaw - dx as f32 * SENSITIVITY, core::f32::consts::TAU);
        self.elevation =
            (self.elevation + dy as f32 * SENSITIVITY).clamp(-MAX_ELEVATION, MAX_ELEVATION);
    }

    /// Positive wheel ticks move closer without changing the field of view.
    pub fn zoom(&mut self, wheel: i16) {
        self.distance = (self.distance * libm::expf(-(wheel as f32) * 0.12))
            .clamp(self.radius * MIN_DISTANCE_RADIUS, self.radius * 20.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn distance(camera: Camera) -> f32 {
        libm::sqrtf(camera.position.iter().map(|axis| axis * axis).sum())
    }

    fn assert_aims_at_origin(camera: Camera) {
        let distance = distance(camera);
        let forward = camera.rotation.rotate([0.0, 0.0, -1.0]);
        for axis in 0..3 {
            assert!((forward[axis] + camera.position[axis] / distance).abs() < 0.000_01);
        }
    }

    fn transform(matrix: [f32; 16], point: [f32; 4]) -> [f32; 4] {
        core::array::from_fn(|row| {
            (0..4)
                .map(|column| matrix[column * 4 + row] * point[column])
                .sum()
        })
    }

    /// The retained material executable negates Y before its positive
    /// SF viewport scale; keep both stages explicit in this screen oracle.
    fn screen(camera: trueos::vgpu::RetainedCamera, point: [f32; 3]) -> [f32; 2] {
        let clip = transform(camera.view_projection, [point[0], point[1], point[2], 1.0]);
        [
            clip[0] / clip[3] * 400.0 + 400.0,
            -clip[1] / clip[3] * 300.0 + 300.0,
        ]
    }

    #[test]
    fn retained_projection_keeps_visual_up_and_inverse_and_history_consistent() {
        let orbit = OrbitCamera::new(2.0);
        let first = orbit.retained(800, 600, [0.0; 16]);
        let origin = screen(first, [0.0; 3]);
        let up = screen(first, orbit.camera().rotation.rotate([0.0, 1.0, 0.0]));
        assert!((up[0] - origin[0]).abs() < 0.0001);
        assert!(up[1] < origin[1]);

        let world = [0.3, 0.4, 0.5, 1.0];
        let clip = transform(first.view_projection, world);
        let restored = transform(first.inverse_view_projection, clip);
        for axis in 0..4 {
            assert!((restored[axis] / restored[3] - world[axis]).abs() < 0.002);
        }
        let next = orbit.retained(800, 600, first.view_projection);
        assert_eq!(next.previous_view_projection, first.view_projection);
    }

    #[test]
    fn hover_tilt_is_bounded_reversible_and_does_not_accumulate() {
        let mut orbit = OrbitCamera::new(2.0);
        let initial = orbit.camera();
        assert!(!orbit.hover_tilt([0., 0.]));
        assert!(orbit.hover_tilt([-1., 0.]));
        let left = orbit.camera();
        assert!(left.position[0] > initial.position[0]);
        assert!(!orbit.hover_tilt([-100., 0.]));
        assert!(orbit.hover_tilt([1., 0.]));
        let right = orbit.camera();
        assert!(right.position[0] < initial.position[0]);
        for _ in 0..1000 {
            assert!(!orbit.hover_tilt([100., 0.]));
        }
        assert_eq!(orbit.camera().rotation, right.rotation);
        assert!(orbit.yaw.abs() <= 8.0 * core::f32::consts::PI / 180.0);
        assert!(orbit.hover_tilt([0., 0.]));
        assert_eq!(orbit.camera().rotation, initial.rotation);
        assert_eq!(orbit.elevation, 70.0 * core::f32::consts::PI / 180.0);
        assert!((distance(orbit.camera()) - distance(initial)).abs() < 1e-5);
        assert_aims_at_origin(orbit.camera());
    }

    #[test]
    fn vertical_hover_is_inverted_limited_and_preserves_zoom_and_target() {
        let mut orbit = OrbitCamera::new(2.);
        orbit.zoom(2);
        let distance_before = distance(orbit.camera());
        assert!(orbit.hover_tilt([0., -1.]));
        assert!((orbit.elevation - 85. * core::f32::consts::PI / 180.).abs() < 1e-6);
        assert!(!orbit.hover_tilt([0., -100.]));
        assert!(orbit.hover_tilt([0., 1.]));
        assert!((orbit.elevation - 55. * core::f32::consts::PI / 180.).abs() < 1e-6);
        assert!(!orbit.hover_tilt([0., 100.]));
        assert!((distance(orbit.camera()) - distance_before).abs() < 1e-5);
        assert_aims_at_origin(orbit.camera());
        assert!(orbit.hover_tilt([0., 0.]));
        assert_eq!(orbit.elevation, BASE_ELEVATION);
    }

    #[test]
    fn prepared_keyboard_keeps_exterior_faces_with_native_culling() {
        let vertices = include_bytes!(concat!(env!("OUT_DIR"), "/keyboard.vertices.bin"));
        let indices = include_bytes!(concat!(env!("OUT_DIR"), "/keyboard.indices.bin"));
        let vertex = |index: usize| {
            let row = &vertices[index * 24..][..24];
            let values: [f32; 6] = core::array::from_fn(|axis| {
                f32::from_le_bytes(row[axis * 4..axis * 4 + 4].try_into().unwrap())
            });
            (
                [values[0], values[1], values[2]],
                [values[3], values[4], values[5]],
            )
        };
        let mut orbit = OrbitCamera::new(20.0);
        for (dx, dy) in [(0, 0), (240, 0), (0, 140), (460, -280), (0, 260)] {
            orbit.orbit(dx, dy);
            let eye = orbit.camera().position;
            let camera = orbit.retained(800, 600, [0.0; 16]);
            let (mut front, mut back) = (0, 0);
            for triangle in indices.chunks_exact(12) {
                let rows: [([f32; 3], [f32; 3]); 3] = core::array::from_fn(|corner| {
                    let index = u32::from_le_bytes(
                        triangle[corner * 4..corner * 4 + 4].try_into().unwrap(),
                    );
                    vertex(index as usize)
                });
                // Authored flat normals identify the exterior independently
                // of the renderer's prepared triangle order.
                if rows.iter().any(|(_, normal)| {
                    (0..3).any(|axis| (normal[axis] - rows[0].1[axis]).abs() > 0.0001)
                }) {
                    continue;
                }
                // The beveled K70 export includes nearly collinear sliver
                // triangles. Their winding can flip under f32 rounding; they
                // have no stable raster area and cannot diagnose culling.
                let ab: [f32; 3] = core::array::from_fn(|i| rows[1].0[i] - rows[0].0[i]);
                let ac: [f32; 3] = core::array::from_fn(|i| rows[2].0[i] - rows[0].0[i]);
                let cross = [
                    ab[1] * ac[2] - ab[2] * ac[1],
                    ab[2] * ac[0] - ab[0] * ac[2],
                    ab[0] * ac[1] - ab[1] * ac[0],
                ];
                let area3 = libm::sqrtf(cross.iter().map(|v| v * v).sum());
                let edge2 = ab
                    .iter()
                    .map(|v| v * v)
                    .sum::<f32>()
                    .max(ac.iter().map(|v| v * v).sum());
                if area3 <= edge2 * 0.00001 {
                    continue;
                }
                let center: [f32; 3] = core::array::from_fn(|axis| {
                    rows.iter().map(|(point, _)| point[axis]).sum::<f32>() / 3.0
                });
                let to_eye: [f32; 3] = core::array::from_fn(|axis| eye[axis] - center[axis]);
                assert!(
                    (0..3)
                        .map(|axis| cross[axis] * rows[0].1[axis])
                        .sum::<f32>()
                        > 0.0,
                    "authored normal opposes a nondegenerate exterior triangle"
                );
                // Bevel/custom normals need not be exactly perpendicular to a
                // flat triangle. Use its geometric normal for grazing views.
                let facing: f32 = (0..3).map(|axis| cross[axis] * to_eye[axis] / area3).sum();
                let eye_distance = libm::sqrtf(to_eye.iter().map(|v| v * v).sum());
                if facing.abs() < eye_distance * 0.001 {
                    continue;
                }
                let [a, b, c] = rows.map(|(point, _)| {
                    let clip =
                        transform(camera.view_projection, [point[0], point[1], point[2], 1.0]);
                    assert!(clip[3] > 0.0);
                    screen(camera, point)
                });
                let area = (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]);
                if area.abs() < 0.001 {
                    continue;
                } // Below stable subpixel winding precision.
                // The material renderer adjusts front-face selection for its
                // positive viewport Y: original glTF fronts need area < 0.
                assert!(
                    area * facing < 0.0,
                    "culled exterior / retained interior at orbit {dx},{dy} area={area} facing={facing} rows={rows:?}"
                );
                if facing > 0.0 {
                    front += 1;
                } else {
                    back += 1;
                }
            }
            assert!(front > 100 && back > 100, "insufficient face coverage");
        }
    }

    #[test]
    fn orbit_always_aims_at_origin_including_elevation_limits() {
        let mut orbit = OrbitCamera::new(3.2);
        assert!(orbit.camera().position[1] > 0.0);
        assert_aims_at_origin(orbit.camera());
        for (dx, dy) in [(125, -80), (-700, 2_000), (12_000, -4_000)] {
            orbit.orbit(dx, dy);
            let camera = orbit.camera();
            assert_aims_at_origin(camera);
            assert!((distance(camera) - 6.24).abs() < 0.000_01);
        }
    }

    #[test]
    fn intro_starts_at_near_limit_and_finishes_at_user_zoom_without_changing_it() {
        let mut orbit = OrbitCamera::new(2.);
        orbit.set_scene([3., 0., 7.], 8.);
        orbit.hover_tilt([0.3, -0.4]);
        orbit.zoom(2);
        let ordinary = orbit.retained(800, 600, [0.; 16]);
        let near = orbit.retained_intro(800, 600, [0.; 16], 0.);
        let offset: [f32; 3] = core::array::from_fn(|i| near.position_near[i] - orbit.target[i]);
        assert!(
            (libm::sqrtf(offset.iter().map(|v| v * v).sum()) - 8. * 0.15 * libm::expf(5. * 0.12))
                .abs()
                < 1e-5
        );
        let finish = orbit.retained_intro(800, 600, [0.; 16], 1.);
        for (a, b) in finish.view_projection.iter().zip(ordinary.view_projection) {
            assert!((a - b).abs() < 1e-5);
        }
        assert_eq!(
            orbit.retained(800, 600, [0.; 16]).view_projection,
            ordinary.view_projection
        );
    }

    #[test]
    fn wheel_clamps_distance_and_preserves_projection() {
        let mut orbit = OrbitCamera::new(2.0);
        let initial = orbit.camera();
        orbit.zoom(1);
        assert!(distance(orbit.camera()) < distance(initial));
        orbit.zoom(i16::MAX);
        assert!((distance(orbit.camera()) - 0.3 * libm::expf(5. * 0.12)).abs() < 0.000_01);
        orbit.zoom(i16::MIN);
        let far = orbit.camera();
        assert!((distance(far) - 40.0).abs() < 0.000_01);
        assert_eq!(far.projection, initial.projection);
        assert_aims_at_origin(far);
    }

    #[test]
    fn changed_row_stays_centered_and_preserves_orbit_and_relative_zoom() {
        let mut orbit = OrbitCamera::new(2.0);
        orbit.orbit(125, -80);
        orbit.zoom(3);
        let initial = orbit.camera();
        let target = [0.0, 0.0, 7.5];
        orbit.set_scene(target, 8.0);

        let camera = orbit.camera();
        let offset: [f32; 3] = core::array::from_fn(|axis| camera.position[axis] - target[axis]);
        let row_distance = libm::sqrtf(offset.iter().map(|axis| axis * axis).sum());
        assert!((row_distance / 8.0 - distance(initial) / 2.0).abs() < 0.000_01);
        assert_eq!(
            camera.rotation.rotate([0.0, 0.0, -1.0]),
            initial.rotation.rotate([0.0, 0.0, -1.0])
        );

        let retained = orbit.retained(800, 600, [0.0; 16]);
        let center = screen(retained, target);
        assert!((center[0] - 400.0).abs() < 0.001);
        assert!((center[1] - 300.0).abs() < 0.001);
    }
}
