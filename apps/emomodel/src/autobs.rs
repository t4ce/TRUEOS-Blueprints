use glam::{
    Mat3, Mat4, Vec3,
    camera::rh::{proj::directx::orthographic, view::look_at_mat4},
};
#[cfg(target_os = "trueos")]
use trueos::vgpu::RetainedCamera;

pub(super) const BASE_YAW: f32 = 36.0_f32.to_radians();
pub(super) const ARC_LIMIT: f32 = 8.0_f32.to_radians();
pub(super) const CAMERA_PITCH: f32 = 12.0_f32.to_radians();
const REFERENCE_DISTANCE: f32 = 3.4;
const FRAME_INSET_PIXELS: f32 = 4.0;
const NEAR: f32 = 0.05;
const FAR: f32 = 16.0;
const APPROACH_NS: u64 = 1_650_000_000;
const RECEDE_NS: u64 = 3_000_000_000;
const DEPARTURE_ROOM: f32 = 0.25;
const DEPARTURE_PULLBACK_NS: u64 = 900_000_000;
const DEPARTURE_HOLD_UNTIL_NS: u64 = 3_500_000_000;
const DEPARTURE_RETURN_NS: u64 = 1_650_000_000;

pub(super) fn camera_back(yaw: f32) -> Vec3 {
    Vec3::new(-yaw.sin(), 0.0, -yaw.cos())
}

