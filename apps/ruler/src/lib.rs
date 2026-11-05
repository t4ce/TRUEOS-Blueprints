//! Physical ruler geometry. All measurements and rotations happen in millimeters;
//! pixels are only the final projection, never the source of the scale.
#![no_std]

extern crate alloc;

#[cfg(test)]
mod tests;

use alloc::{format, string::String, vec::Vec};
use core::f32::consts::{PI, TAU};

pub const BODY_HALF_MM: f32 = 11.0;
const END_MM: f32 = 5.0;
const INCH_MM: f32 = 25.4;
const INK: u32 = color(30, 37, 41, 235);
const MUTED: u32 = color(54, 63, 67, 210);
const ACCENT: u32 = color(0, 108, 112, 255);
const GLASS: u32 = color(183, 190, 194, 82);
const EDGE: u32 = color(94, 105, 110, 140);

pub const fn color(r: u8, g: u8, b: u8, a: u8) -> u32 {
    r as u32 | (g as u32) << 8 | (b as u32) << 16 | (a as u32) << 24
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}

impl Point {
    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Metrics {
    pub px_per_mm_x: f32,
    pub px_per_mm_y: f32,
}

impl Metrics {
    pub fn new(px_per_mm_x: f32, px_per_mm_y: f32) -> Option<Self> {
        (px_per_mm_x.is_finite()
            && px_per_mm_y.is_finite()
            && px_per_mm_x > 0.0
            && px_per_mm_y > 0.0)
            .then_some(Self {
                px_per_mm_x,
                px_per_mm_y,
            })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Straight,
    Circular,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unit {
    Centimeters,
    Millimeters,
    Inches,
}

impl Unit {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Centimeters => "cm",
            Self::Millimeters => "mm",
            Self::Inches => "in",
        }
    }

    fn next(self) -> Self {
        match self {
            Self::Centimeters => Self::Millimeters,
            Self::Millimeters => Self::Inches,
            Self::Inches => Self::Centimeters,
        }
    }

    pub const fn major_mm(self) -> f32 {
        match self {
            Self::Centimeters | Self::Millimeters => 10.0,
            Self::Inches => INCH_MM,
        }
    }

    pub const fn minor_mm(self) -> f32 {
        match self {
            Self::Centimeters | Self::Millimeters => 1.0,
            Self::Inches => INCH_MM / 16.0,
        }
    }

