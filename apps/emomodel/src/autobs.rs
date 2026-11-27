use glam::{
    Mat4, Vec3,
    camera::rh::{proj::directx::orthographic, view::look_at_mat4},
};
#[cfg(target_os = "trueos")]
use trueos::vgpu::RetainedCamera;

pub(super) const BASE_YAW: f32 = 36.0_f32.to_radians();
pub(super) const ARC_LIMIT: f32 = 8.0_f32.to_radians();
pub(super) const CAMERA_PITCH: f32 = 12.0_f32.to_radians();
const REFERENCE_DISTANCE: f32 = 3.4;
const FRAME_FILL: f32 = 0.94;
const POSE_INSET_PIXELS: f32 = 3.0;
const NEAR: f32 = 0.05;
const FAR: f32 = 16.0;
const APPROACH_NS: u64 = 1_650_000_000;
const RECEDE_NS: u64 = 3_000_000_000;

pub(super) fn camera_back(yaw: f32) -> Vec3 {
    Vec3::new(-yaw.sin(), 0.0, -yaw.cos())
}

pub(super) fn view_back(yaw: f32) -> Vec3 {
    let (sin, cos) = CAMERA_PITCH.sin_cos();
    camera_back(yaw) * cos + Vec3::Y * sin
}

#[derive(Clone, Copy, Debug)]
pub(super) struct PoseBounds {
    low: Vec3,
    high: Vec3,
}

impl Default for PoseBounds {
    fn default() -> Self {
        Self {
            low: Vec3::splat(f32::INFINITY),
            high: Vec3::splat(f32::NEG_INFINITY),
        }
    }
}

impl PoseBounds {
    pub(super) fn include(&mut self, point: Vec3) {
        self.low = self.low.min(point);
        self.high = self.high.max(point);
    }

    pub(super) fn clip_bounds(self, transform: Mat4) -> Self {
        if !self.low.is_finite() || !self.high.is_finite() {
            return self;
        }
        let mut bounds = Self::default();
        for x in [self.low.x, self.high.x] {
            for y in [self.low.y, self.high.y] {
                for z in [self.low.z, self.high.z] {
                    bounds.include(transform.transform_point3(Vec3::new(x, y, z)));
                }
            }
        }
        bounds
    }
}

