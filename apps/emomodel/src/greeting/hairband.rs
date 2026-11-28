use std::ops::Range;

use anyhow::{Context, Result, ensure};
use glam::Vec3;

use crate::{
    cutout::Cutout,
    mesh::{HAIR_MATERIAL, is_hair_accessory_triangle},
};

#[cfg(target_os = "trueos")]
use crate::mesh::Mesh;
#[cfg(target_os = "trueos")]
use trueos::vmedia::{self, ImageFormat};

use super::sky_light::{SkyLight, encode_rgb, linear_rgb};

pub(super) const WIDTH: usize = 371;
pub(super) const HEIGHT: usize = 268;
const PNG: &[u8] = include_bytes!("../../assets/AvatarSample_B-hairband.png");
const CUTOUT: &[u8] = include_bytes!("../../assets/AvatarSample_B-hairband-cutout.bin");

#[derive(Clone, Copy)]
struct Finish {
    roughness: f32,
    metallic: f32,
}

const PLASTIC: Finish = Finish {
    roughness: 0.65,
    metallic: 0.0,
};
const METAL: Finish = Finish {
    roughness: 0.24,
    metallic: 0.9,
};

impl Finish {
    fn lighting(self, normal: Vec3, view: Vec3, sky: &SkyLight) -> (Vec3, Vec3) {
        let key = sky.key_direction();
        let direct = normal.dot(key).clamp(0.0, 1.0);
        let half = (key + view).normalize_or_zero();
        let exponent = 2.0 / self.roughness.powi(2);
        let highlight = normal.dot(half).clamp(0.0, 1.0).powf(exponent) * direct;
        let dielectric = 1.0 - self.metallic;
        let tint = sky.key.color();
        let base = Vec3::splat(0.68)
            + tint * (0.48 * direct * dielectric + 0.84 * highlight * self.metallic);
        (base, tint * (0.08 * highlight * dielectric))
    }
}

struct PieceLayout {
    // Local render vertices from the canonical hair primitive.
    vertices: Range<usize>,
    finish: Finish,
    source: [usize; 2],
    size: [usize; 2],
    target: [usize; 2],
}

const STAR: [usize; 2] = [217, 45];
const STAR_SIZE: [usize; 2] = [53, 63];
const PINS: [usize; 2] = [329, 21];
const PIN_SIZE: [usize; 2] = [39, 121];
// The two stars and three pin assemblies reuse texels. The spare tiles give
// each piece its own lighting without changing the atlas extent or draw count.
const PIECES: [PieceLayout; 6] = [
    PieceLayout {
        vertices: 526..535,
        finish: METAL,
        source: STAR,
        size: STAR_SIZE,
        target: STAR,
    },
    PieceLayout {
        vertices: 535..543,
        finish: METAL,
        source: STAR,
        size: STAR_SIZE,
        target: [215, 155],
    },
    PieceLayout {
        vertices: 543..559,
        finish: PLASTIC,
        source: PINS,
        size: PIN_SIZE,
        target: PINS,
    },
    PieceLayout {
        vertices: 3427..3439,
        finish: PLASTIC,
        source: PINS,
        size: PIN_SIZE,
        target: [271, 145],
    },
    PieceLayout {
        vertices: 3439..3451,
        finish: PLASTIC,
        source: PINS,
        size: PIN_SIZE,
        target: [312, 145],
    },
    PieceLayout {
        vertices: 4903..4911,
        finish: METAL,
        source: [160, 25],
        size: [53, 63],
        target: [160, 25],
    },
];
const CLASP_VERTICES: [usize; 5] = [4095, 4097, 4104, 4109, 4116];

struct Piece {
    vertices: Vec<usize>,
    finish: Finish,
    texels: Vec<Texel>,
}

struct Texel {
    xy: [usize; 2],
    base: Vec3,
    coverage: f32,
}

pub(super) struct Hairband {
    cutout: Cutout,
    rgba: Vec<u8>,
    uvs: Vec<[f32; 2]>,
    pieces: Vec<Piece>,
}

impl Hairband {
    #[cfg(target_os = "trueos")]
    pub(super) async fn load(glb: &[u8], mesh: &Mesh) -> Result<Self> {
        let hair = mesh.material_vertices[2].clone();
        let cutout = cutout(glb, hair.clone())?;
        let image = vmedia::decode(ImageFormat::Png, PNG)
            .await
            .map_err(super::gpu_error)
            .context("decode the hair accessory texture")?;
        ensure!(
            image.info.width as usize == WIDTH
                && image.info.height as usize == HEIGHT
                && image.info.stride_bytes as usize == WIDTH * 4
                && image.rgba.len() == WIDTH * HEIGHT * 4,
            "unexpected hair accessory texture extent"
        );
        Ok(Self::from_rgba(cutout, hair.start, image.rgba))
    }

