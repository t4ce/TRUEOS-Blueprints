use anyhow::{Context, Result, ensure};
use glam::{Mat2, Vec2, Vec3};
use trueos::vmedia::{self, ImageFormat};

use super::gpu_error;
use crate::{
    mesh::{BODY_MATERIAL, Mesh},
    top::Surface,
};

mod marks;

const BODY_TINT: usize = 0;
const BODY_MARKS: usize = 1;
const PNGS: [&[u8]; 2] = [
    include_bytes!("../../assets/AvatarSample_B-body-tint.png"),
    include_bytes!("../../assets/AvatarSample_B-body-marks.png"),
];
const MASK_ATLASES: [[usize; 2]; 2] = [[34, 29], [64, 64]];
const BODY_ATLAS_SIZE: f32 = 1024.0;
const TILE_HEIGHT: usize = 40;
pub(super) const WIDTH: usize = 40;
pub(super) const HEIGHT: usize = TILE_HEIGHT * 3;
const MARK_COLUMNS: usize = 3;
pub(super) const MARKS_EXTENT: [usize; 2] = [
    WIDTH * MARK_COLUMNS,
    TILE_HEIGHT * marks::PLACEMENTS.len().div_ceil(MARK_COLUMNS),
];

struct Mask {
    // Coordinates in the source UV atlas and in the packed tint image.
    origin: Vec2,
    span: Vec2,
    offset: [usize; 2],
    size: [usize; 2],
    atlas: usize,
    uv_to_mark: Option<Mat2>,
    mirror_u: bool,
    attenuation: Vec3,
}

const BELLY: Mask = Mask {
    origin: Vec2::new(254.0, 725.0),
    span: Vec2::new(4.5, 8.0),
    offset: [0, 0],
    size: [9, 16],
    atlas: BODY_TINT,
    uv_to_mark: None,
    mirror_u: false,
    attenuation: Vec3::splat(0.16),
};

impl Mask {
    fn chest(center: Vec2, mirror_u: bool) -> Self {
        let span = Vec2::new(7.2, 8.4);
        Self {
            origin: center - span * 0.5,
            span,
            offset: [9, 0],
            size: [25, 29],
            atlas: BODY_TINT,
            uv_to_mark: None,
            mirror_u,
            attenuation: Vec3::splat(0.14),
        }
    }

    fn tint(&self, rgba: &[u8], uv: Vec2) -> Vec3 {
        let [width, height] = self.size;
        let size = Vec2::new(width as f32, height as f32);
        let pixel_uv = uv * BODY_ATLAS_SIZE;
        let local = if let Some(to_mark) = self.uv_to_mark {
            let center = self.origin + self.span * 0.5;
            (to_mark * (pixel_uv - center) + Vec2::splat(0.5)) * size
        } else {
            (pixel_uv - self.origin) * (size / self.span)
        };
        if local.min_element() < 0.0 || local.cmpge(size).any() {
            return Vec3::ONE;
        }
        // Every packed mask has a black border.
        let mut pixel = (local - 0.5).clamp(Vec2::ZERO, size - 1.0);
        if self.mirror_u {
            pixel.x = size.x - 1.0 - pixel.x;
        }
        let x = pixel.x as usize;
        let y = pixel.y as usize;
        let right = (x + 1).min(width - 1);
        let bottom = (y + 1).min(height - 1);
        let sample = |column, row| {
            let i = (self.offset[1] + row) * MASK_ATLASES[self.atlas][0] + self.offset[0] + column;
            f32::from(rgba[i * 4]) / 255.0
        };
        let top = sample(x, y) + (sample(right, y) - sample(x, y)) * pixel.x.fract();
        let bottom =
            sample(x, bottom) + (sample(right, bottom) - sample(x, bottom)) * pixel.x.fract();
        let shade = top + (bottom - top) * pixel.y.fract();
        Vec3::ONE - self.attenuation * shade
    }
}

struct Triangle {
    index: usize,
    sources: [usize; 3],
    uv: [Vec2; 3],
}

