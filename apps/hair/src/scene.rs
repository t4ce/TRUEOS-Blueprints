use anyhow::{Context, Result, bail, ensure};
use glam::{Quat, Vec2, Vec3, Vec4};
use std::io::Cursor;

use crate::{
    Controls,
    obstacle::{BODY_OBSTACLES, HEAD, HEAD_CENTER},
    raster::{Alpha, Camera, Material, Surface, Texture, Vertex},
    solver::{HAIR_REACH_RADIUS, ROOT_STREAMLINE_SEEDS, RootSample, Solver},
};

const AVATAR: &[u8] = include_bytes!("../assets/avatar.glb");
const BACK_HAIR_ANCHOR: &[u8] = include_bytes!("../assets/back-hair-anchor-surface.glb");
// The standing back-hair rest axis points from the roots toward the hips.
const BACK_HAIR_REST_DOWN: Vec3 = Vec3::NEG_Y;

struct Part {
    vertices: Vec<Vertex>,
    triangles: Vec<[u32; 3]>,
    material: Material,
    back_hair: bool,
}

pub(crate) struct Scene {
    width: u32,
    height: u32,
    parts: Vec<Part>,
    body_bounds: (Vec3, Vec3),
    textures: Vec<Texture>,
    solver: Solver,
    camera_scale: Option<f32>,
    base_key: Option<(f32, f32, bool, bool)>,
    base_pixels: Vec<u8>,
    base_depth: Vec<f32>,
    depth: Vec<f32>,
}