    fn from_rgba(cutout: Cutout, first_vertex: usize, mut rgba: Vec<u8>) -> Self {
        let [width, height] = [WIDTH as f32, HEIGHT as f32];
        let uvs = cutout
            .source_uvs()
            .map(|(vertex, uv)| {
                PIECES
                    .iter()
                    .find(|piece| piece.vertices.contains(&(vertex - first_vertex)))
                    .map_or(uv, |piece| {
                        let [x, y] = piece.source;
                        let [u, v] = piece.target;
                        [
                            uv[0] + (u as f32 - x as f32) / width,
                            uv[1] + (v as f32 - y as f32) / height,
                        ]
                    })
            })
            .collect();
        let mut pieces: Vec<_> = PIECES
            .iter()
            .map(|layout| {
                let mut texels = Vec::new();
                for y in 0..layout.size[1] {
                    for x in 0..layout.size[0] {
                        let from = 4 * ((layout.source[1] + y) * WIDTH + layout.source[0] + x);
                        let xy = [layout.target[0] + x, layout.target[1] + y];
                        let to = 4 * (xy[1] * WIDTH + xy[0]);
                        let [r, g, b, a] = rgba[from..from + 4].try_into().unwrap();
                        rgba[to..to + 4].copy_from_slice(&[r, g, b, a]);
                        // Include the edge colors used by bilinear filtering.
                        if [r, g, b, a] != [0; 4] {
                            texels.push(Texel {
                                xy,
                                base: linear_rgb([r, g, b]),
                                coverage: 1.0,
                            });
                        }
                    }
                }
                Piece {
                    vertices: layout
                        .vertices
                        .clone()
                        .map(|index| first_vertex + index)
                        .collect(),
                    finish: layout.finish,
                    texels,
                }
            })
            .collect();
        let mut clasp = Vec::new();
        // The clasp shares the ribbon's mesh. Its isolated bright texels give
        // it a material boundary while retaining the dark fabric around it.
        for y in 191..=214 {
            for x in 82..=105 {
                let i = 4 * (y * WIDTH + x);
                let [r, g, b, a] = rgba[i..i + 4].try_into().unwrap();
                let coverage = ((f32::from(r.min(g).min(b)) - 65.0) / 25.0).clamp(0.0, 1.0);
                if a >= 128 && coverage > 0.0 {
                    clasp.push(Texel {
                        xy: [x, y],
                        base: linear_rgb([r, g, b]),
                        coverage,
                    });
                }
            }
        }
        pieces.push(Piece {
            vertices: CLASP_VERTICES.map(|index| first_vertex + index).to_vec(),
            finish: METAL,
            texels: clasp,
        });
        // The cutout mesh supplies coverage for both depth-writing renderers.
        for pixel in rgba.as_chunks_mut::<4>().0 {
            pixel[3] = 255;
        }
        Self {
            cutout,
            rgba,
            uvs,
            pieces,
        }
    }

    pub(super) fn len(&self) -> usize {
        self.cutout.len()
    }

    pub(super) fn indices(&self) -> &[u32] {
        self.cutout.indices()
    }

    pub(super) fn vertices<'a>(
        &'a self,
        positions: &'a [[f32; 3]],
    ) -> impl Iterator<Item = ([f32; 3], [f32; 2])> + 'a {
        self.cutout
            .vertices(positions)
            .zip(&self.uvs)
            .map(|((position, _), &uv)| (position, uv))
    }

    pub(super) fn rgba(&self) -> &[u8] {
        &self.rgba
    }

    pub(super) fn shade(
        &self,
        rgba: &mut [u8],
        stride: usize,
        [left, top]: [usize; 2],
        normals: &[[f32; 3]],
        view: Vec3,
        sky: &SkyLight,
    ) {
        for piece in &self.pieces {
            let normal = piece
                .vertices
                .iter()
                .map(|&index| Vec3::from_array(normals[index]))
                .sum::<Vec3>()
                .normalize_or_zero();
            // Each clip is a small, nearly planar piece. Evaluate the light
            // once per piece, then apply it to the original texture colors.
            let (base, highlight) = piece.finish.lighting(normal, view, sky);
            for &Texel {
                xy: [x, y],
                base: albedo,
                coverage,
            } in &piece.texels
            {
                let i = (top + y) * stride + (left + x) * 4;
                let lit = albedo.lerp(albedo * base + highlight, coverage);
                rgba[i..i + 3].copy_from_slice(&encode_rgb(lit));
            }
        }
    }
}

