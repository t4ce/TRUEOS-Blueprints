use std::{cmp::Reverse, ops::Range};

use anyhow::{Context, Result};
use glam::{Vec2, Vec3};

use super::floor_material::{self, Palette};

pub(super) const FEET: usize = 4;
pub(super) const FINGERTIPS: usize = 10;
pub(super) const CONTACTS: usize = FEET + FINGERTIPS;
const FOOT_RADIUS: f32 = 0.135;
const FINGERTIP_RADIUS: f32 = 0.090;
const CONTACT_EXPONENT: f32 = 1.3;
const TEXEL_METERS: f32 = 0.0025;
const BRUSH_SIZE: usize = 64;
const BRUSH: &[u8; BRUSH_SIZE * BRUSH_SIZE] = include_bytes!("../../assets/foot-contact-brush.r8");

#[derive(Clone, Copy)]
struct Brush {
    center: Vec2,
    radius: f32,
    opacity: f32,
}

impl Brush {
    fn new(point: Vec3, reach: f32, max_radius: f32, ground_y: f32) -> Option<Self> {
        let proximity = (1.0 - (point.y - ground_y) / reach).clamp(0.0, 1.0);
        if !point.is_finite() || proximity <= 0.0 {
            return None;
        }
        let response =
            ((CONTACT_EXPONENT * proximity).exp_m1() / CONTACT_EXPONENT.exp_m1()).clamp(0.0, 1.0);
        let radius = max_radius * response;
        if !radius.is_finite() || radius <= 0.0 {
            return None;
        }
        let raster_radius = radius.max(2.0 * TEXEL_METERS);
        Some(Self {
            center: Vec2::new(point.x, point.z),
            radius: raster_radius,
            // Keep a few samples across a subpixel mark and conserve its coverage.
            opacity: (radius / raster_radius).powi(2),
        })
    }

    fn alpha(self, position: Vec2) -> f32 {
        let uv = (position - self.center) / (2.0 * self.radius) + Vec2::splat(0.5);
        if uv.min_element() <= 0.0 || uv.max_element() >= 1.0 {
            return 0.0;
        }
        let pixel = uv * (BRUSH_SIZE - 1) as f32;
        let x = pixel.x as usize;
        let y = pixel.y as usize;
        let right = (x + 1).min(BRUSH_SIZE - 1);
        let bottom = (y + 1).min(BRUSH_SIZE - 1);
        let texel = |x, y| f32::from(BRUSH[y * BRUSH_SIZE + x]);
        let along_x = |row| {
            let left = texel(x, row);
            left + (texel(right, row) - left) * pixel.x.fract()
        };
        let top = along_x(y);
        (top + (along_x(bottom) - top) * pixel.y.fract()) * (self.opacity / 255.0)
    }
}

struct Patch {
    low: Vec2,
    high: Vec2,
    brushes: Vec<Brush>,
    size: [usize; 2],
    origin: [usize; 2],
}

impl Patch {
    fn new(brush: Brush) -> Self {
        let radius = Vec2::splat(brush.radius);
        // Use a fixed world-space lattice. When two bounds merge, other brush
        // samples stay at the same coordinates.
        Self {
            low: ((brush.center - radius) / TEXEL_METERS).floor() - Vec2::ONE,
            high: ((brush.center + radius) / TEXEL_METERS).ceil() + Vec2::ONE,
            brushes: vec![brush],
            size: [0; 2],
            origin: [0; 2],
        }
    }

    fn overlaps(&self, other: &Self) -> bool {
        self.low.cmple(other.high).all() && other.low.cmple(self.high).all()
    }

    fn merge(&mut self, other: Self) {
        self.low = self.low.min(other.low);
        self.high = self.high.max(other.high);
        self.brushes.extend(other.brushes);
    }

    fn alpha(&self, position: Vec2) -> f32 {
        1.0 - self
            .brushes
            .iter()
            .fold(1.0, |light, brush| light * (1.0 - brush.alpha(position)))
    }

    fn write(&self, palette: &mut Palette, colors: &[[u8; 3]; 256]) {
        let [width, height] = self.size;
        let [x, y] = self.origin;
        // The outer texels and the gutter match the UI4 background exactly.
        for row in 1..height - 1 {
            for col in 1..width - 1 {
                let position = (self.low + Vec2::new(col as f32, row as f32)) * TEXEL_METERS;
                let alpha = (255.0 * self.alpha(position)).round() as usize;
                palette.set(x + col, y + row, colors[alpha]);
            }
        }
    }
}

pub(super) struct Atlas {
    frames: Vec<Range<usize>>,
    patches: Vec<Patch>,
}

impl Atlas {
    pub(super) fn new(poses: usize) -> Self {
        Self {
            frames: Vec::with_capacity(poses),
            patches: Vec::new(),
        }
    }

    pub(super) fn clear(&mut self) {
        self.frames.clear();
        self.patches.clear();
    }

