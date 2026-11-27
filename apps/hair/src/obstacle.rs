use glam::Vec3;

#[derive(Clone, Copy)]
pub(crate) struct Ellipsoid {
    pub(crate) center: Vec3,
    pub(crate) radii: Vec3,
    inverse_radii: Vec3,
}

impl Ellipsoid {
    const fn new(center: Vec3, radii: Vec3) -> Self {
        Self {
            center,
            radii,
            inverse_radii: Vec3::new(1.0 / radii.x, 1.0 / radii.y, 1.0 / radii.z),
        }
    }

    pub(crate) fn unit_offset(self, offset: Vec3) -> Vec3 {
        offset * self.inverse_radii
    }

    pub(crate) fn contains(self, point: Vec3) -> bool {
        self.unit_offset(point - self.center).length_squared() <= 1.0
    }

    pub(crate) fn intersects(self, from: Vec3, to: Vec3) -> bool {
        let offset = self.unit_offset(from - self.center);
        let step = self.unit_offset(to - from);
        let nearest =
            (-offset.dot(step) / step.length_squared().max(f32::MIN_POSITIVE)).clamp(0.0, 1.0);
        (offset + nearest * step).length_squared() <= 1.0
    }
}

pub(crate) const HEAD_CENTER: Vec3 = Vec3::new(0.0, 1.42, -0.015);
pub(crate) const HEAD_RADIUS: f32 = 0.135;
pub(crate) const HEAD: Ellipsoid = Ellipsoid::new(HEAD_CENTER, Vec3::splat(HEAD_RADIUS));

// Fit to the static AvatarSample_B in metres. The torso follows the coat and
// skirt, and the arm shapes overlap under the loose sleeves and cuffs.
pub(crate) const BODY_OBSTACLES: [Ellipsoid; 11] = [
    Ellipsoid::new(Vec3::new(0.0, 0.785, 0.004), Vec3::new(0.195, 0.170, 0.160)),
    Ellipsoid::new(
        Vec3::new(0.0, 0.955, -0.010),
        Vec3::new(0.169, 0.162, 0.134),
    ),
    Ellipsoid::new(
        Vec3::new(0.0, 1.115, -0.016),
        Vec3::new(0.161, 0.143, 0.134),
    ),
    Ellipsoid::new(Vec3::new(0.0, 1.209, 0.027), Vec3::new(0.170, 0.062, 0.081)),
    Ellipsoid::new(Vec3::new(0.0, 1.274, 0.022), Vec3::new(0.039, 0.053, 0.036)),
    Ellipsoid::new(
        Vec3::new(-0.238, 1.212, 0.011),
        Vec3::new(0.178, 0.066, 0.073),
    ),
    Ellipsoid::new(
        Vec3::new(-0.430, 1.213, 0.016),
        Vec3::new(0.167, 0.066, 0.079),
    ),
    Ellipsoid::new(
        Vec3::new(-0.592, 1.212, 0.016),
        Vec3::new(0.083, 0.052, 0.067),
    ),
    Ellipsoid::new(
        Vec3::new(0.238, 1.212, 0.011),
        Vec3::new(0.178, 0.066, 0.073),
    ),
    Ellipsoid::new(
        Vec3::new(0.430, 1.213, 0.016),
        Vec3::new(0.167, 0.066, 0.079),
    ),
    Ellipsoid::new(
        Vec3::new(0.592, 1.212, 0.016),
        Vec3::new(0.083, 0.052, 0.067),
    ),
];
