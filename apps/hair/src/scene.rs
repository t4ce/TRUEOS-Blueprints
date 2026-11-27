use anyhow::{Context, Result, bail, ensure};
use glam::{Vec2, Vec3, Vec4};
use std::io::Cursor;

use crate::{
    Controls,
    obstacle::{BODY_OBSTACLES, HEAD, HEAD_CENTER},
    raster::{Alpha, Camera, Material, Surface, Texture, Vertex},
    solver::{HAIR_REACH_RADIUS, Solver},
};

const AVATAR: &[u8] = include_bytes!("../assets/avatar.glb");
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
    base_key: Option<(f32, bool, bool)>,
    base_pixels: Vec<u8>,
    base_depth: Vec<f32>,
    depth: Vec<f32>,
}

impl Scene {
    pub(crate) fn new(width: u32, height: u32) -> Result<Self> {
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
        let body_bounds = parts.iter().flat_map(|part| &part.vertices).fold(
            (Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY)),
            |(minimum, maximum), vertex| {
                (minimum.min(vertex.position), maximum.max(vertex.position))
            },
        );
        let count = width as usize * height as usize;
        Ok(Self {
            width,
            height,
            parts,
            body_bounds,
            textures,
            solver: Solver::new(BACK_HAIR_REST_DOWN),
            base_key: None,
            base_pixels: vec![0; count * 4],
            base_depth: vec![0.0; count],
            depth: vec![0.0; count],
        })
    }

    pub(crate) fn advance(&mut self) {
        self.solver.step();
    }

    #[cfg(target_os = "trueos")]
    pub(crate) fn resize(&mut self, width: u32, height: u32) {
        self.width = width;
        self.height = height;
        let count = width as usize * height as usize;
        self.base_pixels.resize(count * 4, 0);
        self.base_depth.resize(count, 0.0);
        self.depth.resize(count, 0.0);
        self.base_key = None;
    }

    pub(crate) fn draw(&mut self, controls: Controls, pixels: &mut [u8]) -> usize {
        let layers = controls.layers;
        let key = (controls.yaw, layers.avatar, layers.hair);
        let camera = Camera::new(
            self.width,
            self.height,
            controls.yaw,
            self.body_bounds,
            HEAD_CENTER,
            HAIR_REACH_RADIUS,
        );
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
        let segments = if layers.flow {
            self.solver.for_each_line(|start, end, speed, change| {
                let value = if controls.speed_color { speed } else { change };
                target.flow_line(camera, start, end, value);
            })
        } else {
            0
        };
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
    let (triangles, extra) = indices.as_chunks::<3>();
    ensure!(
        extra.is_empty()
            && indices
                .iter()
                .all(|&index| (index as usize) < vertices.len()),
        "invalid avatar triangle indices"
    );
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
mod tests {
    use super::*;

    #[test]
    fn evaluation_sphere_fits_close_to_the_resting_back_hair() {
        let asset = gltf::Gltf::from_slice(AVATAR).unwrap();
        let hair = asset
            .meshes()
            .flat_map(|mesh| mesh.primitives())
            .find(|primitive| primitive.material().name() == Some("Back Hair"))
            .unwrap();
        let radius = hair
            .reader(|_| asset.blob.as_deref())
            .read_positions()
            .unwrap()
            .map(|point| Vec3::from_array(point).distance(HEAD_CENTER))
            .fold(0.0, f32::max);
        assert!((radius..radius + 0.03).contains(&HAIR_REACH_RADIUS));
    }
}
