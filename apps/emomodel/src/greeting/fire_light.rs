use glam::{Vec2, Vec3};

use super::sky_light::linear_rgb;

const MASK_SIZE: usize = 64;
const MASK_INDEX: usize = MASK_SIZE - 1;
const GRID_CELLS: usize = 4 * 4 + 8 * 8 + 16 * 16;
const MASK_SCALE: f32 = 1.36;
const MASK_CONTRAST: f32 = 0.10;
const STUDY_STRENGTH: f32 = 1.75 * 0.74;
const TORSO_REFERENCE: Vec3 = Vec3::new(0.0, 0.90, 0.0);
// The butterfly moves the origin through the world while this axis keeps the
// mask orientation stable when the camera or butterfly changes direction.
const MASK_ACROSS: Vec3 = Vec3::new(0.8, 0.0, -0.6);
const MASK: [u8; MASK_SIZE * MASK_SIZE] = bake_mask();

pub(super) struct FireLight {
    position: Vec3,
    view_back: Vec3,
    broad_offset: Vec2,
    detail_offset: Vec2,
    torso_falloff: f32,
    ember: Vec3,
    flare: Vec3,
}

pub(super) struct FireSample {
    incoming: Vec3,
    half_vector: Vec3,
    light: Vec3,
}

impl FireLight {
    pub(super) fn new(
        position: Vec3,
        view_back: Vec3,
        phase: f32,
        ember: [u8; 3],
        flare: [u8; 3],
    ) -> Self {
        let phase = phase.rem_euclid(1.0);
        Self {
            position,
            view_back: view_back.normalize_or_zero(),
            broad_offset: Vec2::new(0.0, -2.0 * phase),
            detail_offset: Vec2::new(0.39, 0.48 - 3.0 * phase),
            torso_falloff: falloff_denominator((position - TORSO_REFERENCE).length_squared()),
            ember: linear_rgb(ember),
            flare: linear_rgb(flare),
        }
    }

    pub(super) fn at(&self, position: Vec3) -> FireSample {
        let outgoing = position - self.position;
        let distance_squared = outgoing.length_squared();
        let incoming = -outgoing * distance_squared.max(0.0001).sqrt().recip();
        // The flight envelope handles overall distance. This ratio shapes
        // local highlights while keeping the torso's strength predictable.
        let falloff =
            (self.torso_falloff / falloff_denominator(distance_squared)).clamp(0.75, 1.25);
        let p = Vec2::new(outgoing.dot(MASK_ACROSS), outgoing.y) * MASK_SCALE;
        let broad = sample_mask(p * Vec2::new(0.86, 0.45) + self.broad_offset);
        let detail = sample_mask(p * Vec2::new(1.71, 0.69) + self.detail_offset);
        let density = 0.71 * broad + 0.29 * detail;
        let alpha = (0.5 + (density - 0.5) * (1.0 + 3.0 * MASK_CONTRAST)).clamp(0.0, 1.0);
        let energy = (0.12 + 0.88 * alpha) * STUDY_STRENGTH * falloff;
        FireSample {
            incoming,
            half_vector: (incoming + self.view_back).normalize_or_zero(),
            light: self.heat_color(alpha) * energy,
        }
    }

    fn heat_color(&self, alpha: f32) -> Vec3 {
        let heat = ((alpha - 0.25) / (0.86 - 0.25)).clamp(0.0, 1.0);
        self.ember
            .lerp(self.flare, heat * heat * (3.0 - 2.0 * heat))
    }
}

impl FireSample {
    /// Linear RGB reflected toward the observer from this one direct light.
    /// Animated skinning supplies unit normals.
    pub(super) fn direct(&self, normal: Vec3, skin_linear: Vec3) -> Vec3 {
        let facing = normal.dot(self.incoming).clamp(-1.0, 1.0);
        let wrap = (facing + 0.22).max(0.0) / 1.22;
        let diffuse = 0.28 * wrap + 0.72 * facing.max(0.0);
        let h = normal.dot(self.half_vector).clamp(0.0, 1.0);
        let h2 = h * h;
        let h4 = h2 * h2;
        self.light * diffuse * (skin_linear + 0.10 * h4 * h4)
    }
}

fn falloff_denominator(distance_squared: f32) -> f32 {
    0.58 + 0.56 * distance_squared
}

