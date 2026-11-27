use glam::{Quat, Vec2, Vec3, Vec4};

use crate::obstacle::Ellipsoid;

const BACKGROUND: [u8; 4] = [229, 235, 239, 255];

#[derive(Clone, Copy)]
pub(crate) struct Camera {
    right: Vec3,
    up: Vec3,
    toward: Vec3,
    light: Vec3,
    center: Vec3,
    offset: Vec2,
    pub(crate) scale: f32,
}

impl Camera {
    pub(crate) fn new(
        viewport: Vec2,
        yaw: f32,
        world_to_avatar: Quat,
        body_bounds: (Vec3, Vec3),
        reach_center: Vec3,
        reach_radius: f32,
        fixed_scale: Option<f32>,
    ) -> Self {
        let (sin, cos) = yaw.sin_cos();
        let toward = Vec3::new(sin, 0.12, -cos).normalize();
        let right = Vec3::Y.cross(toward).normalize();
        let up = toward.cross(right);
        let light = world_to_avatar * (toward + Vec3::new(-0.22, 0.68, 0.12)).normalize();
        // Project onto the fixed world camera from the avatar-local scene.
        let [right, up, toward] = [right, up, toward].map(|axis| world_to_avatar * axis);
        let view = |point: Vec3| Vec2::new(point.dot(right), point.dot(up));
        let (body_min, body_max) = body_bounds;
        let body_center = view((body_min + body_max) * 0.5);
        let body_half = (body_max - body_min) * 0.5;
        let body_span = Vec2::new(body_half.dot(right.abs()), body_half.dot(up.abs()));
        let reach = view(reach_center);
        let low = (body_center - body_span).min(reach - Vec2::splat(reach_radius));
        let high = (body_center + body_span).max(reach + Vec2::splat(reach_radius));
        let fit_center = (low + high) * 0.5;
        Self {
            right,
            up,
            toward,
            light,
            center: right * fit_center.x + up * fit_center.y + toward * reach_center.dot(toward),
            offset: viewport * 0.5,
            scale: fixed_scale.unwrap_or_else(|| 0.90 * (viewport / (high - low)).min_element()),
        }
    }

    fn project(self, world: Vec3) -> Vec3 {
        let p = world - self.center;
        Vec3::new(
            self.offset.x + p.dot(self.right) * self.scale,
            self.offset.y - p.dot(self.up) * self.scale,
            p.dot(self.toward),
        )
    }
}

pub(crate) struct Texture {
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) pixels: Vec<[u8; 4]>,
}

impl Texture {
    fn sample(&self, uv: Vec2) -> Vec4 {
        let size = Vec2::new(self.width as f32, self.height as f32);
        let uv = (uv * size - Vec2::splat(0.5)).clamp(Vec2::ZERO, size - Vec2::ONE);
        let x = uv.x as usize;
        let y = uv.y as usize;
        let x_next = (x + 1).min(self.width as usize - 1);
        let y_next = (y + 1).min(self.height as usize - 1);
        let width = self.width as usize;
        let texel = |x: usize, y: usize| {
            Vec4::from_array(self.pixels[y * width + x].map(|byte| f32::from(byte) / 255.0))
        };
        let top = texel(x, y).lerp(texel(x_next, y), uv.x.fract());
        let bottom = texel(x, y_next).lerp(texel(x_next, y_next), uv.x.fract());
        top.lerp(bottom, uv.y.fract())
    }
}

#[derive(Clone, Copy)]
pub(crate) struct Vertex {
    pub(crate) position: Vec3,
    pub(crate) normal: Vec3,
    pub(crate) uv: Vec2,
}

#[derive(Clone, Copy)]
pub(crate) enum Alpha {
    Opaque,
    Mask(f32),
    Blend,
}

pub(crate) struct Material {
    pub(crate) texture: Option<usize>,
    pub(crate) factor: Vec4,
    pub(crate) alpha: Alpha,
    pub(crate) double_sided: bool,
}

