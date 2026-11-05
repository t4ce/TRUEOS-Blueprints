use alloc::{
    string::{String, ToString},
    vec::Vec,
};
use core::f32::consts::{PI, TAU};

use crate::{Interaction, Metrics, Mode, Point, Ruler, Scene, Unit, View};

fn near(actual: f32, expected: f32, tolerance: f32) {
    assert!(
        (actual - expected).abs() <= tolerance,
        "expected {expected}, got {actual} (tolerance {tolerance})"
    );
}

fn ruler() -> Ruler {
    Ruler::new(Metrics::new(4.0, 6.0).unwrap(), 2560, 1440)
}

fn physical_distance(view: &View, a: Point, b: Point) -> f32 {
    ((b.x - a.x) / view.metrics.px_per_mm_x).hypot((b.y - a.y) / view.metrics.px_per_mm_y)
}

fn scale_labels(scene: &Scene, view: &View, side: f32) -> Vec<(String, f32)> {
    scene
        .labels
        .iter()
        .filter_map(|label| {
            let local = view.unproject(label.center);
            ((local.y - side * 4.7).abs() < 0.001).then(|| (label.text.clone(), local.x))
        })
        .collect()
}

fn graduation_positions(scene: &Scene, view: &View, side: f32) -> Vec<f32> {
    let mut positions = Vec::new();
    for quad in &scene.quads {
        let corners = quad.corners.map(|point| view.unproject(point));
        let min_x = corners.iter().map(|p| p.x).fold(f32::INFINITY, f32::min);
        let max_x = corners
            .iter()
            .map(|p| p.x)
            .fold(f32::NEG_INFINITY, f32::max);
        let min_y = corners
            .iter()
            .map(|p| side * p.y)
            .fold(f32::INFINITY, f32::min);
        let max_y = corners
            .iter()
            .map(|p| side * p.y)
            .fold(f32::NEG_INFINITY, f32::max);
        if min_y > 6.75 && max_y <= 11.01 && max_y - min_y > 1.7 && max_x - min_x < 0.5 {
            positions.push((min_x + max_x) * 0.5);
        }
    }
    positions.sort_by(f32::total_cmp);
    positions
}

fn assert_scene_inside(ruler: &Ruler) {
    let view = ruler.view();
    let scene = ruler.scene(&view);
    assert!(!scene.quads.is_empty());
    // UI4's normal SpriteQuad submission limit is 8,192.
    assert!(scene.quads.len() < 8192, "{} quads", scene.quads.len());
    for point in scene.quads.iter().flat_map(|quad| quad.corners) {
        assert!(point.x.is_finite() && point.y.is_finite());
        assert!(
            point.x >= -0.01
                && point.y >= -0.01
                && point.x <= view.width as f32 + 0.01
                && point.y <= view.height as f32 + 0.01,
            "clipped quad point {point:?}, {}×{}, {:?}, angle {}, half {}, radius {}",
            view.width,
            view.height,
            ruler.mode,
            ruler.angle,
            ruler.half_length_mm,
            ruler.radius_mm
        );
    }
    for label in &scene.labels {
        assert!(label.center.x.is_finite() && label.center.y.is_finite());
        assert!(label.font_pixels.is_finite() && label.font_pixels > 0.0);
        assert!(label.center.x >= 0.0 && label.center.x <= view.width as f32);
        assert!(label.center.y >= 0.0 && label.center.y <= view.height as f32);
    }
    assert!(
        scene.quads.iter().any(|quad| {
            let alpha = quad.color >> 24;
            alpha > 0 && alpha < 255
        }),
        "instrument body should remain transparent"
    );
}

#[test]
fn metrics_reject_invalid_physical_calibration() {
    for value in [0.0, -1.0, f32::INFINITY, f32::NEG_INFINITY, f32::NAN] {
        assert!(Metrics::new(value, 4.0).is_none());
        assert!(Metrics::new(4.0, value).is_none());
    }
    assert_eq!(Metrics::new(3.0, 5.0).unwrap().px_per_mm_y, 5.0);
}

#[test]
fn centimeter_stays_ten_physical_millimeters_under_anisotropic_rotation() {
    let mut ruler = ruler();
    for angle in [0.0, PI / 7.0, PI / 4.0, PI / 2.0, -2.7, PI] {
        ruler.set_angle(angle, false);
        let view = ruler.view();
        let origin = view.project(Point::new(0.0, 0.0));
        let centimeter = view.project(Point::new(10.0, 0.0));
        near(physical_distance(&view, origin, centimeter), 10.0, 0.0001);
        for point in [Point::new(-43.25, -11.0), Point::new(19.125, 6.0)] {
            let restored = view.unproject(view.project(point));
            near(restored.x, point.x, 0.0001);
            near(restored.y, point.y, 0.0001);
        }
        let scene = ruler.scene(&view);
        let labels = scale_labels(&scene, &view, -1.0);
        let zero = labels.iter().find(|(text, _)| text == "0").unwrap().1;
        let one = labels.iter().find(|(text, _)| text == "1").unwrap().1;
        near(one - zero, 10.0, 0.0001);
    }
}

