use anyhow::{Context, Result, ensure};
use glam::{Vec2, Vec3};

pub(super) fn bake(glb: &[u8], material: &str, rgba: &[u8], size: usize) -> Result<Vec<[f32; 3]>> {
    ensure!(
        size != 0
            && size.checked_mul(size).and_then(|area| area.checked_mul(4)) == Some(rgba.len()),
        "invalid square RGBA normal map"
    );
    let asset = gltf::Gltf::from_slice(glb).context("parse normal-map mesh")?;
    let primitive = asset
        .meshes()
        .flat_map(|mesh| mesh.primitives())
        .find(|primitive| primitive.material().name() == Some(material))
        .with_context(|| format!("missing material {material} for normal detail"))?;
    ensure!(
        primitive.mode() == gltf::mesh::Mode::Triangles,
        "normal detail requires triangles"
    );
    let blob = asset
        .blob
        .as_deref()
        .context("normal-map mesh has no BIN chunk")?;
    let reader = primitive
        .reader(|buffer| matches!(buffer.source(), gltf::buffer::Source::Bin).then_some(blob));
    let positions = reader
        .read_positions()
        .context("missing normal-map positions")?
        .map(Vec3::from_array)
        .collect::<Vec<_>>();
    let normals = reader
        .read_normals()
        .context("missing normal-map normals")?
        .map(Vec3::from_array)
        .collect::<Vec<_>>();
    let texcoords = reader
        .read_tex_coords(0)
        .context("missing normal-map TEXCOORD_0")?
        .into_f32()
        .map(Vec2::from_array)
        .collect::<Vec<_>>();
    let indices = reader
        .read_indices()
        .context("missing normal-map triangle indices")?
        .into_u32()
        .map(|index| index as usize)
        .collect::<Vec<_>>();
    ensure!(
        positions.len() == normals.len()
            && positions.len() == texcoords.len()
            && positions.iter().all(|p| p.is_finite())
            && normals.iter().all(|n| {
                let length_squared = n.length_squared();
                length_squared.is_finite() && length_squared > f32::EPSILON
            })
            && texcoords.iter().all(|uv| uv.is_finite()),
        "invalid normal-map vertex attributes"
    );
    ensure!(
        !indices.is_empty()
            && indices.len().is_multiple_of(3)
            && indices.iter().all(|&index| index < positions.len()),
        "invalid normal-map triangle indices"
    );

    let mut tangents = vec![Vec3::ZERO; positions.len()];
    let mut bitangents = vec![Vec3::ZERO; positions.len()];
    for &vertices in indices.as_chunks::<3>().0 {
        let [a, b, c] = vertices.map(|index| positions[index]);
        let [start, right, down] = vertices.map(|index| texcoords[index]);
        let uv_ab = right - start;
        let uv_ac = down - start;
        let determinant = uv_ab.perp_dot(uv_ac);
        if determinant.abs() <= 1e-12 {
            continue;
        }
        let tangent = ((b - a) * uv_ac.y - (c - a) * uv_ab.y) / determinant;
        let bitangent = ((c - a) * uv_ab.x - (b - a) * uv_ac.x) / determinant;
        let (Some(tangent), Some(bitangent)) = (tangent.try_normalize(), bitangent.try_normalize())
        else {
            continue;
        };
        for (index, at, first, second) in [
            (vertices[0], a, b, c),
            (vertices[1], b, c, a),
            (vertices[2], c, a, b),
        ] {
            // Corner angles weight contributions consistently across uneven triangles.
            let angle = (first - at).angle_between(second - at);
            if angle.is_finite() {
                tangents[index] += tangent * angle;
                bitangents[index] += bitangent * angle;
            }
        }
    }

    Ok(normals
        .into_iter()
        .enumerate()
        .map(|(index, authored)| {
            let normal = authored.normalize();
            let tangent = tangents[index];
            let Some(tangent) = (tangent - normal * normal.dot(tangent)).try_normalize() else {
                // The asset accessors include vertices unused by this material.
                return authored.to_array();
            };
            let bitangent = normal.cross(tangent);
            let bitangent = bitangent * bitangent.dot(bitangents[index]).signum();
            let detail = sample_normal(rgba, size, texcoords[index]);
            ((tangent * detail.x + bitangent * detail.y + normal * detail.z)
                .try_normalize()
                .unwrap_or(authored))
            .to_array()
        })
        .collect())
}

fn sample_normal(rgba: &[u8], size: usize, uv: Vec2) -> Vec3 {
    let uv = Vec2::new(uv.x.rem_euclid(1.0), uv.y.rem_euclid(1.0));
    let pixel = uv * size as f32 - Vec2::splat(0.5);
    let lower = pixel.floor();
    let blend = pixel - lower;
    let x = lower.x as i64;
    let y = lower.y as i64;
    let read = |x: i64, y: i64| {
        let x = x.rem_euclid(size as i64) as usize;
        let y = y.rem_euclid(size as i64) as usize;
        let offset = (y * size + x) * 4;
        let rgb = &rgba[offset..offset + 3];
        if rgb == [0, 0, 128] {
            // This atlas uses a sentinel outside its UV islands.
            return Vec3::Z;
        }
        Vec3::new(f32::from(rgb[0]), f32::from(rgb[1]), f32::from(rgb[2])) * (2.0 / 255.0)
            - Vec3::ONE
    };
    let top = read(x, y).lerp(read(x + 1, y), blend.x);
    let bottom = read(x, y + 1).lerp(read(x + 1, y + 1), blend.x);
    top.lerp(bottom, blend.y)
}