    fn label(self, index: i32) -> String {
        format!(
            "{}",
            if self == Self::Millimeters {
                index * 10
            } else {
                index
            }
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Interaction {
    None,
    Changed,
    Rotate,
    Measure,
}

#[derive(Clone, Copy, Debug)]
pub struct View {
    pub width: u32,
    pub height: u32,
    pub center: Point,
    pub metrics: Metrics,
    pub angle: f32,
}

impl View {
    /// Accept a UI4 backing size without scaling the physical instrument.
    pub fn with_canvas(self, width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            center: Point::new(width as f32 * 0.5, height as f32 * 0.5),
            ..self
        }
    }

    pub fn project(&self, mm: Point) -> Point {
        let (sin, cos) = (libm::sinf(self.angle), libm::cosf(self.angle));
        Point::new(
            self.center.x + (cos * mm.x - sin * mm.y) * self.metrics.px_per_mm_x,
            self.center.y + (sin * mm.x + cos * mm.y) * self.metrics.px_per_mm_y,
        )
    }

    pub fn unproject(&self, pixel: Point) -> Point {
        let x = (pixel.x - self.center.x) / self.metrics.px_per_mm_x;
        let y = (pixel.y - self.center.y) / self.metrics.px_per_mm_y;
        let (sin, cos) = (libm::sinf(self.angle), libm::cosf(self.angle));
        Point::new(cos * x + sin * y, -sin * x + cos * y)
    }
}

#[derive(Clone, Debug)]
pub struct Quad {
    pub corners: [Point; 4],
    pub color: u32,
}

#[derive(Clone, Debug)]
pub struct Label {
    pub text: String,
    pub center: Point,
    pub font_pixels: f32,
    pub color: u32,
}

#[derive(Default)]
pub struct Scene {
    pub quads: Vec<Quad>,
    pub labels: Vec<Label>,
}

#[derive(Clone, Debug)]
pub struct Ruler {
    pub metrics: Metrics,
    pub mode: Mode,
    pub unit: Unit,
    /// Clockwise rotation of the body/reference ray, in radians.
    pub angle: f32,
    pub half_length_mm: f32,
    pub radius_mm: f32,
    /// Offset of zero from the body's center along its long axis.
    pub zero_mm: f32,
    /// Counterclockwise angle relative to the circular tool's reference ray.
    pub measure_angle: f32,
    pub radians: bool,
    max_width: u32,
    max_height: u32,
}

impl Ruler {
    pub fn new(metrics: Metrics, max_width: u32, max_height: u32) -> Self {
        let mut ruler = Self {
            metrics,
            mode: Mode::Straight,
            unit: Unit::Centimeters,
            angle: 0.0,
            half_length_mm: 80.0,
            radius_mm: 40.0,
            zero_mm: 0.0,
            measure_angle: PI / 3.0,
            radians: false,
            max_width,
            max_height,
        };
        ruler.fit_limits(max_width, max_height);
        ruler
    }

    pub fn font_pixels(&self) -> f32 {
        // Size UI text for readability; the calibrated marks never depend on it.
        (self.metrics.px_per_mm_y * 2.8).clamp(11.0, 20.0)
    }

    fn padding(&self) -> f32 {
        self.font_pixels() * 2.6
    }

    fn body_half_mm(&self) -> f32 {
        // On very small/high-density outputs use a thinner body. Only the
        // instrument's housing changes; its millimeter spacing remains exact.
        BODY_HALF_MM.min(self.half_length_mm * (BODY_HALF_MM / 35.0))
    }

    /// Tight bounds around the rotated physical body, including upright labels.
    pub fn view(&self) -> View {
        let (x_mm, y_mm) = match self.mode {
            Mode::Straight => {
                let sin = libm::sinf(self.angle).abs();
                let cos = libm::cosf(self.angle).abs();
                let body = self.body_half_mm();
                (
                    cos * self.half_length_mm + sin * body,
                    sin * self.half_length_mm + cos * body,
                )
            }
            Mode::Circular => {
                let extent = self.radius_mm + 2.0_f32.min(self.radius_mm * 0.1);
                (extent, extent)
            }
        };
        let width =
            (libm::ceilf(2.0 * x_mm * self.metrics.px_per_mm_x + self.padding()) as u32).max(1);
        let height =
            (libm::ceilf(2.0 * y_mm * self.metrics.px_per_mm_y + self.padding()) as u32).max(1);
        View {
            width,
            height,
            center: Point::new(width as f32 * 0.5, height as f32 * 0.5),
            metrics: self.metrics,
            angle: self.angle,
        }
    }

    /// Trim the physical span to fit; never stretch the graduation spacing.
    pub fn fit_limits(&mut self, max_width: u32, max_height: u32) {
        self.max_width = max_width.clamp(64, 2560);
        self.max_height = max_height.clamp(64, 1440);
        let x = ((self.max_width as f32 - self.padding() - 2.0) * 0.5 / self.metrics.px_per_mm_x)
            .max(0.01);
        let y = ((self.max_height as f32 - self.padding() - 2.0) * 0.5 / self.metrics.px_per_mm_y)
            .max(0.01);
        match self.mode {
            Mode::Straight => {
                let sin = libm::sinf(self.angle).abs();
                let cos = libm::cosf(self.angle).abs();
                let body = self.body_half_mm();
                if cos * self.half_length_mm + sin * body > x
                    || sin * self.half_length_mm + cos * body > y
                {
                    let mut low = 0.0;
                    let mut high = self.half_length_mm.min(250.0);
                    for _ in 0..24 {
                        let half = (low + high) * 0.5;
                        let body = BODY_HALF_MM.min(half * (BODY_HALF_MM / 35.0));
                        if cos * half + sin * body <= x && sin * half + cos * body <= y {
                            low = half;
                        } else {
                            high = half;
                        }
                    }
                    self.half_length_mm = low;
                }
                let edge = (self.half_length_mm - END_MM).max(0.0);
                self.zero_mm = self.zero_mm.clamp(-edge, edge);
            }
            Mode::Circular => {
                let extent = x.min(y);
                let radius = if extent < 22.0 {
                    extent / 1.1
                } else {
                    extent - 2.0
                };
                self.radius_mm = self.radius_mm.min(radius);
            }
        }
    }

    pub fn refresh_metrics(&mut self, metrics: Metrics) {
        self.metrics = metrics;
        self.fit_limits(self.max_width, self.max_height);
    }

    pub fn reset(&mut self) {
        self.angle = 0.0;
        self.zero_mm = 0.0;
        self.measure_angle = PI / 3.0;
        self.fit_limits(self.max_width, self.max_height);
    }

    pub fn toggle_mode(&mut self) {
        self.mode = if self.mode == Mode::Straight {
            Mode::Circular
        } else {
            Mode::Straight
        };
        self.fit_limits(self.max_width, self.max_height);
    }

    pub fn cycle_unit(&mut self) {
        self.unit = self.unit.next();
    }

    pub fn toggle_angle_unit(&mut self) {
        self.radians = !self.radians;
    }

    pub fn scroll(&mut self, wheel: i16) -> bool {
        if wheel == 0 {
            return false;
        }
        let previous = (self.radius_mm, self.half_length_mm);
        let step = wheel.clamp(-20, 20) as f32 * 2.5;
        match self.mode {
            Mode::Straight => self.half_length_mm = (self.half_length_mm + step).clamp(35.0, 250.0),
            Mode::Circular => self.radius_mm = (self.radius_mm + step).clamp(18.0, 150.0),
        }
        self.fit_limits(self.max_width, self.max_height);
        previous != (self.radius_mm, self.half_length_mm)
    }

    pub fn set_angle(&mut self, angle: f32, snap: bool) {
        if !angle.is_finite() {
            return;
        }
        self.angle = normalize_signed(if snap {
            libm::roundf(angle / (PI / 12.0)) * (PI / 12.0)
        } else {
            angle
        });
        self.fit_limits(self.max_width, self.max_height);
    }

    pub fn set_measure(&mut self, screen_angle: f32, snap: bool) {
        if !screen_angle.is_finite() {
            return;
        }
        let angle = normalize_positive(self.angle - screen_angle);
        self.measure_angle = normalize_positive(if snap {
            libm::roundf(angle / (PI / 12.0)) * (PI / 12.0)
        } else {
            angle
        });
    }

    pub fn pointer_angle(&self, view: &View, pixel: Point) -> f32 {
        libm::atan2f(
            (pixel.y - view.center.y) / self.metrics.px_per_mm_y,
            (pixel.x - view.center.x) / self.metrics.px_per_mm_x,
        )
    }

    pub fn press(&mut self, view: &View, pixel: Point) -> Interaction {
        let p = view.unproject(pixel);
        match self.mode {
            Mode::Straight => {
                let body = self.body_half_mm();
                if p.x.abs() > self.half_length_mm || p.y.abs() > body {
                    return Interaction::None;
                }
                if self.half_length_mm < 35.0 {
                    if p.x > self.half_length_mm * 0.75 {
                        return Interaction::Rotate;
                    }
                    if self.half_length_mm <= END_MM {
                        return Interaction::None;
                    }
                }
                if self.half_length_mm >= 35.0 && p.y.abs() < 2.8 {
                    if (p.x - (self.half_length_mm - 6.0)).abs() < 4.5 {
                        return Interaction::Rotate;
                    }
                    if (p.x - (-self.half_length_mm + 10.0)).abs() < 6.0 {
                        self.cycle_unit();
                    } else if (p.x - (-self.half_length_mm + 24.0)).abs() < 5.0 {
                        self.toggle_mode();
                    } else if (p.x - (self.half_length_mm - 22.0)).abs() < 7.0 {
                        self.set_angle(0.0, false);
                    } else {
                        self.zero_mm = 0.0;
                    }
                    return Interaction::Changed;
                }
                // Click a graduation to put zero exactly there; both scales agree.
                self.zero_mm =
                    p.x.clamp(-self.half_length_mm + END_MM, self.half_length_mm - END_MM);
                Interaction::Changed
            }
            Mode::Circular => {
                let radius = libm::hypotf(p.x, p.y);
                if radius > self.radius_mm + 2.0 {
                    return Interaction::None;
                }
                // Center controls remain in screen coordinates as the ring rotates.
                let screen = Point::new(
                    (pixel.x - view.center.x) / self.metrics.px_per_mm_x,
                    (pixel.y - view.center.y) / self.metrics.px_per_mm_y,
                );
                if self.radius_mm >= 24.0 {
                    if screen.y.abs() < 3.0 && screen.x.abs() < 12.5 {
                        self.toggle_angle_unit();
                        return Interaction::Changed;
                    }
                    if (screen.y - 7.5).abs() < 3.0 && screen.x.abs() < 8.5 {
                        self.toggle_mode();
                        return Interaction::Changed;
                    }
                    if (screen.y + 7.5).abs() < 3.0 && screen.x.abs() < 12.5 {
                        self.cycle_unit();
                        return Interaction::Changed;
                    }
                }
                if libm::hypotf(p.x - (self.radius_mm - 4.0), p.y) < 3.5 {
                    return Interaction::Rotate;
                }
                if radius > 10.0 {
                    self.set_measure(self.pointer_angle(view, pixel), false);
                    return Interaction::Measure;
                }
                Interaction::None
            }
        }
    }

    pub fn scene(&self, view: &View) -> Scene {
        let mut painter = Painter {
            scene: Scene::default(),
            view,
            font: self.font_pixels(),
        };
        match self.mode {
            Mode::Straight => self.straight(&mut painter),
            Mode::Circular => self.circular(&mut painter),
        }
        painter.scene
    }

    fn straight(&self, p: &mut Painter<'_>) {
        let half = self.half_length_mm;
        let body = self.body_half_mm();
        p.rounded_rect(-half, -body, half, body, 1.5_f32.min(body * 0.5), GLASS);
        if half > 4.0 {
            p.line(
                Point::new(-half + 2.0, -body),
                Point::new(half - 2.0, -body),
                0.22,
                EDGE,
            );
            p.line(
                Point::new(-half + 2.0, body),
                Point::new(half - 2.0, body),
                0.22,
                EDGE,
            );
        }
        self.scale(p, self.unit, -1.0);
        self.scale(
            p,
            if self.unit == Unit::Inches {
                Unit::Centimeters
            } else {
                Unit::Inches
            },
            1.0,
        );

        p.line(
            Point::new(self.zero_mm, -body),
            Point::new(self.zero_mm, body),
            0.25,
            ACCENT,
        );
        p.disc(
            Point::new(self.zero_mm, 0.0),
            0.75_f32.min(body * 0.5),
            ACCENT,
        );

        if half < 35.0 {
            return;
        }

        p.pill(Point::new(-half + 10.0, 0.0), 10.0, 4.5);
        p.label(self.unit.name().into(), Point::new(-half + 10.0, 0.0), INK);
        p.pill(Point::new(-half + 24.0, 0.0), 8.0, 4.5);
        p.label("π".into(), Point::new(-half + 24.0, 0.0), ACCENT);
        p.pill(Point::new(half - 22.0, 0.0), 14.0, 4.5);
        p.label(
            format!("{:.0}°", self.angle * 180.0 / PI),
            Point::new(half - 22.0, 0.0),
            MUTED,
        );
        p.disc(Point::new(half - 6.0, 0.0), 2.2, color(83, 102, 108, 120));
        p.disc(Point::new(half - 6.0, 0.0), 0.65, color(240, 246, 248, 235));
        // The center spine gives the instrument a subtle edge without hiding work.
        if half > 62.0 && libm::sinf(self.angle).abs() < 0.15 {
            p.label("R U L E R".into(), Point::new(-20.0, 0.0), MUTED);
        }
    }

    fn scale(&self, p: &mut Painter<'_>, unit: Unit, side: f32) {
        let span = self.half_length_mm - END_MM;
        if span < 0.0 {
            return;
        }
        let body = self.body_half_mm();
        let minor = unit.minor_mm();
        let divisions = if unit == Unit::Inches { 16 } else { 10 };
        let start = libm::ceilf((-span - self.zero_mm) / minor) as i32;
        let end = libm::floorf((span - self.zero_mm) / minor) as i32;
        for i in start..=end {
            let x = self.zero_mm + i as f32 * minor;
            let major = i % divisions == 0;
            let medium = i % (divisions / 2) == 0;
            let length = (if major {
                4.2
            } else if medium {
                3.0
            } else {
                1.8
            }) * (body / BODY_HALF_MM);
            p.line(
                Point::new(x, side * body),
                Point::new(x, side * (body - length)),
                if major { 0.25 } else { 0.16 },
                if i == 0 { ACCENT } else { INK },
            );
            if major && body * self.metrics.px_per_mm_y > self.font_pixels() {
                let label = unit.label(i / divisions);
                p.label(
                    label,
                    Point::new(x, side * body * (4.7 / BODY_HALF_MM)),
                    if i == 0 { ACCENT } else { INK },
                );
            }
        }
        if self.half_length_mm >= 35.0 {
            p.label(
                unit.name().into(),
                Point::new(-self.half_length_mm + 2.5, side * 6.8),
                MUTED,
            );
        }
    }

    fn circular(&self, p: &mut Painter<'_>) {
        let radius = self.radius_mm;
        p.disc(Point::default(), radius, color(183, 190, 194, 26));
        p.ring((radius - 6.5).max(0.0), radius, GLASS);
        p.arc(radius, 0.0, TAU, 0.22, EDGE);
        if radius > 6.5 {
            p.arc(radius - 6.5, 0.0, TAU, 0.16, EDGE);
        }
        for degree in 0..360 {
            let a = -(degree as f32) * PI / 180.0;
            let major = degree % 30 == 0;
            let medium = degree % 10 == 0;
            // At small radii, 1° marks overlap. Keep 5° marks instead.
            if degree % 5 != 0
                && radius * self.metrics.px_per_mm_x.min(self.metrics.px_per_mm_y) * PI / 180.0
                    < 1.6
            {
                continue;
            }
            let length = (if major {
                4.0_f32
            } else if medium {
                2.8
            } else if degree % 5 == 0 {
                2.0
            } else {
                1.1
            })
            .min(radius * 0.3);
            p.line(
                polar(radius, a),
                polar(radius - length, a),
                if major { 0.25 } else { 0.16 },
                INK,
            );
            let label_step = if radius < 24.0 {
                90
            } else if radius < 32.0 {
                60
            } else {
                30
            };
            if major && radius > 14.0 && degree % label_step == 0 {
                let text = if self.radians {
                    pi_label(degree / 30).into()
                } else {
                    format!("{degree}")
                };
                p.label(
                    text,
                    polar(radius - 9.0, a),
                    if degree == 0 { ACCENT } else { INK },
                );
            }
        }
        if radius < 14.0 {
            return;
        }
        // Scientific convention: positive measured angles turn counterclockwise.
        p.arc(
            radius - 13.0,
            -self.measure_angle,
            0.0,
            0.55,
            color(0, 108, 112, 185),
        );
        p.line(Point::default(), polar(radius - 1.0, 0.0), 0.25, MUTED);
        p.line(
            Point::default(),
            polar(radius - 1.0, -self.measure_angle),
            0.4,
            ACCENT,
        );
        p.disc(polar(radius - 2.0, -self.measure_angle), 1.2, ACCENT);
        p.disc(Point::default(), 0.8, ACCENT);
        p.disc(Point::new(radius - 4.0, 0.0), 2.2, color(83, 102, 108, 180));
        p.disc(
            Point::new(radius - 4.0, 0.0),
            0.65,
            color(240, 246, 248, 235),
        );
        if radius < 24.0 {
            return;
        }
        // Center information stays screen-aligned together with its hit regions.
        let screen_view = View {
            angle: 0.0,
            ..*p.view
        };
        let mut center = Painter {
            scene: Scene::default(),
            view: &screen_view,
            font: p.font,
        };
        let radius_text = match self.unit {
            Unit::Centimeters => format!("r {:.1} cm", radius / 10.0),
            Unit::Millimeters => format!("r {:.0} mm", radius),
            Unit::Inches => format!("r {:.2} in", radius / INCH_MM),
        };
        center.pill(Point::new(0.0, -7.5), 25.0, 5.0);
        center.label(radius_text, Point::new(0.0, -7.5), MUTED);
        center.pill(Point::default(), 25.0, 5.0);
        let text = if self.radians {
            format!("{:.3}π", self.measure_angle / PI)
        } else {
            format!("{:.1}°", self.measure_angle * 180.0 / PI)
        };
        center.label(text, Point::default(), ACCENT);
        center.pill(Point::new(0.0, 7.5), 17.0, 5.0);
        center.label("RULER".into(), Point::new(0.0, 7.5), INK);
        p.scene.quads.extend(center.scene.quads);
        p.scene.labels.extend(center.scene.labels);
    }
}

pub fn normalize_signed(angle: f32) -> f32 {
    normalize_positive(angle + PI) - PI
}

pub fn normalize_positive(angle: f32) -> f32 {
    let result = angle % TAU;
    if result < 0.0 { result + TAU } else { result }
}

fn polar(radius: f32, angle: f32) -> Point {
    Point::new(radius * libm::cosf(angle), radius * libm::sinf(angle))
}

fn pi_label(index: i32) -> &'static str {
    [
        "0", "π/6", "π/3", "π/2", "2π/3", "5π/6", "π", "7π/6", "4π/3", "3π/2", "5π/3", "11π/6",
    ][index as usize]
}

struct Painter<'a> {
    scene: Scene,
    view: &'a View,
    font: f32,
}

impl Painter<'_> {
    fn quad(&mut self, corners: [Point; 4], color: u32) {
        self.scene.quads.push(Quad {
            corners: corners.map(|p| self.view.project(p)),
            color,
        });
    }