#[test]
fn inch_graduations_are_exact_sixteenths_without_cumulative_drift() {
    let mut ruler = Ruler::new(Metrics::new(2.0, 3.0).unwrap(), 2560, 1440);
    ruler.half_length_mm = 240.0;
    ruler.unit = Unit::Inches;
    ruler.set_angle(0.37, false);
    let view = ruler.view();
    let scene = ruler.scene(&view);
    let positions = graduation_positions(&scene, &view, -1.0);
    assert_eq!(positions.len(), 297);
    for (index, &position) in positions.iter().enumerate() {
        near(
            position,
            (index as i32 - 148) as f32 * (25.4 / 16.0),
            0.00015,
        );
    }
    let labels = scale_labels(&scene, &view, -1.0);
    for integer in -9_i32..=9 {
        let x = labels
            .iter()
            .find(|(text, _)| *text == integer.to_string())
            .unwrap()
            .1;
        near(x, integer as f32 * 25.4, 0.00015);
    }
}

#[test]
fn clicking_graduations_relocates_zero_and_keeps_both_signed_scales_aligned() {
    let mut ruler = ruler();
    ruler.set_angle(0.71, false);
    for zero in [-17.25, 23.125] {
        let view = ruler.view();
        let click = view.project(Point::new(zero, -9.0));
        assert_eq!(ruler.press(&view, click), Interaction::Changed);
        near(ruler.zero_mm, zero, 0.0001);
        let scene = ruler.scene(&view);
        for (side, step) in [(-1.0, 10.0), (1.0, 25.4)] {
            let labels = scale_labels(&scene, &view, side);
            for integer in -1_i32..=1 {
                let x = labels
                    .iter()
                    .find(|(text, _)| *text == integer.to_string())
                    .unwrap()
                    .1;
                near(x, zero + integer as f32 * step, 0.0001);
            }
        }
    }
    let view = ruler.view();
    let outside = view.project(Point::new(ruler.half_length_mm + 1.0, -9.0));
    assert_eq!(ruler.press(&view, outside), Interaction::None);
    near(ruler.zero_mm, 23.125, 0.0001);
}

#[test]
fn unit_clicks_preserve_physical_geometry_and_user_origin() {
    let mut ruler = ruler();
    ruler.zero_mm = -13.5;
    ruler.set_angle(0.45, false);
    let geometry = (ruler.angle, ruler.half_length_mm, ruler.zero_mm);
    for expected in [Unit::Millimeters, Unit::Inches, Unit::Centimeters] {
        let view = ruler.view();
        let button = view.project(Point::new(-ruler.half_length_mm + 10.0, 0.0));
        assert_eq!(ruler.press(&view, button), Interaction::Changed);
        assert_eq!(ruler.unit, expected);
        assert_eq!((ruler.angle, ruler.half_length_mm, ruler.zero_mm), geometry);
    }
    ruler.toggle_mode();
    let radius = ruler.radius_mm;
    ruler.measure_angle = 1.23;
    let view = ruler.view();
    assert_eq!(ruler.press(&view, view.center), Interaction::Changed);
    assert!(ruler.radians);
    assert_eq!(ruler.measure_angle, 1.23);
    assert_eq!(ruler.radius_mm, radius);
}

#[test]
fn pointer_angles_compensate_for_unequal_axis_dpi() {
    let mut ruler = ruler();
    ruler.toggle_mode();
    ruler.set_angle(0.7, false);
    let view = ruler.view();
    for angle in [-2.8_f32, -PI / 2.0, 0.0, PI / 4.0, 2.8] {
        let pixel = Point::new(
            view.center.x + 25.0 * angle.cos() * ruler.metrics.px_per_mm_x,
            view.center.y + 25.0 * angle.sin() * ruler.metrics.px_per_mm_y,
        );
        near(ruler.pointer_angle(&view, pixel), angle, 0.0001);
    }
}

#[test]
fn circular_measurement_wraps_counterclockwise_and_shift_snaps_to_fifteen_degrees() {
    let mut ruler = ruler();
    ruler.toggle_mode();
    ruler.set_angle(-170.0_f32.to_radians(), false);
    ruler.set_measure(170.0_f32.to_radians(), false);
    near(ruler.measure_angle.to_degrees(), 20.0, 0.0001);
    ruler.set_measure(ruler.angle - 31.0_f32.to_radians(), true);
    near(ruler.measure_angle.to_degrees(), 30.0, 0.0001);
    ruler.set_measure(ruler.angle + 1.0_f32.to_radians(), true);
    near(ruler.measure_angle, 0.0, 0.0001);
    for (input, expected) in [
        (22.0_f32, 15.0_f32),
        (23.0, 30.0),
        (-22.0, -15.0),
        (383.0, 30.0),
    ] {
        ruler.set_angle(input.to_radians(), true);
        near(ruler.angle.to_degrees(), expected, 0.0001);
    }
    let before = (ruler.angle, ruler.measure_angle);
    ruler.set_angle(f32::NAN, true);
    ruler.set_measure(f32::INFINITY, true);
    assert_eq!((ruler.angle, ruler.measure_angle), before);
}