impl Triangle {
    fn weights(&self, point: Vec2) -> Vec3 {
        let [a, b, c] = self.uv;
        let offset = point - a;
        let ab = b - a;
        let ac = c - a;
        let area = ab.perp_dot(ac);
        let v = offset.perp_dot(ac) / area;
        let w = ab.perp_dot(offset) / area;
        Vec3::new(1.0 - v - w, v, w)
    }

    fn point(&self, weights: Vec3) -> Vec2 {
        let [a, b, c] = self.uv;
        a * weights.x + b * weights.y + c * weights.z
    }
}

struct Texel {
    triangle: usize,
    weights: Vec3,
    tint: Vec3,
}

pub(super) struct BodyTint {
    tiles: Vec<TintTile>,
    rgba: [Vec<u8>; 2],
}

struct TintTile {
    mask: Mask,
    atlas_origin: [usize; 2],
    triangles: Vec<Triangle>,
    texels: Vec<Texel>,
    origin: Vec2,
    scale: Vec2,
    shade_uvs: Vec<[Vec2; 3]>,
}

impl BodyTint {
    pub(super) async fn load(
        glb: &[u8],
        mesh: &Mesh,
        [body_x, body_y]: [usize; 2],
        [marks_x, marks_y]: [usize; 2],
    ) -> Result<Self> {
        let rgba = [load_atlas(BODY_TINT).await?, load_atlas(BODY_MARKS).await?];
        let asset = gltf::Gltf::from_slice(glb).context("parse body tint mesh")?;
        let primitive = asset
            .meshes()
            .flat_map(|mesh| mesh.primitives())
            .find(|primitive| primitive.material().name() == Some(BODY_MATERIAL))
            .context("missing body material for body tint")?;
        let blob = asset
            .blob
            .as_deref()
            .context("body mesh has no BIN chunk")?;
        let reader = primitive
            .reader(|buffer| matches!(buffer.source(), gltf::buffer::Source::Bin).then_some(blob));
        let coords: Vec<_> = reader
            .read_tex_coords(0)
            .context("missing body TEXCOORD_0 for body tint")?
            .into_f32()
            .map(Vec2::from_array)
            .collect();
        let material_vertices = &mesh.material_vertices[1];
        ensure!(
            coords.len() == material_vertices.len(),
            "body tint UV count differs from the body mesh"
        );
        // The mesh has a 16-triangle fan at each center (vertices 3010 and
        // 2190). Their UVs mirror about U=0.25.
        let center = Vec2::new(225.4891, 640.6290);
        let opposite = Vec2::new(BODY_ATLAS_SIZE * 0.5 - center.x, center.y);
        let tile =
            |mask: Mask, origin| TintTile::new(mesh, &coords, &rgba[mask.atlas], mask, origin);
        let mut tiles = Vec::with_capacity(3 + marks::PLACEMENTS.len());
        for (slot, mask) in [
            BELLY,
            Mask::chest(center, false),
            Mask::chest(opposite, true),
        ]
        .into_iter()
        .enumerate()
        {
            tiles.push(tile(mask, [body_x, body_y + slot * TILE_HEIGHT])?);
        }
        for (slot, placement) in marks::PLACEMENTS.iter().enumerate() {
            let x = marks_x + slot % MARK_COLUMNS * WIDTH;
            let y = marks_y + slot / MARK_COLUMNS * TILE_HEIGHT;
            tiles.push(tile(placement.mask(), [x, y])?);
        }
        // Every tile replaces its triangles' indices. Keep source triangles
        // disjoint so a later tile cannot hide an earlier mark.
        let body = mesh.material_triangles[1].clone();
        let mut claimed = vec![false; body.len()];
        for triangle in tiles.iter().flat_map(|tile| &tile.triangles) {
            ensure!(
                !std::mem::replace(&mut claimed[triangle.index - body.start], true),
                "body tint tiles overlap at triangle {}",
                triangle.index
            );
        }
        Ok(Self { tiles, rgba })
    }