fn sample_mask(uv: Vec2) -> f32 {
    let point = uv * MASK_SIZE as f32 - Vec2::splat(0.5);
    let pixel = point.floor();
    let fraction = point - pixel;
    let x = pixel.x as i32 as usize & MASK_INDEX;
    let y = pixel.y as i32 as usize & MASK_INDEX;
    let next_x = (x + 1) & MASK_INDEX;
    let next_y = (y + 1) & MASK_INDEX;
    let pixel = |x, y| f32::from(MASK[y * MASK_SIZE + x]);
    let top = lerp(pixel(x, y), pixel(next_x, y), fraction.x);
    let bottom = lerp(pixel(x, next_y), pixel(next_x, next_y), fraction.x);
    lerp(top, bottom, fraction.y) * (1.0 / 255.0)
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

// Match the seed, wrapping and lattice weights in studies/sky/fire.js.
const fn bake_mask() -> [u8; MASK_SIZE * MASK_SIZE] {
    let mut seed = 0xd3a9_3278_u32;
    let mut cells = [0.0_f32; GRID_CELLS];
    let mut i = 0;
    while i < cells.len() {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        cells[i] = (seed as f64 / 4_294_967_296.0) as f32;
        i += 1;
    }
    let mut mask = [0; MASK_SIZE * MASK_SIZE];
    i = 0;
    while i < mask.len() {
        let x = i % MASK_SIZE;
        let y = i / MASK_SIZE;
        let broad = grid_value(&cells, 0, 4, x, y);
        let middle = grid_value(&cells, 4 * 4, 8, x, y);
        let detail = grid_value(&cells, 4 * 4 + 8 * 8, 16, x, y);
        mask[i] = ((0.60 * broad + 0.28 * middle + 0.12 * detail) * 255.0 + 0.5) as u8;
        i += 1;
    }
    mask
}

const fn grid_value(
    cells: &[f32; GRID_CELLS],
    offset: usize,
    width: usize,
    x: usize,
    y: usize,
) -> f64 {
    let u = x * width;
    let v = y * width;
    let left = u / MASK_SIZE;
    let top = v / MASK_SIZE;
    let right = (left + 1) % width;
    let bottom = (top + 1) % width;
    let tx = cubic(u % MASK_SIZE);
    let ty = cubic(v % MASK_SIZE);
    let a = cells[offset + top * width + left] as f64;
    let b = cells[offset + top * width + right] as f64;
    let c = cells[offset + bottom * width + left] as f64;
    let d = cells[offset + bottom * width + right] as f64;
    let top = a + (b - a) * tx;
    let bottom = c + (d - c) * tx;
    top + (bottom - top) * ty
}

const fn cubic(subpixel: usize) -> f64 {
    let t = subpixel as f64 / MASK_SIZE as f64;
    t * t * (3.0 - 2.0 * t)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn moving_mask_repeats_without_a_phase_jump() {
        let emitter = Vec3::new(0.8, 0.9, 0.6);
        let surface = Vec3::new(0.0, 0.65, 0.1);
        let normal = (emitter - surface).normalize();
        let shade = |phase| {
            FireLight::new(emitter, Vec3::Z, phase, [245, 130, 40], [255, 234, 195])
                .at(surface)
                .direct(normal, Vec3::splat(0.5))
        };
        assert!((shade(0.2) - shade(1.2)).length() < 0.00001);
        assert!((shade(0.2) - shade(0.5)).length() > 0.02);
        let uv = Vec2::new(-0.032, 0.71);
        assert!((sample_mask(uv) - sample_mask(uv + Vec2::ONE)).abs() < 0.00001);
    }

    #[test]
    fn direction_and_tint_follow_the_emitter() {
        let surface = Vec3::new(0.0, 1.0, 0.0);
        let light = FireLight::new(
            Vec3::X + surface,
            Vec3::Z,
            0.3,
            [32, 96, 250],
            [70, 150, 255],
        );
        let hit = light.at(surface);
        let lit = hit.direct(Vec3::X, Vec3::splat(0.5));
        assert!(lit.z > lit.y && lit.y > lit.x);
        assert_eq!(hit.direct(Vec3::NEG_X, Vec3::ONE), Vec3::ZERO);
        assert!(
            light
                .at(Vec3::X + surface)
                .direct(Vec3::X, Vec3::ONE)
                .is_finite()
        );
    }
}
