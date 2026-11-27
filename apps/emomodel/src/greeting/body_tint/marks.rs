use glam::{Mat2, Vec2, Vec3};

use super::{BODY_MARKS, Mask};

const SMALL: f32 = 42.0;
const LARGE: f32 = 84.0;

#[derive(Clone, Copy)]
enum Kind {
    Spot,
    Brown,
    Pink,
    Pale,
}

pub(super) struct Placement {
    kind: Kind,
    center: Vec2,
    // UV pixels per millimeter along the bind surface's horizontal and vertical axes.
    uv_per_mm: Mat2,
    diameter: f32,
}

impl Placement {
    const fn new(
        kind: Kind,
        diameter: f32,
        center: [f32; 2],
        [du_h, du_v, dv_h, dv_v]: [f32; 4],
    ) -> Self {
        Self {
            kind,
            center: Vec2::from_array(center),
            uv_per_mm: Mat2::from_cols(Vec2::new(du_h, dv_h), Vec2::new(du_v, dv_v)),
            diameter,
        }
    }

    pub(super) fn mask(&self) -> Mask {
        let (offset, attenuation) = match self.kind {
            Kind::Spot => ([0, 0], Vec3::new(0.49, 0.55, 0.60)),
            Kind::Brown => ([32, 0], Vec3::new(0.16, 0.23, 0.28)),
            Kind::Pink => ([0, 32], Vec3::new(0.01, 0.16, 0.10)),
            Kind::Pale => ([32, 32], Vec3::new(-0.04, -0.13, -0.18)),
        };
        let axes = self.uv_per_mm * self.diameter;
        let span = axes.x_axis.abs() + axes.y_axis.abs();
        Mask {
            origin: self.center - span * 0.5,
            span,
            offset,
            size: [32, 32],
            atlas: BODY_MARKS,
            uv_to_mark: Some(axes.inverse()),
            mirror_u: false,
            attenuation,
        }
    }
}

pub(super) const PLACEMENTS: [Placement; 9] = [
    // The same beauty spot on the left forearm, right upper arm, and left thigh.
    Placement::new(
        Kind::Spot,
        2.2,
        [425.415, 679.358],
        [-0.0034, 0.7079, -0.5388, -0.0099],
    ),
    Placement::new(
        Kind::Spot,
        3.4,
        [52.119, 623.928],
        [-0.0326, 0.7795, -0.5220, -0.0344],
    ),
    Placement::new(
        Kind::Spot,
        5.0,
        [426.235, 811.010],
        [-0.3531, -0.0103, 0.0087, -0.2979],
    ),
    // The large brown mark is on the front of the right shin below the knee.
    Placement::new(
        Kind::Brown,
        SMALL,
        [465.915, 944.625],
        [-0.4678, -0.0081, -0.0002, -0.3375],
    ),
    Placement::new(
        Kind::Brown,
        LARGE,
        [93.089, 915.822],
        [-0.5487, -0.0083, -0.0105, -0.3980],
    ),
    Placement::new(
        Kind::Pink,
        SMALL,
        [78.965, 816.377],
        [-0.3658, -0.0096, -0.0109, -0.3086],
    ),
    Placement::new(
        Kind::Pink,
        LARGE,
        [34.245, 827.276],
        [-0.4376, 0.0014, 0.0231, -0.3360],
    ),
    Placement::new(
        Kind::Pale,
        SMALL,
        [83.080, 700.372],
        [-0.0491, -0.7826, 0.5045, -0.0579],
    ),
    Placement::new(
        Kind::Pale,
        LARGE,
        [477.713, 840.467],
        [-0.4975, 0.0032, -0.0292, -0.3693],
    ),
];
