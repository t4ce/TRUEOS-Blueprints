use anyhow::{Context, Result, ensure};
use glam::{DVec3, Vec3};
use std::ops::Range;

use crate::{
    autobs::{ARC_LIMIT, BASE_YAW, CAMERA_PITCH, camera_back, view_back},
    hair::PARTICLES,
    hair_guides,
};

pub(super) const HAIR_MATERIAL: &str = "N00_000_Hair_00_HAIR_01 (Instance)";
pub(super) const BODY_MATERIAL: &str = "N00_000_00_Body_00_SKIN (Instance)";
pub(super) const FACE_MATERIAL: &str = "N00_000_00_Face_00_SKIN (Instance)";
pub(super) const PREVIEW_MATERIALS: [&str; 3] = [FACE_MATERIAL, BODY_MATERIAL, HAIR_MATERIAL];
const FRAME_FILL: f32 = 0.86;

pub(super) struct Mesh {
    vertices: Vec<[f32; 3]>,
    reference_extent: (u32, u32),
    #[cfg(target_os = "trueos")]
    pub observer_target: [f32; 3],
    #[cfg(target_os = "trueos")]
    pub pixels_per_unit: f32,
    #[cfg(target_os = "trueos")]
    pub material_vertices: Vec<Range<usize>>,
    #[cfg(target_os = "trueos")]
    pub material_triangles: Vec<Range<usize>>,
    #[cfg(target_os = "trueos")]
    pub skin_triangle_count: usize,
    pub indices: Vec<u32>,
    double_sided: Vec<bool>,
    pub triangle_count: usize,
    pub hair_triangle_count: usize,
    pub strand_guides: Vec<[Vec3; PARTICLES]>,
}

impl Mesh {
    pub fn vertex_count(&self) -> usize {
        self.vertices.len()
    }

    pub fn projected_vertices(
        &self,
        width: u32,
        height: u32,
    ) -> impl Iterator<Item = [f32; 3]> + '_ {
        // Keep the original pixel offsets from the center as the viewport changes.
        let (reference_width, reference_height) = self.reference_extent;
        let x_scale = reference_width as f32 / width as f32;
        let y_scale = reference_height as f32 / height as f32;
        self.vertices
            .iter()
            .map(move |&[x, y, z]| [x * x_scale, y * y_scale, z])
    }

    pub fn retained_triangles<'a>(
        &'a self,
        compact_indices: &'a [u32],
        positions: &'a [[f32; 3]],
        section: Range<usize>,
    ) -> impl Iterator<Item = [u32; 3]> + 'a {
        debug_assert_eq!(compact_indices.len(), self.indices.len());
        let center = view_back(BASE_YAW).as_dvec3();
        let horizontal = camera_back(BASE_YAW).as_dvec3() * f64::from(CAMERA_PITCH.cos());
        let side_margin = DVec3::Y.cross(horizontal) * f64::from(ARC_LIMIT.sin());
        let depth_margin = horizontal * f64::from(1.0 - ARC_LIMIT.cos());
        compact_indices.as_chunks::<3>().0[section.clone()]
            .iter()
            .zip(&self.double_sided[section])
            .flat_map(move |(&triangle, &double_sided)| {
                let (facing, reverse) = if double_sided {
                    camera_winding(triangle, positions, center, side_margin, depth_margin)
                } else {
                    (triangle, None)
                };
                std::iter::once(facing).chain(reverse)
            })
    }
}

fn camera_winding(
    triangle: [u32; 3],
    positions: &[[f32; 3]],
    center: DVec3,
    side_margin: DVec3,
    depth_margin: DVec3,
) -> ([u32; 3], Option<[u32; 3]>) {
    let [a, b, c] =
        triangle.map(|index| DVec3::from_array(positions[index as usize].map(f64::from)));
    let normal = (b - a).cross(c - a);
    let facing = normal.dot(center);
    let margin = normal.dot(side_margin).abs() + normal.dot(depth_margin).abs();
    let [a, b, c] = triangle;
    let reverse = [a, c, b];
    // Bound the change in facing across the pitched camera's yaw arc.
    if facing < -margin {
        (reverse, None)
    } else if facing >= margin {
        (triangle, None)
    } else {
        (triangle, Some(reverse))
    }
}