#[test]
fn rotate_handles_and_circular_controls_follow_their_visible_positions() {
    let mut ruler = ruler();
    ruler.set_angle(-1.1, false);
    let view = ruler.view();
    let handle = view.project(Point::new(ruler.half_length_mm - 6.0, 0.0));
    assert_eq!(ruler.press(&view, handle), Interaction::Rotate);
    ruler.toggle_mode();
    let view = ruler.view();
    let handle = view.project(Point::new(ruler.radius_mm - 4.0, 0.0));
    assert_eq!(ruler.press(&view, handle), Interaction::Rotate);
    let unit = Point::new(
        view.center.x,
        view.center.y - 7.5 * ruler.metrics.px_per_mm_y,
    );
    assert_eq!(ruler.press(&view, unit), Interaction::Changed);
    assert_eq!(ruler.unit, Unit::Millimeters);
    let ring = view.project(Point::new(0.0, -ruler.radius_mm));
    assert_eq!(ruler.press(&view, ring), Interaction::Measure);
    near(ruler.measure_angle, PI / 2.0, 0.0001);
}

#[test]
fn wheel_changes_physical_radius_without_rescaling_millimeters() {
    let mut ruler = ruler();
    ruler.toggle_mode();
    let metrics = ruler.metrics;
    let initial_radius = ruler.radius_mm;
    assert!(!ruler.scroll(0));
    assert!(ruler.scroll(1));
    near(ruler.radius_mm - initial_radius, 2.5, 0.00001);
    assert_eq!(ruler.metrics, metrics);
    let view = ruler.view();
    near(
        physical_distance(
            &view,
            view.center,
            view.project(Point::new(ruler.radius_mm, 0.0)),
        ),
        initial_radius + 2.5,
        0.0001,
    );
    assert!(ruler.scroll(-1));
    near(ruler.radius_mm, initial_radius, 0.00001);
    ruler.toggle_mode();
    let half = ruler.half_length_mm;
    assert!(ruler.scroll(1));
    near(ruler.half_length_mm - half, 2.5, 0.00001);
    assert_eq!(ruler.metrics, metrics);
}

#[test]
fn rotations_fit_available_desktop_without_clipping_or_exceeding_quad_limit() {
    for (width, height, px_x, px_y) in [
        (320, 240, 3.0, 4.0),
        (640, 480, 8.0, 8.0),
        (1280, 720, 12.0, 15.0),
        (2560, 1440, 1.0, 1.5),
        (2560, 1440, 7.0, 4.0),
    ] {
        for mode in [Mode::Straight, Mode::Circular] {
            for degrees in (0..360).step_by(15) {
                let mut ruler = Ruler::new(Metrics::new(px_x, px_y).unwrap(), width, height);
                if mode == Mode::Circular {
                    ruler.toggle_mode();
                }
                ruler.set_angle((degrees as f32).to_radians(), false);
                let view = ruler.view();
                assert!(view.width <= width, "{} > {width} px", view.width);
                assert!(view.height <= height, "{} > {height} px", view.height);
                assert_scene_inside(&ruler);
            }
        }
    }
}

#[test]
fn largest_scales_and_full_circular_measurement_remain_within_sprite_budget() {
    let mut ruler = Ruler::new(Metrics::new(2.0, 2.0).unwrap(), 2560, 1440);
    ruler.half_length_mm = 250.0;
    ruler.fit_limits(2560, 1440);
    for unit in [Unit::Centimeters, Unit::Millimeters, Unit::Inches] {
        ruler.unit = unit;
        ruler.zero_mm = 200.0;
        assert_scene_inside(&ruler);
    }
    ruler.toggle_mode();
    ruler.radius_mm = 150.0;
    ruler.measure_angle = TAU - 0.0001;
    ruler.set_angle(0.67, false);
    for radians in [false, true] {
        ruler.radians = radians;
        assert_scene_inside(&ruler);
    }
}

#[test]
fn compact_high_dpi_desktop_still_respects_frame_limits() {
    for (width, height) in [(320, 200), (64, 64)] {
        for mode in [Mode::Straight, Mode::Circular] {
            let mut ruler = Ruler::new(Metrics::new(8.0, 8.0).unwrap(), width, height);
            if mode == Mode::Circular {
                ruler.toggle_mode();
            }
            for degrees in [0.0_f32, 45.0, 90.0] {
                ruler.set_angle(degrees.to_radians(), false);
                let view = ruler.view();
                assert!(
                    view.width <= width && view.height <= height,
                    "{mode:?} {degrees}° produced {}×{} on {width}×{height} desktop",
                    view.width,
                    view.height
                );
                assert_scene_inside(&ruler);
            }
        }
    }
}