    fn rect(&mut self, x0: f32, y0: f32, x1: f32, y1: f32, color: u32) {
        self.quad(
            [
                Point::new(x0, y0),
                Point::new(x1, y0),
                Point::new(x1, y1),
                Point::new(x0, y1),
            ],
            color,
        );
    }

    fn line(&mut self, a: Point, b: Point, width: f32, color: u32) {
        let dx = b.x - a.x;
        let dy = b.y - a.y;
        let length = libm::hypotf(dx, dy);
        if length < 0.0001 {
            return;
        }
        // Ensure a fine mark still covers roughly a pixel on a low-DPI output.
        let width = width.max(
            0.8 / self
                .view
                .metrics
                .px_per_mm_x
                .min(self.view.metrics.px_per_mm_y),
        );
        let nx = -dy / length * width * 0.5;
        let ny = dx / length * width * 0.5;
        self.quad(
            [
                Point::new(a.x + nx, a.y + ny),
                Point::new(b.x + nx, b.y + ny),
                Point::new(b.x - nx, b.y - ny),
                Point::new(a.x - nx, a.y - ny),
            ],
            color,
        );
    }

    fn disc(&mut self, center: Point, radius: f32, color: u32) {
        for i in 0..32 {
            let a = polar(radius, i as f32 * TAU / 32.0);
            let b = polar(radius, (i + 1) as f32 * TAU / 32.0);
            // A duplicated vertex is a triangle in the quad submission.
            self.quad(
                [
                    center,
                    Point::new(center.x + a.x, center.y + a.y),
                    Point::new(center.x + b.x, center.y + b.y),
                    center,
                ],
                color,
            );
        }
    }