impl Scene {
    pub(crate) fn new(width: u32, height: u32, roll: f32) -> Result<Self> {
        let asset = gltf::Gltf::from_slice(AVATAR).context("parse embedded reference avatar")?;
        let blob = asset
            .blob
            .as_deref()
            .context("avatar GLB has no embedded buffer")?;
        let textures = asset
            .images()
            .map(|image| match image.source() {
                gltf::image::Source::View {
                    view,
                    mime_type: "image/png",
                } => decode_texture(&blob[view.offset()..view.offset() + view.length()]),
                _ => bail!("expected an embedded PNG atlas"),
            })
            .collect::<Result<Vec<_>>>()?;
        let mut parts = asset
            .meshes()
            .flat_map(|mesh| mesh.primitives())
            .map(|primitive| load_part(primitive, blob))
            .collect::<Result<Vec<_>>>()?;
        // Masked/opaque surfaces establish the depth used by eye overlays.
        parts.sort_by_key(|part| matches!(part.material.alpha, Alpha::Blend));
        let roots = back_hair_roots()?;
        let body_bounds = parts.iter().flat_map(|part| &part.vertices).fold(
            (Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY)),
            |(minimum, maximum), vertex| {
                (minimum.min(vertex.position), maximum.max(vertex.position))
            },
        );
        // Keep the previous rear view's magnification for the Ubuntu turntable.
        let camera_scale = cfg!(target_os = "linux").then(|| {
            Camera::new(
                Vec2::new(width as f32, height as f32),
                165.0_f32.to_radians(),
                world_to_avatar(roll),
                body_bounds,
                HEAD_CENTER,
                HAIR_REACH_RADIUS,
                None,
            )
            .scale
        });
        let count = width as usize * height as usize;
        Ok(Self {
            width,
            height,
            parts,
            body_bounds,
            textures,
            solver: Solver::new(BACK_HAIR_REST_DOWN, world_to_avatar(roll), roots),
            camera_scale,
            base_key: None,
            base_pixels: vec![0; count * 4],
            base_depth: vec![0.0; count],
            depth: vec![0.0; count],
        })
    }

    pub(crate) fn advance(&mut self, roll: f32, tip_spread: bool) {
        self.solver.step(world_to_avatar(roll), tip_spread);
    }

    #[cfg(target_os = "trueos")]
    pub(crate) fn resize(&mut self, width: u32, height: u32) {
        self.width = width;
        self.height = height;
        let count = width as usize * height as usize;
        self.base_pixels.resize(count * 4, 0);
        self.base_depth.resize(count, 0.0);
        self.depth.resize(count, 0.0);
        self.camera_scale = None;
        self.base_key = None;
    }

    pub(crate) fn draw(&mut self, controls: Controls, pixels: &mut [u8]) -> usize {
        let layers = controls.layers;
        let key = (controls.yaw, controls.roll, layers.avatar, layers.hair);
        let camera = Camera::new(
            Vec2::new(self.width as f32, self.height as f32),
            controls.yaw,
            world_to_avatar(controls.roll),
            self.body_bounds,
            HEAD_CENTER,
            HAIR_REACH_RADIUS,
            self.camera_scale,
        );
        self.camera_scale.get_or_insert(camera.scale);
        if self.base_key != Some(key) {
            let mut base = Surface::new(
                self.width,
                self.height,
                &mut self.base_pixels,
                &mut self.base_depth,
            );
            base.clear();
            for part in &self.parts {
                if (part.back_hair && layers.hair) || (!part.back_hair && layers.avatar) {
                    let texture = part.material.texture.map(|index| &self.textures[index]);
                    base.mesh(
                        camera,
                        &part.vertices,
                        &part.triangles,
                        &part.material,
                        texture,
                    );
                }
            }
            self.base_key = Some(key);
        }
        pixels.copy_from_slice(&self.base_pixels);
        self.depth.copy_from_slice(&self.base_depth);
        let mut target = Surface::new(self.width, self.height, pixels, &mut self.depth);
        if layers.reach {
            target.reach_sphere(camera, HEAD_CENTER, HAIR_REACH_RADIUS);
        }
        let mut segments = 0;
        if layers.field {
            segments += self
                .solver
                .for_each_field_line(|start, end, speed, change| {
                    let value = if controls.speed_color { speed } else { change };
                    target.flow_line(camera, start, end, None, value, 0.24);
                });
        }
        if layers.guides {
            let opacity = if cfg!(target_os = "trueos") {
                0.24
            } else {
                0.50
            };
            segments += self
                .solver
                .for_each_guide_line(|attachment, start, end, speed, change| {
                    let value = if controls.speed_color { speed } else { change };
                    target.flow_line(camera, start, end, attachment, value, opacity);
                });
        }
        if layers.body_obstacles {
            let color = Vec3::new(0.45, 0.77, 0.94);
            for shape in BODY_OBSTACLES {
                target.ellipsoid(camera, shape, color);
            }
        }
        if layers.sphere {
            target.ellipsoid(camera, HEAD, Vec3::ONE);
        }
        segments
    }
}

fn world_to_avatar(roll: f32) -> Quat {
    Quat::from_rotation_z(-roll)
}

