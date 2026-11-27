use std::collections::HashSet;

use anyhow::{Context, Result, ensure};
use glam::Vec3;

pub(super) struct Cutout {
    vertices: Vec<Vertex>,
    indices: Vec<u32>,
}

struct Vertex {
    sources: [usize; 3],
    weights: [f32; 3],
    uv: [f32; 2],
}

impl Cutout {
    pub(super) fn read(
        bytes: &[u8],
        source_triangles: impl Iterator<Item = [u32; 3]>,
        first_vertex: usize,
    ) -> Result<Self> {
        let source_triangles: HashSet<_> = source_triangles.map(triangle_key).collect();
        let (header, payload) = bytes
            .split_first_chunk::<8>()
            .context("missing cutout header")?;
        let vertex_count = read_u32(header) as usize;
        let index_count = read_u32(&header[4..]) as usize;
        let geometry_bytes = vertex_count
            .checked_mul(28)
            .context("cutout vertex count overflow")?;
        let (vertex_data, index_data) = payload
            .split_at_checked(geometry_bytes)
            .context("short cutout vertex data")?;
        ensure!(
            vertex_count > 0
                && index_count > 0
                && index_count.is_multiple_of(3)
                && index_count.checked_mul(4) == Some(index_data.len()),
            "invalid cutout extent"
        );
        let vertices = vertex_data
            .as_chunks::<28>()
            .0
            .iter()
            .map(|bytes| {
                let [a, b, c, w0, w1, u, v] = std::array::from_fn(|i| read_u32(&bytes[i * 4..]));
                let source = [a, b, c];
                ensure!(
                    source_triangles.contains(&triangle_key(source)),
                    "unknown cutout source triangle"
                );
                let [w0, w1] = [w0, w1].map(f32::from_bits);
                let weights = [w0, w1, 1.0 - w0 - w1];
                let uv = [u, v].map(f32::from_bits);
                ensure!(
                    weights
                        .iter()
                        .all(|value| (-1.0e-6..=1.000001).contains(value))
                        && uv.iter().all(|value| (0.0..=1.0).contains(value)),
                    "invalid cutout interpolation"
                );
                Ok(Vertex {
                    sources: source.map(|index| first_vertex + index as usize),
                    weights,
                    uv,
                })
            })
            .collect::<Result<_>>()?;
        let indices: Vec<_> = index_data
            .as_chunks::<4>()
            .0
            .iter()
            .copied()
            .map(u32::from_le_bytes)
            .collect();
        ensure!(
            indices.iter().all(|&index| (index as usize) < vertex_count),
            "cutout triangle outside its vertex range"
        );
        Ok(Self { vertices, indices })
    }

    pub(super) fn len(&self) -> usize {
        self.vertices.len()
    }

    pub(super) fn indices(&self) -> &[u32] {
        &self.indices
    }

    pub(super) fn vertices<'a>(
        &'a self,
        positions: &'a [[f32; 3]],
    ) -> impl Iterator<Item = ([f32; 3], [f32; 2])> + 'a {
        self.vertices.iter().map(move |vertex| {
            // Interpolate the posed triangle with the weights used to cut its UVs.
            let [a, b, c] = vertex
                .sources
                .map(|index| Vec3::from_array(positions[index]));
            let [u, v, w] = vertex.weights;
            ((a * u + b * v + c * w).to_array(), vertex.uv)
        })
    }
}

fn triangle_key(mut triangle: [u32; 3]) -> [u32; 3] {
    triangle.sort_unstable();
    triangle
}

fn read_u32(bytes: &[u8]) -> u32 {
    u32::from_le_bytes(bytes[..4].try_into().unwrap())
}