#[derive(Clone, Copy)]
struct Projected {
    position: Vec3,
    shade: f32,
    uv: Vec2,
}

pub(crate) struct Surface<'a> {
    width: usize,
    height: usize,
    pixels: &'a mut [u8],
    depth: &'a mut [f32],
}

impl<'a> Surface<'a> {
    pub(crate) fn new(width: u32, height: u32, pixels: &'a mut [u8], depth: &'a mut [f32]) -> Self {
        Self {
            width: width as usize,
            height: height as usize,
            pixels,
            depth,
        }
    }

    pub(crate) fn clear(&mut self) {
        self.pixels.as_chunks_mut::<4>().0.fill(BACKGROUND);
        self.depth.fill(f32::NEG_INFINITY);
    }

    pub(crate) fn mesh(
        &mut self,
        camera: Camera,
        vertices: &[Vertex],
        indices: &[[u32; 3]],
        material: &Material,
        texture: Option<&Texture>,
    ) {
        let projected: Vec<_> = vertices
            .iter()
            .map(|vertex| Projected {
                position: camera.project(vertex.position),
                shade: 0.71 + 0.29 * vertex.normal.dot(camera.light).max(0.0),
                uv: vertex.uv,
            })
            .collect();
        for triangle in indices {
            self.triangle(
                triangle.map(|index| projected[index as usize]),
                material,
                texture,
            );
        }
    }

    fn triangle(
        &mut self,
        [a, b, c]: [Projected; 3],
        material: &Material,
        texture: Option<&Texture>,
    ) {
        let ab = (b.position - a.position).truncate();
        let ac = (c.position - a.position).truncate();
        let area = ab.perp_dot(ac);
        if area.abs() < 1e-4 || (!material.double_sided && area > 0.0) {
            return;
        }
        let inverse_area = area.recip();
        let low = a.position.min(b.position).min(c.position).truncate();
        let high = a.position.max(b.position).max(c.position).truncate();
        let (xs, ys) = self.pixel_rect(low, high);
        for y in ys {
            for x in xs.clone() {
                let d = Vec2::new(x as f32 + 0.5, y as f32 + 0.5) - a.position.truncate();
                let wb = d.perp_dot(ac) * inverse_area;
                let wc = ab.perp_dot(d) * inverse_area;
                let wa = 1.0 - wb - wc;
                if wa.min(wb).min(wc) < 0.0 {
                    continue;
                }
                let index = y * self.width + x;
                let depth = wa * a.position.z + wb * b.position.z + wc * c.position.z;
                if depth <= self.depth[index] {
                    continue;
                }
                let uv = wa * a.uv + wb * b.uv + wc * c.uv;
                let color =
                    texture.map_or(Vec4::ONE, |texture| texture.sample(uv)) * material.factor;
                let alpha = match material.alpha {
                    Alpha::Opaque => 1.0,
                    Alpha::Mask(cutoff) if color.w < cutoff => continue,
                    Alpha::Mask(_) => 1.0,
                    Alpha::Blend if color.w == 0.0 => continue,
                    Alpha::Blend => color.w,
                };
                let shade = wa * a.shade + wb * b.shade + wc * c.shade;
                self.blend(index, color.truncate() * shade, alpha);
                if alpha >= 0.98 {
                    self.depth[index] = depth;
                }
            }
        }
    }

    pub(crate) fn flow_line(
        &mut self,
        camera: Camera,
        start: Vec3,
        end: Vec3,
        attachment: Option<Vec3>,
        value: f32,
        minimum_opacity: f32,
    ) {
        let color = rainbow(value);
        let opacity = minimum_opacity + (0.8 - minimum_opacity) * value.clamp(0.0, 1.0).sqrt();
        let mut start = camera.project(start);
        let end = camera.project(end);
        if let Some(root) = attachment {
            let root = camera.project(root);
            let join = start.lerp(end, 0.35);
            // The first fluid sample is the quadratic control point. The bend
            // leaves along the scalp normal and arrives along the solved flow.
            const STEPS: u32 = 6;
            let mut previous = root;
            for step in 1..=STEPS {
                let t = step as f32 / STEPS as f32;
                let next = root.lerp(start, t).lerp(start.lerp(join, t), t);
                self.line::<true>(previous, next, color, opacity);
                previous = next;
            }
            start = join;
        }
        self.line::<true>(start, end, color, opacity);
    }