pub(super) fn fit_projection(extent: (u32, u32), bounds: PoseBounds, min_height: f32) -> Mat4 {
    let span = bounds.high - bounds.low;
    if !span.is_finite() || span.x <= 0.0 || span.y <= 0.0 {
        return Mat4::IDENTITY;
    }
    let inset_half_span = |axis: u32| {
        let pixels = axis.max(1) as f32;
        (pixels - 2.0 * POSE_INSET_PIXELS).max(1.0) / pixels
    };
    let width = inset_half_span(extent.0);
    let height = inset_half_span(extent.1);
    let scale = (2.0 * width / span.x).min(2.0 * height / span.y.max(min_height));
    let center = (bounds.low + bounds.high) * (0.5 * scale);
    // Leave the observer's horizontal target in place while the pose fits.
    let x_room = (width - 0.5 * scale * span.x).max(0.0);
    let shift_x = center.x.clamp(-x_room, x_room) - center.x;
    let mut fit = Mat4::from_scale(Vec3::new(scale, scale, 1.0));
    fit.w_axis = Vec3::new(shift_x, -center.y, 0.0).extend(1.0);
    fit
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct ObserverPose {
    pub(super) zoom: f32,
    pub(super) distance: f32,
    pub(super) yaw_offset: f32,
    target: Vec3,
}

pub(super) struct Autobs {
    reference: (u32, u32),
    extent: (u32, u32),
    target: Vec3,
    look_target: Vec3,
    reference_pixels_per_unit: f32,
    animated_half_span: [f32; 2],
    from: ObserverPose,
    goal: ObserverPose,
    started_ns: u64,
    duration_ns: u64,
    yaw_arc: f32,
    front_on: bool,
}

impl Autobs {
    pub(super) fn new(
        reference: (u32, u32),
        target: [f32; 3],
        reference_pixels_per_unit: f32,
        animated_half_span: [f32; 2],
    ) -> Self {
        assert!(reference.0 > 0 && reference.1 > 0);
        assert!(reference_pixels_per_unit.is_finite() && reference_pixels_per_unit > 0.0);
        assert!(
            animated_half_span
                .iter()
                .all(|&span| span.is_finite() && span > 0.0)
        );
        let target = Vec3::from_array(target);
        let pose = fitted_pose(
            reference,
            reference,
            reference_pixels_per_unit,
            animated_half_span,
            target,
        );
        Self {
            reference,
            extent: reference,
            target,
            look_target: target,
            reference_pixels_per_unit,
            animated_half_span,
            from: pose,
            goal: pose,
            started_ns: 0,
            duration_ns: 0,
            yaw_arc: 0.0,
            front_on: false,
        }
    }

    pub(super) fn set_front_on(&mut self, front_on: bool) {
        self.front_on = front_on;
    }

    pub(super) fn aim_at_landing(&mut self, target: Vec3, weight: f32) {
        self.look_target = self.target.lerp(target, weight.clamp(0.0, 1.0));
    }

    pub(super) fn retarget(&mut self, extent: (u32, u32), from: ObserverPose, now_ns: u64) {
        let goal = fitted_pose(
            self.reference,
            extent,
            self.reference_pixels_per_unit,
            self.animated_half_span,
            from.target,
        );
        let area = u64::from(extent.0) * u64::from(extent.1);
        let prior_area = u64::from(self.extent.0) * u64::from(self.extent.1);
        let receding = area < prior_area || (area == prior_area && goal.zoom < from.zoom);
        self.duration_ns = if from == goal {
            0
        } else if receding {
            RECEDE_NS
        } else {
            APPROACH_NS
        };
        let change = (goal.distance / from.distance)
            .ln()
            .abs()
            .max((goal.zoom / from.zoom).ln().abs());
        let available_arc = if self.front_on {
            0.0
        } else {
            (ARC_LIMIT - from.yaw_offset.abs()).max(0.0)
        };
        let arc_sign = if receding { -1.0 } else { 1.0 };
        self.yaw_arc = arc_sign * available_arc * (change / 0.55).min(1.0);
        self.from = from;
        self.goal = goal;
        self.started_ns = now_ns;
        self.extent = extent;
    }

    pub(super) fn duration_ns(&self) -> u64 {
        self.duration_ns
    }

    pub(super) fn is_moving(&self, now_ns: u64) -> bool {
        now_ns.saturating_sub(self.started_ns) < self.duration_ns
    }

    pub(super) fn pose(&self, now_ns: u64) -> ObserverPose {
        let target = self.look_target;
        if !self.is_moving(now_ns) {
            return ObserverPose {
                target,
                ..self.goal
            };
        }
        let elapsed_ns = now_ns.saturating_sub(self.started_ns);
        if elapsed_ns == 0 {
            return ObserverPose {
                target,
                ..self.from
            };
        }
        let u = elapsed_ns as f32 / self.duration_ns as f32;
        let eased = ease(u);
        let remaining = 1.0 - u;
        let arc = 16.0 * u * u * remaining * remaining;
        ObserverPose {
            zoom: self.from.zoom + (self.goal.zoom - self.from.zoom) * eased,
            distance: self.from.distance + (self.goal.distance - self.from.distance) * eased,
            yaw_offset: self.from.yaw_offset * (1.0 - eased) + self.yaw_arc * arc,
            target,
        }
    }

    #[cfg(any(target_os = "trueos", test))]
    pub(super) fn framing_zoom(&self, pose: ObserverPose, extent: (u32, u32)) -> f32 {
        let goal = fitted_pose(
            self.reference,
            extent,
            self.reference_pixels_per_unit,
            self.animated_half_span,
            pose.target,
        );
        (pose.zoom / goal.zoom).min(1.0)
    }

    fn matrices(&self, pose: ObserverPose, extent: (u32, u32)) -> (Mat4, Mat4, Vec3, Vec3) {
        let back = if self.front_on {
            -Vec3::Z
        } else {
            view_back(BASE_YAW + pose.yaw_offset)
        };
        let eye = pose.target + back * pose.distance;
        let view = look_at_mat4(eye, pose.target, Vec3::Y);
        let diameter_pixels_per_unit = 2.0 * self.reference_pixels_per_unit * pose.zoom;
        let half_width = extent.0.max(1) as f32 / diameter_pixels_per_unit;
        let half_height = extent.1.max(1) as f32 / diameter_pixels_per_unit;
        let projection = orthographic(
            -half_width,
            half_width,
            -half_height,
            half_height,
            NEAR,
            FAR,
        );
        (view, projection, eye, -back)
    }

    #[cfg(target_os = "trueos")]
    pub(super) fn camera(&self, pose: ObserverPose, extent: (u32, u32)) -> RetainedCamera {
        let (view, projection, eye, forward) = self.matrices(pose, extent);
        let view_projection = projection * view;
        RetainedCamera {
            view: view.to_cols_array(),
            projection: projection.to_cols_array(),
            view_projection: view_projection.to_cols_array(),
            previous_view_projection: view_projection.to_cols_array(),
            inverse_view_projection: view_projection.inverse().to_cols_array(),
            position_near: eye.extend(NEAR).to_array(),
            forward_far: forward.extend(FAR).to_array(),
            ..Default::default()
        }
    }
}

fn ease(t: f32) -> f32 {
    t * t * t * (10.0 + t * (-15.0 + t * 6.0))
}

fn fitted_pose(
    reference: (u32, u32),
    extent: (u32, u32),
    reference_pixels_per_unit: f32,
    animated_half_span: [f32; 2],
    target: Vec3,
) -> ObserverPose {
    let (width, height) = (extent.0.max(1) as f32, extent.1.max(1) as f32);
    let area_scale = ((width / reference.0 as f32) * (height / reference.1 as f32)).sqrt();
    let fit_zoom = FRAME_FILL
        * (width / (2.0 * reference_pixels_per_unit * animated_half_span[0]))
            .min(height / (2.0 * reference_pixels_per_unit * animated_half_span[1]));
    ObserverPose {
        zoom: area_scale.min(fit_zoom),
        distance: (REFERENCE_DISTANCE / area_scale).clamp(1.6, 8.0),
        yaw_offset: 0.0,
        target,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const REFERENCE: (u32, u32) = (640, 560);
    const TARGET: [f32; 3] = [0.05, 0.8, -0.02];
    const PIXELS_PER_UNIT: f32 = 250.0;
    const HALF_SPAN: [f32; 2] = [0.65, 0.9];

    fn observer() -> Autobs {
        Autobs::new(REFERENCE, TARGET, PIXELS_PER_UNIT, HALF_SPAN)
    }

    fn inset_bounds() -> PoseBounds {
        let mut bounds = PoseBounds::default();
        bounds.include(Vec3::new(-0.2, -0.8, 0.1));
        bounds.include(Vec3::new(0.4, 0.5, 0.9));
        bounds
    }

    #[test]
    fn pose_fit_leaves_three_pixels_above_and_below() {
        let bounds = inset_bounds();
        for extent in [(640, 560), (1280, 720), (1280, 1440)] {
            let fit = fit_projection(extent, bounds, 0.0);
            let fitted = bounds.clip_bounds(fit);
            let top = (1.0 - fitted.high.y) * extent.1 as f32 * 0.5;
            let bottom = (fitted.low.y + 1.0) * extent.1 as f32 * 0.5;
            assert!((top - 3.0).abs() < 1e-3);
            assert!((bottom - 3.0).abs() < 1e-3);
            assert_eq!(fit.x_axis.x, fit.y_axis.y);
            assert_eq!(fit.w_axis.x, 0.0);
            assert_eq!(fitted.low.z, bounds.low.z);
            assert_eq!(fitted.high.z, bounds.high.z);
        }
    }

    #[test]
    fn pose_fit_can_fit_reaching_hands_or_limit_zoom_on_short_poses() {
        let mut reaching = inset_bounds();
        reaching.include(Vec3::new(1.2, 0.5, 0.5));
        let extent = (240, 700);
        let fitted = reaching.clip_bounds(fit_projection(extent, reaching, 0.0));
        let horizontal_inset = 1.0 - 6.0 / extent.0 as f32;
        assert!((fitted.high.x - horizontal_inset).abs() < 1e-5);
        assert!((fitted.low.x + horizontal_inset).abs() < 1e-5);

        let crouch = inset_bounds();
        let limited = crouch.clip_bounds(fit_projection(REFERENCE, crouch, 1.7));
        assert!(limited.high.y < 0.8);
        assert!((limited.high.y + limited.low.y).abs() < 1e-5);
    }

    #[test]
    fn front_on_heart_view_keeps_the_drawing_plane_parallel_on_resize() {
        let mut observer = observer();
        observer.set_front_on(true);
        let larger = (960, 840);
        observer.retarget(larger, observer.pose(0), 1);
        let pose = observer.pose(1 + APPROACH_NS / 2);
        let (_, _, _, forward) = observer.matrices(pose, larger);
        assert_eq!(forward, Vec3::Z);
        assert_eq!(pose.yaw_offset, 0.0);
    }

    #[test]
    fn starts_parked_and_approaches_faster_than_it_recedes() {
        let mut observer = observer();
        let parked = observer.pose(0);
        assert_eq!(parked.zoom, 1.0);
        assert_eq!(parked.distance, REFERENCE_DISTANCE);
        assert!(!observer.is_moving(0));

        let start = 10;
        observer.retarget((1280, 1120), parked, start);
        assert_eq!(observer.pose(start), parked);
        assert_eq!(observer.duration_ns(), APPROACH_NS);
        let approaching = observer.pose(start + APPROACH_NS / 2);
        assert!(approaching.distance < parked.distance);
        assert!(approaching.zoom > parked.zoom);
        assert!(approaching.yaw_offset > 0.0 && approaching.yaw_offset <= ARC_LIMIT);
        let arrived = observer.pose(start + APPROACH_NS);
        assert!(!observer.is_moving(start + APPROACH_NS));
        assert_eq!(arrived.yaw_offset, 0.0);

        let retreat_start = start + APPROACH_NS;
        observer.retarget(REFERENCE, arrived, retreat_start);
        assert_eq!(observer.duration_ns(), RECEDE_NS);
        let retreating = observer.pose(retreat_start + APPROACH_NS);
        assert!(observer.is_moving(retreat_start + APPROACH_NS));
        assert!(retreating.distance > arrived.distance);
        assert!(retreating.zoom < arrived.zoom);
        assert!(retreating.yaw_offset < 0.0 && retreating.yaw_offset >= -ARC_LIMIT);
        assert_eq!(observer.pose(retreat_start + RECEDE_NS), parked);
        assert!(!observer.is_moving(retreat_start + RECEDE_NS));
    }

    #[test]
    fn tight_framing_eases_into_a_larger_surface_and_fits_a_shrink_immediately() {
        let mut observer = observer();
        let larger = (1280, 1120);
        let parked = observer.pose(0);
        observer.retarget(larger, parked, 0);
        let arriving = observer.framing_zoom(observer.pose(APPROACH_NS / 2), larger);
        assert!(arriving < 1.0);
        assert!(arriving > observer.framing_zoom(parked, larger));
        let arrived = observer.pose(APPROACH_NS);
        assert_eq!(observer.framing_zoom(arrived, larger), 1.0);
        assert_eq!(observer.framing_zoom(arrived, REFERENCE), 1.0);
    }

    #[test]
    fn keeps_the_look_target_centered_and_fits_each_aspect_ratio() {
        let mut observer = observer();
        for extent in [(1280, 300), (300, 1280), (1280, 1120), (320, 280)] {
            observer.retarget(extent, observer.pose(0), 0);
            let orbit = observer.pose(APPROACH_NS / 2);
            let (view, projection, eye, forward) = observer.matrices(orbit, extent);
            let target = Vec3::from_array(TARGET);
            assert!((eye + orbit.distance * forward - target).length() < 1e-5);
            let center = (projection * view).project_point3(target);
            assert!(center.x.abs() < 1e-5 && center.y.abs() < 1e-5);
            assert!((0.0..1.0).contains(&center.z));

            let fitted = observer.pose(RECEDE_NS);
            let (_, projection, _, _) = observer.matrices(fitted, extent);
            let horizontal_scale = projection.x_axis.x * extent.0 as f32 / 2.0;
            let vertical_scale = projection.y_axis.y * extent.1 as f32 / 2.0;
            assert!((horizontal_scale - vertical_scale).abs() < 1e-3);
            assert!(projection.x_axis.x * HALF_SPAN[0] <= FRAME_FILL + 1e-5);
            assert!(projection.y_axis.y * HALF_SPAN[1] <= FRAME_FILL + 1e-5);
        }
    }

    #[test]
    fn repeats_a_resize_from_the_last_presented_pose() {
        let mut observer = observer();
        let parked = observer.pose(0);
        observer.retarget((1600, 1000), parked, 0);
        let visible = observer.pose(200_000_000);
        let second_resize = 340_000_000;
        assert_ne!(observer.pose(second_resize), visible);
        observer.retarget((500, 400), visible, second_resize);
        assert_eq!(observer.pose(second_resize), visible);
        for step in 0..=100 {
            let now = second_resize + step * (RECEDE_NS / 100);
            assert!(observer.pose(now).yaw_offset.abs() <= ARC_LIMIT + 1e-6);
        }
        let settled = observer.pose(second_resize + RECEDE_NS);
        assert_eq!(settled.yaw_offset, 0.0);
        let almost_settled = observer.pose(second_resize + RECEDE_NS - 1_000_000);
        assert!((settled.distance - almost_settled.distance).abs() < 1e-5);
        assert!((settled.zoom - almost_settled.zoom).abs() < 1e-5);
        assert!(almost_settled.yaw_offset.abs() < 1e-5);
    }

    #[test]
    fn landing_pan_can_reverse_independently_of_a_resize() {
        let mut observer = observer();
        let origin = Vec3::from_array(TARGET);
        let landing = origin + Vec3::new(-0.08, 0.0, -0.36);
        let parked = observer.pose(0);
        assert_eq!(parked.target, origin);

        observer.aim_at_landing(landing, 0.5);
        let resize = 100_000_000;
        let halfway = observer.pose(resize);
        assert!(halfway.target.abs_diff_eq(origin.lerp(landing, 0.5), 1e-6));
        assert_eq!(halfway.zoom, parked.zoom);
        assert_eq!(halfway.distance, parked.distance);
        let extent = (960, 560);
        observer.retarget(extent, halfway, resize);
        assert_eq!(observer.pose(resize), halfway);

        let time = resize + APPROACH_NS / 2;
        let resizing = observer.pose(time);
        observer.aim_at_landing(landing, 1.0);
        let arrived = observer.pose(time);
        assert!(arrived.target.abs_diff_eq(landing, 1e-6));
        assert_eq!(arrived.zoom, resizing.zoom);
        assert_eq!(arrived.distance, resizing.distance);
        assert_eq!(arrived.yaw_offset, resizing.yaw_offset);
        let (view, projection, eye, forward) = observer.matrices(arrived, extent);
        assert!(eye.y > landing.y);
        assert!((eye + forward * arrived.distance).abs_diff_eq(landing, 1e-6));
        let center = (projection * view).project_point3(landing);
        assert!(center.x.abs() < 1e-5 && center.y.abs() < 1e-5);

        observer.aim_at_landing(landing, 0.0);
        let returned = observer.pose(resize + APPROACH_NS);
        assert_eq!(returned.target, origin);
        assert_eq!(returned.yaw_offset, 0.0);
        observer.aim_at_landing(landing, 0.5);
        assert_eq!(observer.pose(resize + APPROACH_NS).target, halfway.target);
    }

    #[test]
    fn chooses_recede_timing_when_the_extent_shrinks_mid_approach() {
        let mut observer = observer();
        observer.retarget((1600, 1000), observer.pose(0), 0);
        let visible = observer.pose(100_000_000);
        observer.retarget((1000, 800), visible, 200_000_000);
        let settled = observer.pose(200_000_000 + RECEDE_NS);
        assert!(settled.distance < visible.distance);
        assert_eq!(observer.duration_ns(), RECEDE_NS);
    }
}