    pub(super) fn triangle_tint(&self, triangle_index: usize) -> impl Fn(Vec3) -> Vec3 + '_ {
        let matched = self.tiles.iter().find_map(|tile| {
            let slot = tile
                .triangles
                .binary_search_by_key(&triangle_index, |triangle| triangle.index)
                .ok()?;
            Some((&tile.mask, &tile.triangles[slot]))
        });
        move |weights| {
            matched.map_or(Vec3::ONE, |(mask, triangle)| {
                mask.tint(&self.rgba[mask.atlas], triangle.point(weights))
            })
        }
    }

    pub(super) fn vertex_count(&self) -> usize {
        self.tiles.iter().map(TintTile::vertex_count).sum()
    }

    pub(super) fn remap_indices(&self, indices: &mut [u32], mut base: u32) {
        for tile in &self.tiles {
            tile.remap_indices(indices, base);
            base += tile.vertex_count() as u32;
        }
    }

    pub(super) fn vertices(&self) -> impl Iterator<Item = (usize, Vec2)> + '_ {
        self.tiles.iter().flat_map(|tile| {
            let [x, y] = tile.atlas_origin;
            let offset = Vec2::new(x as f32, y as f32);
            tile.vertices().map(move |(index, uv)| (index, uv + offset))
        })
    }

    pub(super) fn write_tiles(
        &mut self,
        atlas: &mut [u8],
        extent: [usize; 2],
        top_row: Option<usize>,
        shade_uv: impl Fn(usize) -> [f32; 2],
    ) {
        for tile in &mut self.tiles {
            tile.write(atlas, extent, top_row, &shade_uv);
        }
    }
}

async fn load_atlas(atlas: usize) -> Result<Vec<u8>> {
    let image = vmedia::decode(ImageFormat::Png, PNGS[atlas])
        .await
        .map_err(gpu_error)
        .with_context(|| format!("decode body mask atlas {atlas}"))?;
    let [width, height] = MASK_ATLASES[atlas];
    ensure!(
        image.info.width as usize == width
            && image.info.height as usize == height
            && image.info.stride_bytes as usize == width * 4
            && image.rgba.len() == width * height * 4,
        "unexpected body mask atlas extent"
    );
    Ok(image.rgba)
}

impl TintTile {
    fn new(
        mesh: &Mesh,
        coords: &[Vec2],
        rgba: &[u8],
        mask: Mask,
        atlas_origin: [usize; 2],
    ) -> Result<Self> {
        let min = mask.origin / BODY_ATLAS_SIZE;
        let max = (mask.origin + mask.span) / BODY_ATLAS_SIZE;
        let material_vertices = &mesh.material_vertices[1];
        let material_triangles = mesh.material_triangles[1].clone();
        let triangles: Vec<_> = mesh.indices.as_chunks::<3>().0[material_triangles.clone()]
            .iter()
            .enumerate()
            .filter_map(|(offset, indices)| {
                let sources = indices.map(|index| index as usize);
                let uv = sources.map(|index| coords[index - material_vertices.start]);
                let [a, b, c] = uv;
                ((a.max(b).max(c).cmpgt(min)).all()
                    && (a.min(b).min(c).cmplt(max)).all()
                    && (b - a).perp_dot(c - a).abs() > 1e-12)
                    .then_some(Triangle {
                        index: material_triangles.start + offset,
                        sources,
                        uv,
                    })
            })
            .collect();
        ensure!(
            !triangles.is_empty(),
            "body tint crop has no matching body triangles"
        );
        let (min, max) = triangles.iter().flat_map(|triangle| triangle.uv).fold(
            (Vec2::splat(f32::INFINITY), Vec2::splat(f32::NEG_INFINITY)),
            |(min, max), uv| (min.min(uv), max.max(uv)),
        );
        // Keep a one-texel gutter so the tile cannot sample neighboring atlas regions.
        let scale = Vec2::new((WIDTH - 2) as f32, (TILE_HEIGHT - 2) as f32) / (max - min);
        let origin = min - scale.recip();
        let mut texels = Vec::with_capacity(WIDTH * TILE_HEIGHT);
        for row in 0..TILE_HEIGHT {
            for column in 0..WIDTH {
                let uv = origin + Vec2::new(column as f32 + 0.5, row as f32 + 0.5) / scale;
                let (triangle, weights) = triangle_sample(&triangles, uv);
                texels.push(Texel {
                    triangle,
                    weights,
                    tint: mask.tint(rgba, uv),
                });
            }
        }
        let shade_uvs = Vec::with_capacity(triangles.len());
        Ok(Self {
            mask,
            atlas_origin,
            triangles,
            texels,
            origin,
            scale,
            shade_uvs,
        })
    }