    fn line<const WRITE_DEPTH: bool>(&mut self, start: Vec3, end: Vec3, color: Vec3, opacity: f32) {
        let span = (end - start).abs().truncate().max_element();
        let steps = (span.ceil() as u32).max(1);
        for step in 0..=steps {
            let p = start.lerp(end, step as f32 / steps as f32);
            if p.x >= 0.0 && p.x < self.width as f32 && p.y >= 0.0 && p.y < self.height as f32 {
                let index = p.y as usize * self.width + p.x as usize;
                if p.z > self.depth[index] {
                    self.blend(index, color, opacity);
                    if WRITE_DEPTH {
                        self.depth[index] = p.z;
                    }
                }
            }
        }
    }

    pub(crate) fn reach_sphere(&mut self, camera: Camera, center: Vec3, radius: f32) {
        for axes in [[Vec3::X, Vec3::Y], [Vec3::Y, Vec3::Z], [Vec3::Z, Vec3::X]] {
            self.reach_circle(camera, center, radius, axes, 0.22);
        }
        self.reach_circle(camera, center, radius, [camera.right, camera.up], 0.58);
    }

    fn reach_circle(
        &mut self,
        camera: Camera,
        center: Vec3,
        radius: f32,
        axes: [Vec3; 2],
        opacity: f32,
    ) {
        const STEPS: usize = 128;
        const WIRE: Vec3 = Vec3::new(0.25, 0.35, 0.46);
        let [x, y] = axes;
        let mut previous = camera.project(center + radius * x);
        for step in 1..=STEPS {
            let (sin, cos) = (std::f32::consts::TAU * step as f32 / STEPS as f32).sin_cos();
            let next = camera.project(center + radius * (cos * x + sin * y));
            // The avatar hides the far-side wires. Passing air can still draw across them.
            self.line::<false>(previous, next, WIRE, opacity);
            previous = next;
        }
    }

    pub(crate) fn ellipsoid(&mut self, camera: Camera, shape: Ellipsoid, color: Vec3) {
        let center = camera.project(shape.center);
        let extent = Vec2::new(
            (camera.right * shape.radii).length(),
            (camera.up * shape.radii).length(),
        ) * camera.scale;
        let (xs, ys) = self.pixel_rect(center.truncate() - extent, center.truncate() + extent);
        // Intersect orthographic rays in the ellipsoid's unit sphere coordinates.
        let right = shape.unit_offset(camera.right / camera.scale);
        let down = shape.unit_offset(-camera.up / camera.scale);
        let toward = shape.unit_offset(camera.toward);
        let a = toward.length_squared();
        for y in ys {
            for x in xs.clone() {
                let pixel = Vec2::new(x as f32 + 0.5, y as f32 + 0.5) - center.truncate();
                let offset = pixel.x * right + pixel.y * down;
                let b = offset.dot(toward);
                let discriminant = b * b - a * (offset.length_squared() - 1.0);
                let index = y * self.width + x;
                if discriminant >= 0.0
                    && center.z + (-b + discriminant.sqrt()) / a > self.depth[index]
                {
                    self.blend(index, color, 0.5);
                }
            }
        }
    }

    fn pixel_rect(
        &self,
        low: Vec2,
        high: Vec2,
    ) -> (std::ops::Range<usize>, std::ops::Range<usize>) {
        let width = self.width;
        let height = self.height;
        (
            (low.x.floor() as usize).min(width)..(high.x.ceil() as usize).min(width),
            (low.y.floor() as usize).min(height)..(high.y.ceil() as usize).min(height),
        )
    }