    fn ring(&mut self, inner: f32, outer: f32, color: u32) {
        for i in 0..180 {
            let a = i as f32 * TAU / 180.0;
            let b = (i + 1) as f32 * TAU / 180.0;
            self.quad(
                [
                    polar(inner, a),
                    polar(outer, a),
                    polar(outer, b),
                    polar(inner, b),
                ],
                color,
            );
        }
    }

    fn arc(&mut self, radius: f32, from: f32, to: f32, width: f32, color: u32) {
        let steps = (libm::ceilf((to - from).abs() * 40.0) as usize).max(1);
        for i in 0..steps {
            let a = from + (to - from) * i as f32 / steps as f32;
            let b = from + (to - from) * (i + 1) as f32 / steps as f32;
            self.line(polar(radius, a), polar(radius, b), width, color);
        }
    }

    fn rounded_rect(&mut self, x0: f32, y0: f32, x1: f32, y1: f32, r: f32, color: u32) {
        self.rect(x0 + r, y0, x1 - r, y1, color);
        self.rect(x0, y0 + r, x0 + r, y1 - r, color);
        self.rect(x1 - r, y0 + r, x1, y1 - r, color);
        for (cx, cy, start) in [
            (x0 + r, y0 + r, PI),
            (x1 - r, y0 + r, 1.5 * PI),
            (x1 - r, y1 - r, 0.0),
            (x0 + r, y1 - r, PI * 0.5),
        ] {
            let center = Point::new(cx, cy);
            for i in 0..8 {
                let a = start + i as f32 * PI / 16.0;
                let b = start + (i + 1) as f32 * PI / 16.0;
                self.quad(
                    [
                        center,
                        Point::new(cx + r * libm::cosf(a), cy + r * libm::sinf(a)),
                        Point::new(cx + r * libm::cosf(b), cy + r * libm::sinf(b)),
                        center,
                    ],
                    color,
                );
            }
        }
    }

    fn pill(&mut self, center: Point, width: f32, height: f32) {
        self.rounded_rect(
            center.x - width * 0.5,
            center.y - height * 0.5,
            center.x + width * 0.5,
            center.y + height * 0.5,
            height * 0.5,
            color(233, 238, 240, 170),
        );
    }

    fn label(&mut self, text: String, center_mm: Point, color: u32) {
        self.scene.labels.push(Label {
            text,
            center: self.view.project(center_mm),
            font_pixels: self.font,
            color,
        });
    }
}