    pub(super) fn push(
        &mut self,
        ground_y: f32,
        feet: [(Vec3, f32); FEET],
        fingertips: [(Vec3, f32); FINGERTIPS],
    ) {
        let start = self.patches.len();
        for ((point, reach), radius) in feet
            .into_iter()
            .map(|contact| (contact, FOOT_RADIUS))
            .chain(fingertips.map(|contact| (contact, FINGERTIP_RADIUS)))
        {
            let Some(brush) = Brush::new(point, reach, radius, ground_y) else {
                continue;
            };
            let mut patch = Patch::new(brush);
            // Merge rectangles as well as brushes so no drawn quads can overlap.
            while let Some(index) = self.patches[start..]
                .iter()
                .position(|other| patch.overlaps(other))
            {
                patch.merge(self.patches.swap_remove(start + index));
            }
            self.patches.push(patch);
        }
        for patch in &mut self.patches[start..] {
            patch.size = (patch.high - patch.low)
                .to_array()
                .map(|texels| texels as usize + 1);
        }
        self.frames.push(start..self.patches.len());
    }

    #[cfg(test)]
    pub(super) fn bake(&mut self) -> Result<Palette> {
        self.bake_rows(&[512, 1024, 2048], 4096, floor_material::SWATCH_ROWS)
    }

    pub(super) fn bake_rows(
        &mut self,
        widths: &[usize],
        max_height: usize,
        reserved_rows: usize,
    ) -> Result<Palette> {
        let mut sorted: Vec<_> = (0..self.patches.len()).collect();
        sorted.sort_unstable_by_key(|&i| Reverse(self.patches[i].size[1]));
        let (width, height) = widths
            .iter()
            .find_map(|&width| {
                let height = self.pack(&sorted, width, reserved_rows)?;
                (height <= max_height).then_some((width, height))
            })
            .with_context(|| {
                let largest = sorted.first().map(|&index| self.patches[index].size);
                format!(
                    "ground contact masks exceed the atlas extent: patches={} tallest={largest:?}",
                    sorted.len()
                )
            })?;
        let mut image = Palette::new(width, height);
        let colors = floor_material::shadow_colors();
        for patch in &self.patches {
            patch.write(&mut image, &colors);
        }
        Ok(image)
    }

    fn pack(&mut self, sorted: &[usize], width: usize, reserved_rows: usize) -> Option<usize> {
        let (mut x, mut y, mut row_height) = (0, reserved_rows, 0);
        for &index in sorted {
            let patch = &mut self.patches[index];
            // Reserve one clear texel on every side, even between poses.
            let [w, h] = patch.size.map(|extent| extent + 2);
            if w > width {
                return None;
            }
            if x + w > width {
                x = 0;
                y += row_height;
                row_height = 0;
            }
            patch.origin = [x + 1, y + 1];
            x += w;
            row_height = row_height.max(h);
        }
        Some(y + row_height)
    }

    pub(super) fn quads(
        &self,
        frame: usize,
        ground_y: f32,
        palette: &Palette,
    ) -> impl Iterator<Item = ([[f32; 3]; 4], [[f32; 2]; 4])> {
        self.patches[self.frames[frame].clone()]
            .iter()
            .map(move |patch| {
                let Vec2 { x: x0, y: z0 } = patch.low * TEXEL_METERS;
                let Vec2 { x: x1, y: z1 } = patch.high * TEXEL_METERS;
                let [u0, v0] = patch.origin;
                let [u1, v1] = [u0 + patch.size[0] - 1, v0 + patch.size[1] - 1];
                (
                    [
                        [x0, ground_y, z0],
                        [x0, ground_y, z1],
                        [x1, ground_y, z0],
                        [x1, ground_y, z1],
                    ],
                    [
                        palette.texel_uv(u0, v0),
                        palette.texel_uv(u0, v1),
                        palette.texel_uv(u1, v0),
                        palette.texel_uv(u1, v1),
                    ],
                )
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LIFTED: (Vec3, f32) = (Vec3::Y, 0.018);

    #[test]
    fn touching_fingers_composite_once_and_leave_no_matte_border() {
        let mut tips = [LIFTED; FINGERTIPS];
        tips[0].0 = Vec3::new(-0.04, 0.0, 0.0);
        tips[1].0 = Vec3::new(0.04, 0.0, 0.0);
        let mut atlas = Atlas::new(2);
        atlas.push(0.0, [LIFTED; FEET], tips);
        atlas.push(0.0, [LIFTED; FEET], [LIFTED; FINGERTIPS]);
        let image = atlas.bake().unwrap();
        assert_eq!(atlas.quads(0, 0.0, &image).count(), 1);
        assert_eq!(atlas.quads(1, 0.0, &image).count(), 0);
        let patch = &atlas.patches[0];
        let each = patch.brushes[0].alpha(Vec2::ZERO);
        assert!(each > 0.0 && each < 1.0);
        assert!((patch.alpha(Vec2::ZERO) - (1.0 - (1.0 - each).powi(2))).abs() < 1.0e-5);
        let [x, y] = patch.origin;
        let [w, h] = patch.size;
        assert_eq!(image.pixel(x, y + h / 2), floor_material::CLEAR_RGB);
        let center_pixel = image.pixel(x + w / 2, y + h / 2);
        let center = (patch.low + Vec2::new((w / 2) as f32, (h / 2) as f32)) * TEXEL_METERS;
        let alpha = (255.0 * patch.alpha(center)).round() as usize;
        assert_eq!(center_pixel, floor_material::shadow_colors()[alpha]);
        assert_eq!(&image.bmp()[..2], b"BM");
        assert!(image.swatch(2).iter().all(|uv| (0.0..1.0).contains(uv)));
        assert_eq!(
            floor_material::CLEAR_RGBA.to_le_bytes()[..3],
            floor_material::CLEAR_RGB
        );
        assert!(atlas.patches.len() <= CONTACTS);
        atlas.clear();
    }
}