    fn blend(&mut self, index: usize, color: Vec3, alpha: f32) {
        let pixel = &mut self.pixels[index * 4..index * 4 + 3];
        for (byte, channel) in pixel.iter_mut().zip(color.to_array()) {
            *byte = ((1.0 - alpha) * f32::from(*byte) + alpha * channel * 255.0).round() as u8;
        }
    }
}

// Piecewise rainbow in the same order as the FluidX3D streamline visualization.
fn rainbow(value: f32) -> Vec3 {
    const STOPS: [(f32, Vec3); 7] = [
        (0.0, Vec3::ZERO),
        (1.0 / 6.0, Vec3::new(0.4, 0.0, 0.5)),
        (2.0 / 6.0, Vec3::Z),
        (0.5, Vec3::new(0.0, 1.0, 1.0)),
        (4.0 / 6.0, Vec3::Y),
        (0.8, Vec3::new(1.0, 1.0, 0.0)),
        (1.0, Vec3::X),
    ];
    let value = value.clamp(0.0, 1.0);
    let next = STOPS
        .partition_point(|&(threshold, _)| threshold < value)
        .clamp(1, STOPS.len() - 1);
    let (start, low) = STOPS[next - 1];
    let (end, high) = STOPS[next];
    low.lerp(high, (value - start) / (end - start))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn camera_fits_the_avatar_and_sphere_in_wide_and_tall_views() {
        let body_min = Vec3::new(-0.8, 0.0, -0.2);
        let body_max = Vec3::new(0.8, 1.6, 0.34);
        let center = Vec3::new(0.0, 1.42, -0.015);
        let radius = 1.0;
        for (width, height) in [(960, 720), (640, 960), (1280, 480)] {
            let viewport = Vec2::new(width as f32, height as f32);
            for yaw in [0.0, 0.6, 1.4, 2.3, 4.9] {
                for roll in [
                    0.0,
                    0.7,
                    core::f32::consts::FRAC_PI_2,
                    core::f32::consts::PI,
                ] {
                    let world_to_avatar = Quat::from_rotation_z(-roll);
                    let camera = Camera::new(
                        viewport,
                        yaw,
                        world_to_avatar,
                        (body_min, body_max),
                        center,
                        radius,
                        None,
                    );
                    let local_down = world_to_avatar * Vec3::NEG_Y;
                    assert!(camera.project(center + local_down).y > camera.project(center).y);
                    let inside_margin = |point| {
                        let pixels = camera.project(point).truncate();
                        assert!(pixels.cmpge(viewport * 0.049).all());
                        assert!(pixels.cmple(viewport * 0.951).all());
                    };
                    for x in [body_min.x, body_max.x] {
                        for y in [body_min.y, body_max.y] {
                            for z in [body_min.z, body_max.z] {
                                inside_margin(Vec3::new(x, y, z));
                            }
                        }
                    }
                    for edge in [camera.right, -camera.right, camera.up, -camera.up] {
                        inside_margin(center + radius * edge);
                    }
                }
            }
        }
    }

    #[test]
    fn orbit_keeps_the_initial_projection_scale() {
        let viewport = Vec2::new(960.0, 720.0);
        let bounds = (Vec3::new(-0.8, 0.0, -0.34), Vec3::new(0.8, 1.6, 0.34));
        let center = Vec3::new(0.0, 1.42, -0.015);
        let camera =
            |yaw, scale| Camera::new(viewport, yaw, Quat::IDENTITY, bounds, center, 0.75, scale);
        let scale = camera(165.0_f32.to_radians(), None).scale;
        for yaw in [0.0, 1.6, 4.7, 5.5] {
            let camera = camera(yaw, Some(scale));
            let projected_center = camera.project(center);
            let projected_right = camera.project(center + camera.right);
            let projected_up = camera.project(center + camera.up);
            assert!((projected_right.x - projected_center.x - scale).abs() < 1e-4);
            assert!((projected_center.y - projected_up.y - scale).abs() < 1e-4);
        }
    }
}