pub(super) fn view_back(yaw: f32) -> Vec3 {
    let (sin, cos) = CAMERA_PITCH.sin_cos();
    camera_back(yaw) * cos + Vec3::Y * sin
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Framing {
    pub(super) target: Vec3,
    pub(super) half_span: [f32; 2],
    pub(super) front_on: bool,
}

pub(super) struct Bounds {
    world_to_view: Mat3,
    low: Vec3,
    high: Vec3,
    front_on: bool,
}

impl Bounds {
    pub(super) fn new(front_on: bool) -> Self {
        let (right, back) = if front_on {
            (-Vec3::X, -Vec3::Z)
        } else {
            (Vec3::Y.cross(camera_back(BASE_YAW)), view_back(BASE_YAW))
        };
        let up = back.cross(right);
        Self {
            world_to_view: Mat3::from_cols(right, up, back).transpose(),
            low: Vec3::splat(f32::INFINITY),
            high: Vec3::splat(f32::NEG_INFINITY),
            front_on,
        }
    }

    pub(super) fn include(&mut self, position: Vec3) {
        let point = self.world_to_view * position;
        self.low = self.low.min(point);
        self.high = self.high.max(point);
    }

    pub(super) fn framing(&self, padding: f32) -> Framing {
        let center = (self.low + self.high) * 0.5;
        let half = (self.high - self.low) * 0.5;
        assert!(half.is_finite() && half.x > 0.0 && half.y > 0.0);
        assert!(padding.is_finite() && padding >= 0.0);
        let mut half_span = [half.x + padding, half.y + padding];
        if !self.front_on {
            // A resize orbits around world up. Bound its effect on both axes
            // from the fixed view box, including horizontal depth under pitch.
            let (arc_sin, arc_cos) = ARC_LIMIT.sin_cos();
            let (pitch_sin, pitch_cos) = CAMERA_PITCH.sin_cos();
            let horizontal_depth = half.z * pitch_cos + half.y * pitch_sin;
            half_span[0] += horizontal_depth * arc_sin;
            half_span[1] += pitch_sin * (half.x * arc_sin + horizontal_depth * (1.0 - arc_cos));
        }
        Framing {
            target: self.world_to_view.transpose() * center,
            half_span,
            front_on: self.front_on,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct ObserverPose {
    pub(super) zoom: f32,
    pub(super) distance: f32,
    pub(super) yaw_offset: f32,
    departure: f32,
}

impl ObserverPose {
    fn departure_scale(self) -> f32 {
        1.0 + DEPARTURE_ROOM * self.departure
    }

    #[cfg(target_os = "trueos")]
    pub(super) fn rendered_zoom(self) -> f32 {
        self.zoom / self.departure_scale()
    }

    pub(super) fn rendered_distance(self) -> f32 {
        self.distance * self.departure_scale()
    }
}

struct ButterflyDeparture {
    start_ns: u64,
    from: f32,
}

impl ButterflyDeparture {
    fn amount(&self, now_ns: u64) -> f32 {
        let elapsed = now_ns.saturating_sub(self.start_ns);
        if elapsed < DEPARTURE_PULLBACK_NS {
            self.from + (1.0 - self.from) * ease(elapsed as f32 / DEPARTURE_PULLBACK_NS as f32)
        } else {
            let returning = elapsed.saturating_sub(DEPARTURE_HOLD_UNTIL_NS);
            1.0 - ease((returning as f32 / DEPARTURE_RETURN_NS as f32).min(1.0))
        }
    }
}

pub(super) struct Autobs {
    reference: (u32, u32),
    extent: (u32, u32),
    framing: Framing,
    reference_pixels_per_unit: f32,
    from: ObserverPose,
    goal: ObserverPose,
    started_ns: u64,
    duration_ns: u64,
    yaw_arc: f32,
    departure: Option<ButterflyDeparture>,
}

impl Autobs {
    pub(super) fn new(
        reference: (u32, u32),
        framing: Framing,
        reference_pixels_per_unit: f32,
    ) -> Self {
        assert!(reference.0 > 0 && reference.1 > 0);
        assert!(reference_pixels_per_unit.is_finite() && reference_pixels_per_unit > 0.0);
        let pose = fitted_pose(reference, reference, reference_pixels_per_unit, framing);
        Self {
            reference,
            extent: reference,
            framing,
            reference_pixels_per_unit,
            from: pose,
            goal: pose,
            started_ns: 0,
            duration_ns: 0,
            yaw_arc: 0.0,
            departure: None,
        }
    }

    pub(super) fn set_framing(&mut self, framing: Framing, now_ns: u64) -> bool {
        if framing == self.framing {
            return false;
        }
        let previous = self.resize_pose(now_ns);
        let remaining = self
            .duration_ns
            .saturating_sub(now_ns.saturating_sub(self.started_ns));
        self.framing = framing;
        self.goal = self.fitted_pose(self.extent);
        self.from = if remaining == 0 {
            self.goal
        } else {
            ObserverPose {
                zoom: previous.zoom.min(self.goal.zoom),
                yaw_offset: if framing.front_on {
                    0.0
                } else {
                    previous.yaw_offset
                },
                ..previous
            }
        };
        self.started_ns = now_ns;
        self.duration_ns = remaining;
        self.yaw_arc = 0.0;
        true
    }

    pub(super) fn retarget(&mut self, extent: (u32, u32), from: ObserverPose, now_ns: u64) {
        let from = ObserverPose {
            departure: 0.0,
            ..from
        };
        let goal = self.fitted_pose(extent);
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
        let available_arc = if self.framing.front_on {
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

    fn fitted_pose(&self, extent: (u32, u32)) -> ObserverPose {
        fitted_pose(
            self.reference,
            extent,
            self.reference_pixels_per_unit,
            self.framing,
        )
    }

    pub(super) fn duration_ns(&self) -> u64 {
        self.duration_ns
    }

    pub(super) fn is_moving(&self, now_ns: u64) -> bool {
        now_ns.saturating_sub(self.started_ns) < self.duration_ns
    }

    pub(super) fn pose(&self, now_ns: u64) -> ObserverPose {
        let departure = self
            .departure
            .as_ref()
            .map_or(0.0, |effect| effect.amount(now_ns));
        ObserverPose {
            departure,
            ..self.resize_pose(now_ns)
        }
    }

    pub(super) fn butterfly_departure(&mut self, from: ObserverPose, now_ns: u64) {
        self.departure = Some(ButterflyDeparture {
            start_ns: now_ns,
            from: from.departure,
        });
    }

    fn resize_pose(&self, now_ns: u64) -> ObserverPose {
        if !self.is_moving(now_ns) {
            return self.goal;
        }
        let elapsed_ns = now_ns.saturating_sub(self.started_ns);
        if elapsed_ns == 0 {
            return self.from;
        }
        let u = elapsed_ns as f32 / self.duration_ns as f32;
        let eased = ease(u);
        let remaining = 1.0 - u;
        let arc = 16.0 * u * u * remaining * remaining;
        ObserverPose {
            zoom: self.from.zoom + (self.goal.zoom - self.from.zoom) * eased,
            distance: self.from.distance + (self.goal.distance - self.from.distance) * eased,
            yaw_offset: self.from.yaw_offset * (1.0 - eased) + self.yaw_arc * arc,
            departure: 0.0,
        }
    }

    fn matrices(&self, pose: ObserverPose, extent: (u32, u32)) -> (Mat4, Mat4, Vec3, Vec3) {
        let back = if self.framing.front_on {
            -Vec3::Z
        } else {
            view_back(BASE_YAW + pose.yaw_offset)
        };
        let target = self.framing.target;
        let eye = target + back * pose.rendered_distance();
        let view = look_at_mat4(eye, target, Vec3::Y);
        // Keep the sampled pixel scale even when the backing extent changes.
        let diameter_pixels_per_unit = 2.0 * self.reference_pixels_per_unit * pose.zoom;
        let half_width = extent.0.max(1) as f32 / diameter_pixels_per_unit * pose.departure_scale();
        let half_height = extent.1.max(1) as f32 / diameter_pixels_per_unit;
        // Grow the frame above its current bottom edge and keep it centered horizontally.
        let top = half_height * (2.0 * pose.departure_scale() - 1.0);
        let projection = orthographic(-half_width, half_width, -half_height, top, NEAR, FAR);
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

fn fitted_zoom(extent: (u32, u32), reference_pixels_per_unit: f32, framing: Framing) -> f32 {
    let available = |axis: u32| (axis as f32 - 2.0 * FRAME_INSET_PIXELS).max(1.0);
    let [half_width, half_height] = framing.half_span;
    (available(extent.0) / half_width).min(available(extent.1) / half_height)
        / (2.0 * reference_pixels_per_unit)
}

fn fitted_pose(
    reference: (u32, u32),
    extent: (u32, u32),
    reference_pixels_per_unit: f32,
    framing: Framing,
) -> ObserverPose {
    let (width, height) = (extent.0.max(1) as f32, extent.1.max(1) as f32);
    let area_scale = ((width / reference.0 as f32) * (height / reference.1 as f32)).sqrt();
    ObserverPose {
        zoom: fitted_zoom(extent, reference_pixels_per_unit, framing),
        distance: (REFERENCE_DISTANCE / area_scale).clamp(1.6, 8.0),
        yaw_offset: 0.0,
        departure: 0.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const REFERENCE: (u32, u32) = (640, 560);
    const PIXELS_PER_UNIT: f32 = 250.0;
    const NORMAL: Framing = Framing {
        target: Vec3::new(0.05, 0.8, -0.02),
        half_span: [0.65, 0.9],
        front_on: false,
    };
    const HEART: Framing = Framing {
        target: Vec3::new(0.1, 0.9, -0.15),
        half_span: [0.45, 0.8],
        front_on: true,
    };

    fn observer() -> Autobs {
        Autobs::new(REFERENCE, NORMAL, PIXELS_PER_UNIT)
    }

    #[test]
    fn holds_the_same_projection_through_playback() {
        let mut observer = observer();
        let before = observer.matrices(observer.pose(0), REFERENCE);
        for time in [1, 10_000_000, 55_000_000, 2_000_000_000] {
            assert!(!observer.set_framing(NORMAL, time));
            assert_eq!(observer.matrices(observer.pose(time), REFERENCE), before);
        }
    }

    #[test]
    fn fits_the_limiting_axis_with_four_pixels_of_clearance() {
        let mut observer = observer();
        for extent in [(640, 560), (1280, 300), (300, 1280), (2560, 1440)] {
            observer.retarget(extent, observer.pose(0), 0);
            let fitted = observer.pose(RECEDE_NS);
            let (view, projection, eye, forward) = observer.matrices(fitted, extent);
            assert!((eye + fitted.distance * forward - NORMAL.target).length() < 1e-5);
            let center = (projection * view).project_point3(NORMAL.target);
            assert!(center.x.abs() < 1e-5 && center.y.abs() < 1e-5);
            let horizontal_scale = projection.x_axis.x * extent.0 as f32 * 0.5;
            let vertical_scale = projection.y_axis.y * extent.1 as f32 * 0.5;
            assert!((horizontal_scale - vertical_scale).abs() < 1e-3);
            let x_margin = extent.0 as f32 * 0.5 - horizontal_scale * NORMAL.half_span[0];
            let y_margin = extent.1 as f32 * 0.5 - vertical_scale * NORMAL.half_span[1];
            assert!((x_margin.min(y_margin) - FRAME_INSET_PIXELS).abs() < 1e-3);
        }
    }

    #[test]
    fn bounds_keep_raised_hands_and_hair_in_frame_through_the_resize_arc() {
        let mut bounds = Bounds::new(false);
        let view_to_world = bounds.world_to_view.transpose();
        let low = Vec3::new(-0.63, -0.68, -0.32);
        let high = Vec3::new(0.41, 1.01, 0.67);
        let mut points = Vec::new();
        for x in [low.x, high.x] {
            for y in [low.y, high.y] {
                for z in [low.z, high.z] {
                    let point = view_to_world * Vec3::new(x, y, z);
                    bounds.include(point);
                    points.push(point);
                }
            }
        }
        let framing = bounds.framing(0.0);
        let observer = Autobs::new(REFERENCE, framing, PIXELS_PER_UNIT);
        for step in 0..=32 {
            let pose = ObserverPose {
                yaw_offset: ARC_LIMIT * (step as f32 / 16.0 - 1.0),
                ..observer.pose(0)
            };
            let (view, projection, _, _) = observer.matrices(pose, REFERENCE);
            for &point in &points {
                let projected = (projection * view).project_point3(point);
                let x_margin = (1.0 - projected.x.abs()) * REFERENCE.0 as f32 * 0.5;
                let y_margin = (1.0 - projected.y.abs()) * REFERENCE.1 as f32 * 0.5;
                assert!(x_margin.min(y_margin) >= FRAME_INSET_PIXELS - 1e-3);
            }
        }
    }

    #[test]
    fn eases_projected_scale_when_the_frame_grows_or_shrinks() {
        let mut observer = observer();
        let parked = observer.pose(0);
        let larger = (1280, 1120);
        observer.retarget(larger, parked, 1);
        assert_eq!(observer.pose(1), parked);
        assert_eq!(observer.duration_ns(), APPROACH_NS);
        let approaching = observer.pose(1 + APPROACH_NS / 2);
        let arrived = observer.pose(1 + APPROACH_NS);
        assert!(parked.zoom < approaching.zoom && approaching.zoom < arrived.zoom);
        assert!(approaching.yaw_offset > 0.0 && approaching.yaw_offset <= ARC_LIMIT);
        assert!(arrived.distance < approaching.distance && approaching.distance < parked.distance);

        let pixels_per_unit = |observer: &Autobs, pose, extent: (u32, u32)| {
            let (_, projection, _, _) = observer.matrices(pose, extent);
            projection.y_axis.y * extent.1 as f32 * 0.5
        };
        let large_scale = pixels_per_unit(&observer, arrived, larger);
        let first_small_scale = pixels_per_unit(&observer, arrived, REFERENCE);
        assert!((first_small_scale - large_scale).abs() < 1e-3);

        let shrink_start = 1 + APPROACH_NS;
        observer.retarget(REFERENCE, arrived, shrink_start);
        assert_eq!(observer.duration_ns(), RECEDE_NS);
        assert_eq!(observer.pose(shrink_start), arrived);
        let receding = observer.pose(shrink_start + RECEDE_NS / 2);
        let settled = observer.pose(shrink_start + RECEDE_NS);
        let receding_scale = pixels_per_unit(&observer, receding, REFERENCE);
        let settled_scale = pixels_per_unit(&observer, settled, REFERENCE);
        assert!(settled_scale < receding_scale && receding_scale < first_small_scale);
        assert_eq!(settled, parked);
    }

    #[test]
    fn keeps_the_heart_drawing_plane_parallel_and_preserves_resize_timing() {
        let mut observer = observer();
        let larger = (960, 840);
        observer.retarget(larger, observer.pose(0), 1);
        let halfway = 1 + APPROACH_NS / 2;
        assert!(observer.set_framing(HEART, halfway));
        assert_eq!(observer.duration_ns(), APPROACH_NS / 2);
        let pose = observer.pose(halfway);
        let (view, projection, _, forward) = observer.matrices(pose, larger);
        assert_eq!(forward, Vec3::Z);
        assert_eq!(pose.yaw_offset, 0.0);
        let center = (projection * view).project_point3(HEART.target);
        assert!(center.x.abs() < 1e-5 && center.y.abs() < 1e-5);
        assert!(!observer.set_framing(HEART, halfway + 1));
        assert!(observer.set_framing(NORMAL, 1 + APPROACH_NS));
        assert_eq!(observer.duration_ns(), 0);
    }

    #[test]
    fn repeated_resizes_keep_the_orbit_inside_its_envelope() {
        let mut observer = observer();
        observer.retarget((1600, 1000), observer.pose(0), 0);
        let visible = observer.pose(200_000_000);
        let second_resize = 340_000_000;
        observer.retarget((500, 400), visible, second_resize);
        assert_eq!(observer.pose(second_resize), visible);
        assert_eq!(observer.duration_ns(), RECEDE_NS);
        for step in 0..=100 {
            let now = second_resize + step * (RECEDE_NS / 100);
            assert!(observer.pose(now).yaw_offset.abs() <= ARC_LIMIT + 1e-6);
        }
        assert_eq!(observer.pose(second_resize + RECEDE_NS).yaw_offset, 0.0);
    }

    #[test]
    fn butterfly_departure_expands_upward_and_keeps_the_bottom_stationary() {
        let start = 50_000_000;
        for framing in [NORMAL, HEART] {
            let mut observer = Autobs::new(REFERENCE, framing, PIXELS_PER_UNIT);
            let parked = observer.pose(start);
            let before = observer.matrices(parked, REFERENCE);
            let clip_to_world = (before.1 * before.0).inverse();
            let bottom = clip_to_world.project_point3(Vec3::new(0.0, -1.0, 0.25));
            let old_top = clip_to_world.project_point3(Vec3::new(0.0, 1.0, 0.25));
            observer.butterfly_departure(parked, start);
            assert_eq!(observer.matrices(observer.pose(start), REFERENCE), before);

            for offset in [
                1,
                DEPARTURE_PULLBACK_NS / 2,
                DEPARTURE_PULLBACK_NS,
                DEPARTURE_HOLD_UNTIL_NS,
                DEPARTURE_HOLD_UNTIL_NS + DEPARTURE_RETURN_NS / 2,
            ] {
                let (view, projection, _, _) =
                    observer.matrices(observer.pose(start + offset), REFERENCE);
                let bottom_clip = (projection * view).project_point3(bottom);
                assert!(
                    (bottom_clip.y + 1.0).abs() < 1e-5,
                    "{offset}: {bottom_clip:?}"
                );
            }

            let expanded = observer.pose(start + DEPARTURE_PULLBACK_NS);
            let (view, projection, eye, forward) = observer.matrices(expanded, REFERENCE);
            assert!(((projection * view).project_point3(old_top).y - 0.6).abs() < 1e-5);
            assert!((projection.x_axis.x / before.1.x_axis.x - 0.8).abs() < 1e-5);
            assert!(
                (eye - before.2 + forward * (parked.distance * DEPARTURE_ROOM)).length() < 1e-5
            );
            let returned = observer.pose(start + DEPARTURE_HOLD_UNTIL_NS + DEPARTURE_RETURN_NS);
            assert_eq!(observer.matrices(returned, REFERENCE), before);
        }
    }

    #[test]
    fn butterfly_departure_composes_with_resize_and_does_not_stack() {
        let mut observer = observer();
        observer.butterfly_departure(observer.pose(0), 0);
        let next = DEPARTURE_HOLD_UNTIL_NS + DEPARTURE_RETURN_NS / 2;
        let visible = observer.pose(next);
        observer.retarget((950, 840), visible, next);
        assert_eq!(observer.pose(next), visible);
        observer.butterfly_departure(visible, next);
        assert_eq!(observer.pose(next), visible);
        assert_eq!(
            observer
                .pose(next + DEPARTURE_PULLBACK_NS)
                .departure_scale(),
            1.25
        );
        let finished = next + DEPARTURE_HOLD_UNTIL_NS + DEPARTURE_RETURN_NS;
        assert_eq!(observer.pose(finished), observer.fitted_pose((950, 840)));
    }
}