pub(super) fn prepare(glb: &[u8], width: u32, height: u32) -> Result<Mesh> {
    prepare_materials(glb, width, height, &PREVIEW_MATERIALS, false)
}

pub(super) fn prepare_materials(
    glb: &[u8],
    width: u32,
    height: u32,
    materials: &[&str],
    procedural_hair: bool,
) -> Result<Mesh> {
    ensure!(width != 0 && height != 0, "empty preview extent");
    let asset = gltf::Gltf::from_slice(glb).context("parse humanoid GLB")?;
    let blob = asset
        .blob
        .as_deref()
        .context("humanoid GLB has no binary buffer")?;
    let mut vertices = Vec::new();
    #[cfg(target_os = "trueos")]
    let mut material_vertices = Vec::with_capacity(materials.len());
    #[cfg(target_os = "trueos")]
    let mut material_triangles = Vec::with_capacity(materials.len());
    let mut indices = Vec::new();
    let mut double_sided = Vec::new();
    let mut skin_index_count = 0;
    let mut hair_triangle_count = 0;
    let mut strand_guides = Vec::new();
    let back = camera_back(BASE_YAW);
    let right = Vec3::Y.cross(back);

    for &name in materials {
        let primitive = asset
            .meshes()
            .flat_map(|mesh| mesh.primitives())
            .find(|primitive| primitive.material().name() == Some(name))
            .with_context(|| format!("missing humanoid material {name}"))?;
        ensure!(
            primitive.mode() == gltf::mesh::Mode::Triangles,
            "expected skin triangles"
        );
        let reader = primitive
            .reader(|buffer| matches!(buffer.source(), gltf::buffer::Source::Bin).then_some(blob));
        let first_vertex = u32::try_from(vertices.len()).context("too many humanoid vertices")?;
        // The embedded rest skin transforms are identity. View the -Z front from 36 degrees.
        vertices.extend(
            reader
                .read_positions()
                .context("missing skin positions")?
                .map(|position| {
                    let position = Vec3::from_array(position);
                    [position.dot(right), position.y, position.dot(back)]
                }),
        );
        let vertex_count =
            u32::try_from(vertices.len()).context("too many humanoid vertices")? - first_vertex;
        #[cfg(target_os = "trueos")]
        material_vertices.push(first_vertex as usize..vertices.len());
        let start = indices.len();
        let replacement = (procedural_hair && name == HAIR_MATERIAL)
            .then(|| hair_guides::prepare(&primitive, blob))
            .transpose()?;
        let source_indices = reader
            .read_indices()
            .context("missing skin indices")?
            .into_u32();
        if let Some(guides) = &replacement {
            ensure!(
                guides.replaced.len() * 3 == source_indices.len(),
                "hair guide mask does not match its mesh"
            );
        }
        for (corner, index) in source_indices.enumerate() {
            ensure!(index < vertex_count, "skin index outside its vertex range");
            if replacement
                .as_ref()
                .is_none_or(|guides| !guides.replaced[corner / 3])
            {
                indices.push(first_vertex + index);
            }
        }
        if let Some(guides) = replacement {
            strand_guides = guides.strands;
        }
        let index_count = indices.len() - start;
        ensure!(
            index_count != 0 && index_count.is_multiple_of(3),
            "invalid skin index count"
        );
        #[cfg(target_os = "trueos")]
        material_triangles.push(start / 3..indices.len() / 3);
        if name == HAIR_MATERIAL {
            hair_triangle_count = index_count / 3;
        } else if matches!(name, FACE_MATERIAL | BODY_MATERIAL) {
            skin_index_count = indices.len();
        }
        double_sided.extend(std::iter::repeat_n(
            primitive.material().double_sided(),
            index_count / 3,
        ));
    }

    // Frame the referenced skin vertices so hair does not change the rest view's fit.
    let mut minimum = Vec3::splat(f32::INFINITY);
    let mut maximum = Vec3::splat(f32::NEG_INFINITY);
    for &index in &indices[..skin_index_count] {
        let position = Vec3::from_array(vertices[index as usize]);
        minimum = minimum.min(position);
        maximum = maximum.max(position);
    }
    let span = maximum - minimum;
    ensure!(
        span.is_finite() && span.x > 0.0 && span.y > 0.0,
        "empty humanoid projection"
    );
    let reference_extent = (width, height);
    let (width, height) = (width as f32, height as f32);
    let scale = FRAME_FILL * (width / span.x).min(height / span.y);
    let center = (minimum + maximum) * 0.5;
    for [x, y, z] in &mut vertices {
        *x = (*x - center.x) * (2.0 * scale / width);
        *y = (*y - center.y) * (2.0 * scale / height);
        *z = 0.5;
        ensure!(
            x.is_finite() && y.is_finite(),
            "nonfinite humanoid position"
        );
    }
    let triangle_count = indices.len() / 3;
    Ok(Mesh {
        indices,
        double_sided,
        vertices,
        reference_extent,
        #[cfg(target_os = "trueos")]
        observer_target: (right * center.x + Vec3::Y * center.y + back * center.z).to_array(),
        #[cfg(target_os = "trueos")]
        pixels_per_unit: scale,
        #[cfg(target_os = "trueos")]
        material_vertices,
        #[cfg(target_os = "trueos")]
        material_triangles,
        #[cfg(target_os = "trueos")]
        skin_triangle_count: skin_index_count / 3,
        triangle_count,
        hair_triangle_count,
        strand_guides,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn procedural_preview_replaces_only_the_long_back_hair_indices() {
        let glb = crate::HUMANOID_GLB;
        let reference = prepare(glb, 640, 560).unwrap();
        let procedural = prepare_materials(glb, 640, 560, &PREVIEW_MATERIALS, true).unwrap();
        assert_eq!(procedural.vertices, reference.vertices);
        assert_eq!(procedural.hair_triangle_count, 3_589);
        assert_eq!(procedural.strand_guides.len(), 28);
        let hair_start = reference.triangle_count - reference.hair_triangle_count;
        let triangles = reference.indices.as_chunks::<3>().0;
        let expected = triangles[..hair_start + 1_770]
            .iter()
            .chain(&triangles[hair_start + 4_194..hair_start + 6_013]);
        assert!(procedural.indices.as_chunks::<3>().0.iter().eq(expected));
    }

    #[test]
    fn body_winding_and_double_sided_coverage_survive_the_observer_arc() {
        let glb = include_bytes!("../assets/AvatarSample_B.glb");
        let asset = gltf::Gltf::from_slice(glb).unwrap();
        let mut source_indices = Vec::new();
        let mut positions = Vec::new();
        for (material, first_vertex) in PREVIEW_MATERIALS.into_iter().zip([0, 4_171, 12_706]) {
            let primitive = asset
                .meshes()
                .flat_map(|mesh| mesh.primitives())
                .find(|primitive| primitive.material().name() == Some(material))
                .unwrap();
            let reader = primitive.reader(|_| asset.blob.as_deref());
            positions.extend(reader.read_positions().unwrap());
            source_indices.extend(
                reader
                    .read_indices()
                    .unwrap()
                    .into_u32()
                    .map(|index| first_vertex + index),
            );
        }
        let source_triangles = source_indices.as_chunks::<3>().0;
        let (face, body_and_hair) = source_triangles.split_at(4_232);
        let (body, hair) = body_and_hair.split_at(10_022);
        assert_eq!(hair.len(), 11_669);
        let mesh = prepare(glb, 640, 560).unwrap();
        assert_eq!(mesh.vertex_count(), 4_171 + 8_535 + 9_263);
        assert_eq!(mesh.triangle_count, face.len() + body.len() + hair.len());
        assert_eq!(mesh.hair_triangle_count, hair.len());
        assert_eq!(mesh.indices, source_indices);
        let body_start = face.len();
        let hair_start = body_start + body.len();
        let retained_body =
            mesh.retained_triangles(&mesh.indices, &positions, body_start..hair_start);
        assert!(retained_body.eq(body.iter().copied()));
        let retained_hair =
            mesh.retained_triangles(&mesh.indices, &positions, hair_start..mesh.triangle_count);
        assert!(retained_hair.flatten().all(|index| index >= 12_706));
        let retained: Vec<_> = mesh
            .retained_triangles(&mesh.indices, &positions, 0..mesh.triangle_count)
            .collect();
        assert!(retained.len() > mesh.triangle_count);
        assert!(retained.len() < mesh.triangle_count + face.len() + hair.len());
        for step in 0..=8 {
            let yaw = BASE_YAW + ARC_LIMIT * (step as f32 / 4.0 - 1.0);
            let back = view_back(yaw).as_dvec3();
            let facing = |triangle: [u32; 3]| {
                let [a, b, c] = triangle
                    .map(|index| DVec3::from_array(positions[index as usize].map(f64::from)));
                (b - a).cross(c - a).dot(back)
            };
            let expected = source_triangles.iter().enumerate().filter_map(|(i, &t)| {
                if facing(t) > 0.0 {
                    Some(t)
                } else if (i < face.len() || i >= face.len() + body.len()) && facing(t) < 0.0 {
                    Some([t[0], t[2], t[1]])
                } else {
                    None
                }
            });
            assert!(
                retained
                    .iter()
                    .copied()
                    .filter(|&t| facing(t) > 0.0)
                    .eq(expected)
            );
        }
    }

    #[test]
    fn adding_hair_keeps_the_body_fit() {
        let frame_aspect = |width, height| {
            let mesh = prepare(crate::HUMANOID_GLB, width, height).unwrap();
            let mut bound_x = 0.0_f32;
            let mut bound_y = 0.0_f32;
            let skin_indices = (mesh.triangle_count - mesh.hair_triangle_count) * 3;
            let (skin, hair) = mesh.indices.split_at(skin_indices);
            for &index in skin {
                let [x, y, z] = mesh.vertices[index as usize];
                assert_eq!(z, 0.5);
                bound_x = bound_x.max(x.abs());
                bound_y = bound_y.max(y.abs());
            }
            assert!((bound_x.max(bound_y) - FRAME_FILL).abs() < 0.000_01);
            assert!(hair.iter().all(|&index| {
                let [x, y, _] = mesh.vertices[index as usize];
                x.abs() <= 1.0 && y.abs() <= 1.0
            }));
            bound_x * width as f32 / (bound_y * height as f32)
        };
        assert!((frame_aspect(640, 560) - frame_aspect(240, 560)).abs() < 0.000_01);
    }

    #[test]
    fn resizing_keeps_the_avatar_centered_at_its_initial_pixel_size() {
        let mesh = prepare(include_bytes!("../assets/AvatarSample_B.glb"), 640, 560).unwrap();
        let pixel_positions = |width, height| {
            mesh.projected_vertices(width, height)
                .map(move |[x, y, z]| [x * width as f32 * 0.5, y * height as f32 * 0.5, z])
        };
        let initial: Vec<_> = pixel_positions(640, 560).collect();
        for (width, height) in [(1920, 1080), (240, 560), (640, 160), (640, 560)] {
            for ([x, y, z], &[initial_x, initial_y, initial_z]) in
                pixel_positions(width, height).zip(&initial)
            {
                assert!((x - initial_x).abs() < 0.000_1);
                assert!((y - initial_y).abs() < 0.000_1);
                assert_eq!(z, initial_z);
            }
        }
        let small_frame: Vec<_> = mesh.projected_vertices(640, 160).collect();
        assert!(
            mesh.indices
                .iter()
                .any(|&index| small_frame[index as usize][1].abs() > 1.0)
        );
    }
}