fn back_hair_roots() -> Result<[RootSample; ROOT_STREAMLINE_SEEDS]> {
    let asset =
        gltf::Gltf::from_slice(BACK_HAIR_ANCHOR).context("parse back-hair anchor surface")?;
    let blob = asset
        .blob
        .as_deref()
        .context("back-hair anchor has no embedded buffer")?;
    let primitive = asset
        .meshes()
        .find(|mesh| mesh.name() == Some("Back Hair AnkorSurf"))
        .and_then(|mesh| mesh.primitives().next())
        .context("missing Back Hair AnkorSurf mesh")?;
    ensure!(
        primitive.mode() == gltf::mesh::Mode::Triangles,
        "expected triangles in the back-hair anchor"
    );
    let reader = primitive.reader(|_| Some(blob));
    let positions: Vec<_> = reader
        .read_positions()
        .context("back-hair anchor is missing positions")?
        .map(Vec3::from_array)
        .collect();
    let indices: Vec<_> = reader
        .read_indices()
        .context("back-hair anchor is missing triangle indices")?
        .into_u32()
        .collect();
    let triangles = checked_triangles(&indices, positions.len()).context("back-hair anchor")?;

    let mut total_area = 0.0;
    let mut surface = Vec::new();
    let mut normals = vec![Vec3::ZERO; positions.len()];
    for triangle in triangles {
        let vertices = triangle.map(|index| index as usize);
        let [a, b, c] = vertices.map(|index| positions[index]);
        let normal = (b - a).cross(c - a);
        let area = 0.5 * normal.length();
        if area > 0.0 {
            total_area += area;
            surface.push((vertices, area, total_area));
            for vertex in vertices {
                normals[vertex] += normal;
            }
        }
    }
    ensure!(
        total_area.is_finite() && total_area > 0.0,
        "invalid back-hair anchor surface area"
    );
    for normal in &mut normals {
        *normal = normal.normalize_or_zero();
    }

    const CANDIDATES: usize = ROOT_STREAMLINE_SEEDS * 8;
    let candidates: [RootSample; CANDIDATES] = core::array::from_fn(|index| {
        let target = total_area * ((index as f32 + 0.5) / CANDIDATES as f32);
        let triangle_index = surface.partition_point(|&(_, _, end)| end <= target);
        let (vertices, area, end) = surface[triangle_index];
        // Use one area stratum per candidate and a base-two radical inverse for
        // the second barycentric parameter.
        let radius = ((target - (end - area)) / area).clamp(0.0, 1.0).sqrt();
        let across = ((index as u32).reverse_bits() as f64 / 4_294_967_296.0) as f32;
        let [a, b, c] = vertices.map(|index| positions[index]);
        let [na, nb, nc] = vertices.map(|index| normals[index]);
        RootSample {
            position: a.lerp(b.lerp(c, across), radius),
            outward: na.lerp(nb.lerp(nc, across), radius).normalize(),
        }
    });
    let center = candidates.iter().map(|root| root.position).sum::<Vec3>() / CANDIDATES as f32;
    let mut next = (0..CANDIDATES)
        .min_by(|&a, &b| {
            candidates[a]
                .position
                .distance_squared(center)
                .total_cmp(&candidates[b].position.distance_squared(center))
        })
        .unwrap();
    let mut nearest = [f32::INFINITY; CANDIDATES];
    // Fill the gaps on the curved patch by repeatedly choosing the candidate
    // farthest from the roots already selected.
    Ok(core::array::from_fn(|_| {
        let root = candidates[next];
        for (distance, candidate) in nearest.iter_mut().zip(&candidates) {
            *distance = distance.min(candidate.position.distance_squared(root.position));
        }
        next = (0..CANDIDATES)
            .max_by(|&a, &b| nearest[a].total_cmp(&nearest[b]))
            .unwrap();
        root
    }))
}

fn load_part(primitive: gltf::Primitive<'_>, blob: &[u8]) -> Result<Part> {
    ensure!(
        primitive.mode() == gltf::mesh::Mode::Triangles,
        "expected triangles in the static avatar"
    );
    let reader = primitive.reader(|_| Some(blob));
    let positions = reader
        .read_positions()
        .context("avatar is missing positions")?;
    let normals = reader.read_normals().context("avatar is missing normals")?;
    let uv = reader
        .read_tex_coords(0)
        .context("avatar is missing UVs")?
        .into_f32();
    ensure!(
        positions.len() == normals.len() && positions.len() == uv.len(),
        "inconsistent mesh vertex streams"
    );
    let vertices: Vec<_> = positions
        .zip(normals)
        .zip(uv)
        .map(|((position, normal), uv)| Vertex {
            position: Vec3::from_array(position),
            normal: Vec3::from_array(normal).normalize_or_zero(),
            uv: Vec2::from_array(uv),
        })
        .collect();
    let indices: Vec<_> = reader
        .read_indices()
        .context("avatar is missing triangle indices")?
        .into_u32()
        .collect();
    let triangles = checked_triangles(&indices, vertices.len()).context("avatar mesh")?;
    let source = primitive.material();
    let pbr = source.pbr_metallic_roughness();
    let material = Material {
        texture: pbr
            .base_color_texture()
            .map(|info| info.texture().source().index()),
        factor: Vec4::from_array(pbr.base_color_factor()),
        alpha: match source.alpha_mode() {
            gltf::material::AlphaMode::Opaque => Alpha::Opaque,
            gltf::material::AlphaMode::Mask => Alpha::Mask(source.alpha_cutoff().unwrap_or(0.5)),
            gltf::material::AlphaMode::Blend => Alpha::Blend,
        },
        double_sided: source.double_sided(),
    };
    Ok(Part {
        vertices,
        triangles: triangles.to_vec(),
        material,
        back_hair: source.name() == Some("Back Hair"),
    })
}