fn cutout(glb: &[u8], vertices: Range<usize>) -> Result<Cutout> {
    let asset = gltf::Gltf::from_slice(glb).context("parse the hair accessory mesh")?;
    let blob = asset
        .blob
        .as_deref()
        .context("hair mesh has no BIN chunk")?;
    let primitive = asset
        .meshes()
        .flat_map(|mesh| mesh.primitives())
        .find(|primitive| primitive.material().name() == Some(HAIR_MATERIAL))
        .context("missing hair material for the accessories")?;
    ensure!(
        primitive
            .get(&gltf::Semantic::Positions)
            .is_some_and(|positions| positions.count() == vertices.len()),
        "accessory vertex range differs from the hair mesh"
    );
    let reader = primitive
        .reader(|buffer| matches!(buffer.source(), gltf::buffer::Source::Bin).then_some(blob));
    let indices: Vec<_> = reader
        .read_indices()
        .context("missing hair mesh indices")?
        .into_u32()
        .collect();
    let (triangles, extra) = indices.as_chunks::<3>();
    ensure!(extra.is_empty(), "invalid hair mesh triangle indices");
    let selected = triangles
        .iter()
        .enumerate()
        .filter(|&(index, _)| is_hair_accessory_triangle(index))
        .map(|(_, &triangle)| triangle);
    Cutout::read(CUTOUT, selected, vertices.start).context("read the hair accessory cutout")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{HUMANOID_GLB, animation::Animation, mesh::PREVIEW_MATERIALS};

    #[test]
    fn metal_highlight_is_tighter_and_tinted_by_its_base_color() {
        let view = Vec3::NEG_Z;
        let sky = SkyLight::new(view, Default::default());
        let key = sky.key_direction();
        let facing = (key + view).normalize();
        let tilted = (facing + 0.55 * facing.cross(key).normalize()).normalize();
        let (metal_on, metal_glint) = METAL.lighting(facing, view, &sky);
        let (metal_off, _) = METAL.lighting(tilted, view, &sky);
        let (plastic_on, plastic_glint) = PLASTIC.lighting(facing, view, &sky);
        let (plastic_off, _) = PLASTIC.lighting(tilted, view, &sky);
        assert!(metal_on.x - metal_off.x > 0.4);
        assert!((plastic_on.x - plastic_off.x).abs() < 0.09);
        assert!(metal_glint.x < plastic_glint.x * 0.11);
    }

    #[test]
    fn authored_accessory_cutout_follows_the_skinned_hair() {
        let mut avatar = Animation::nod(HUMANOID_GLB, &PREVIEW_MATERIALS, None).unwrap();
        let first_hair = avatar.bind_positions(0).len() + avatar.bind_positions(1).len();
        let hair_vertices = first_hair..first_hair + avatar.bind_positions(2).len();
        let cutout = cutout(HUMANOID_GLB, hair_vertices).unwrap();
        let size = |offset| u32::from_be_bytes(PNG[offset..offset + 4].try_into().unwrap());
        assert_eq!([size(16) as usize, size(20) as usize], [WIDTH, HEIGHT]);
        let pixels = [170, 140, 120, 255].repeat(WIDTH * HEIGHT);
        let band = Hairband::from_rgba(cutout, first_hair, pixels);
        for ((vertex, original), &remapped) in band.cutout.source_uvs().zip(&band.uvs) {
            if let Some(piece) = PIECES
                .iter()
                .find(|piece| piece.vertices.contains(&(vertex - first_hair)))
            {
                for (axis, extent) in [WIDTH, HEIGHT].into_iter().enumerate() {
                    let px = remapped[axis] * extent as f32;
                    assert!(px > piece.target[axis] as f32);
                    assert!(px < (piece.target[axis] + piece.size[axis]) as f32);
                }
            } else {
                assert_eq!(remapped, original);
            }
        }
        let mut shaded = band.rgba().to_vec();
        let view = Vec3::NEG_Z;
        let sky = SkyLight::new(view, Default::default());
        band.shade(
            &mut shaded,
            WIDTH * 4,
            [0, 0],
            avatar.skin().normals,
            view,
            &sky,
        );
        for piece in &band.pieces {
            assert!(piece.texels.iter().any(|texel| {
                let [x, y] = texel.xy;
                let start = (y * WIDTH + x) * 4;
                shaded[start..start + 3] != band.rgba()[start..start + 3]
            }));
        }
        let cloth = (30 * WIDTH + 50) * 4;
        assert_eq!(shaded[cloth..cloth + 4], band.rgba()[cloth..cloth + 4]);
        let before: Vec<_> = band.vertices(avatar.skin().positions).collect();
        avatar.apply(1.5);
        let after: Vec<_> = band.vertices(avatar.skin().positions).collect();

        assert_eq!(before.len(), band.len());
        assert!(band.indices().len() > 260 * 3);
        assert!(before.iter().any(|&(_, [u, _])| u * WIDTH as f32 > 215.0));
        assert!(
            before
                .iter()
                .zip(after)
                .all(|(&(_, original_uv), (_, posed_uv))| original_uv == posed_uv)
        );
        assert!(band.vertices(avatar.skin().positions).zip(before).any(
            |((posed, _), (original, _))| {
                glam::Vec3::from_array(posed).distance(glam::Vec3::from_array(original)) > 0.005
            }
        ));
    }
}