    fn vertex_count(&self) -> usize {
        self.triangles.len() * 3
    }

    fn remap_indices(&self, indices: &mut [u32], base: u32) {
        for (slot, triangle) in self.triangles.iter().enumerate() {
            let first = base + (slot * 3) as u32;
            indices[triangle.index * 3..][..3].copy_from_slice(&[first, first + 1, first + 2]);
        }
    }

    fn vertices(&self) -> impl Iterator<Item = (usize, Vec2)> + '_ {
        self.triangles.iter().flat_map(|triangle| {
            triangle
                .sources
                .into_iter()
                .zip(triangle.uv.map(|uv| (uv - self.origin) * self.scale))
        })
    }

    fn write(
        &mut self,
        atlas: &mut [u8],
        extent: [usize; 2],
        top_row: Option<usize>,
        shade_uv: &impl Fn(usize) -> [f32; 2],
    ) {
        let [width, height] = extent;
        let [left, top] = self.atlas_origin;
        let dimensions = Vec2::new(width as f32, height as f32);
        self.shade_uvs.clear();
        self.shade_uvs.extend(self.triangles.iter().map(|triangle| {
            triangle
                .sources
                .map(|source| Vec2::from_array(shade_uv(source)) * dimensions)
        }));
        for (index, texel) in self.texels.iter().enumerate() {
            let [a, b, c] = self.shade_uvs[texel.triangle];
            let Vec3 { x, y, z } = texel.weights;
            let uv = a * x + b * y + c * z;
            let column = (uv.x as usize).min(width - 1);
            let row = (uv.y as usize).min(height - 1);
            let source = (row * width + column) * 4;
            let target = ((top + index / WIDTH) * width + left + index % WIDTH) * 4;
            let tint = if top_row
                .is_none_or(|start| Surface::at_row(row.saturating_sub(start)) == Surface::Skin)
            {
                texel.tint
            } else {
                Vec3::ONE
            };
            for (channel, factor) in tint.to_array().into_iter().enumerate() {
                atlas[target + channel] =
                    (f32::from(atlas[source + channel]) * factor).round() as u8;
            }
            atlas[target + 3] = atlas[source + 3];
        }
    }
}

fn triangle_sample(triangles: &[Triangle], uv: Vec2) -> (usize, Vec3) {
    let mut nearest = (0, Vec3::X);
    let mut best = f32::INFINITY;
    for (index, triangle) in triangles.iter().enumerate() {
        let weights = triangle.weights(uv);
        if weights.min_element() >= 0.0 {
            return (index, weights);
        }
        // The nearest sampler also sees texels just outside the triangle's UVs.
        // Extend its color from the closest edge to keep the tile filled there.
        for (a, b) in [(0, 1), (1, 2), (2, 0)] {
            let start = triangle.uv[a];
            let edge = triangle.uv[b] - start;
            let t = ((uv - start).dot(edge) / edge.length_squared()).clamp(0.0, 1.0);
            let mut weights = Vec3::ZERO;
            weights[a] = 1.0 - t;
            weights[b] = t;
            let distance = uv.distance_squared(triangle.point(weights));
            if distance < best {
                nearest = (index, weights);
                best = distance;
            }
        }
    }
    nearest
}