fn checked_triangles(indices: &[u32], vertices: usize) -> Result<&[[u32; 3]]> {
    let (triangles, extra) = indices.as_chunks::<3>();
    ensure!(
        extra.is_empty() && indices.iter().all(|&index| (index as usize) < vertices),
        "invalid triangle indices"
    );
    Ok(triangles)
}

fn decode_texture(bytes: &[u8]) -> Result<Texture> {
    let mut decoder = png::Decoder::new(Cursor::new(bytes));
    decoder.set_transformations(png::Transformations::normalize_to_color8());
    let mut reader = decoder.read_info()?;
    let mut bytes = vec![
        0;
        reader
            .output_buffer_size()
            .context("avatar PNG too large")?
    ];
    let image = reader.next_frame(&mut bytes)?;
    let bytes = &bytes[..image.buffer_size()];
    let pixels = match image.color_type {
        png::ColorType::Rgba => bytes.as_chunks::<4>().0.to_vec(),
        png::ColorType::Rgb => bytes
            .as_chunks::<3>()
            .0
            .iter()
            .map(|&[r, g, b]| [r, g, b, 255])
            .collect(),
        _ => bail!("expected an RGB or RGBA atlas"),
    };
    Ok(Texture {
        width: image.width,
        height: image.height,
        pixels,
    })
}

#[cfg(test)]
fn reference_back_hair() -> Part {
    let asset = gltf::Gltf::from_slice(AVATAR).unwrap();
    let primitive = asset
        .meshes()
        .flat_map(|mesh| mesh.primitives())
        .find(|primitive| primitive.material().name() == Some("Back Hair"))
        .unwrap();
    load_part(primitive, asset.blob.as_deref().unwrap()).unwrap()
}

#[cfg(test)]
pub(crate) fn reference_roots() -> [RootSample; ROOT_STREAMLINE_SEEDS] {
    back_hair_roots().unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn evaluation_sphere_fits_close_to_the_resting_back_hair() {
        let hair = reference_back_hair();
        let radius = hair
            .vertices
            .iter()
            .map(|vertex| vertex.position.distance(HEAD_CENTER))
            .fold(0.0, f32::max);
        assert!((radius..radius + 0.03).contains(&HAIR_REACH_RADIUS));
    }

    #[test]
    fn root_samples_cover_the_authored_scalp_patch() {
        let roots = reference_roots();
        for (index, root) in roots.iter().enumerate() {
            assert!(root.outward.is_normalized());
            assert!(root.outward.dot(root.position - HEAD_CENTER) > 0.0);
            assert!(
                roots[..index]
                    .iter()
                    .all(|other| root.position.distance(other.position) > 0.01)
            );
        }
        let (min, max) = roots.iter().fold(
            (Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY)),
            |(min, max), root| (min.min(root.position), max.max(root.position)),
        );
        let span = max - min;
        assert!((0.12..0.17).contains(&span.x));
        assert!((0.08..0.11).contains(&span.y));
        assert!((0.05..0.08).contains(&span.z));
        assert!(min.y < HEAD_CENTER.y && max.y > HEAD_CENTER.y && min.z > HEAD_CENTER.z);
    }
}
