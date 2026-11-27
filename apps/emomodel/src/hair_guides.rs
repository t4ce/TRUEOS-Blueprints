use anyhow::{Context, Result, ensure};
use glam::{Vec2, Vec3};
use std::collections::{BTreeMap, HashMap};

use crate::hair::PARTICLES;

const STRAND_BUDGET: usize = 28;
const POSITION_WELD_SCALE: f32 = 1_000_000.0;

pub(super) struct HairGuides {
    pub(super) replaced: Vec<bool>,
    pub(super) strands: Vec<[Vec3; PARTICLES]>,
}

pub(super) fn prepare(primitive: &gltf::Primitive<'_>, blob: &[u8]) -> Result<HairGuides> {
    let reader = primitive
        .reader(|buffer| matches!(buffer.source(), gltf::buffer::Source::Bin).then_some(blob));
    let positions: Vec<_> = reader
        .read_positions()
        .context("missing hair guide positions")?
        .map(Vec3::from_array)
        .collect();
    let indices: Vec<_> = reader
        .read_indices()
        .context("missing hair guide triangles")?
        .into_u32()
        .map(|index| index as usize)
        .collect();
    let (triangles, remainder) = indices.as_chunks::<3>();
    ensure!(remainder.is_empty(), "incomplete hair guide triangle");
    ensure!(
        triangles
            .iter()
            .flatten()
            .all(|&index| index < positions.len()),
        "hair guide triangle outside its vertex range"
    );
    ensure!(
        positions.iter().all(|p| p.is_finite()),
        "nonfinite hair guide position"
    );

    let mut replaced = vec![false; triangles.len()];
    let islands = welded_islands(&positions, triangles);
    let mut selected = Vec::new();
    for island in islands.values() {
        let minimum = island
            .iter()
            .flat_map(|&triangle| triangles[triangle])
            .fold(Vec3::splat(f32::INFINITY), |min, vertex| {
                min.min(positions[vertex])
            });
        // In AvatarSample_B, Y is up and +Z is behind the face. Only the long
        // back locks reach below Y=1 entirely behind the model's origin plane.
        if minimum.y < 1.0 && minimum.z > 0.0 {
            for &triangle in island {
                replaced[triangle] = true;
            }
            selected.push(island);
        }
    }
    let mut strands = Vec::with_capacity(STRAND_BUDGET);
    if selected.is_empty() {
        return Ok(HairGuides { replaced, strands });
    }

    let uv: Vec<_> = reader
        .read_tex_coords(0)
        .context("missing hair guide UV coordinates")?
        .into_f32()
        .map(Vec2::from_array)
        .collect();
    ensure!(
        uv.len() == positions.len() && uv.iter().all(|p| p.is_finite()),
        "invalid hair guide UV coordinates"
    );

    let surfaces = selected
        .into_iter()
        .map(|island| Surface::new(&positions, &uv, triangles, island))
        .collect::<Result<Vec<_>>>()?;
    let perimeters = surfaces
        .iter()
        .map(Surface::perimeter)
        .collect::<Result<Vec<_>>>()?;
    let total_perimeter: f32 = perimeters.iter().sum();
    ensure!(total_perimeter > 0.0, "hair guide surface has no width");
    // Represent each lock first, then put more guides on the wider locks.
    let minimum_per_island = usize::from(surfaces.len() <= STRAND_BUDGET);
    let distributed = STRAND_BUDGET - surfaces.len() * minimum_per_island;
    let mut accumulated = 0.0;
    let mut assigned = 0;
    for (surface, perimeter) in surfaces.iter().zip(perimeters) {
        accumulated += perimeter;
        let cumulative_count =
            (distributed as f32 * accumulated / total_perimeter).round() as usize;
        let count = minimum_per_island + cumulative_count - assigned;
        assigned = cumulative_count;
        for strand in 0..count {
            let across = (strand as f32 + 0.5) / count as f32;
            let tip = [1.0, 0.975, 0.99, 0.965][strand % 4];
            let mut guide = [Vec3::ZERO; PARTICLES];
            for (node, position) in guide.iter_mut().enumerate() {
                let along = node as f32 / (PARTICLES - 1) as f32 * tip;
                *position = surface.sample(Vec2::new(across, along))?;
            }
            strands.push(guide);
        }
    }
    Ok(HairGuides { replaced, strands })
}

fn welded_islands(positions: &[Vec3], triangles: &[[usize; 3]]) -> BTreeMap<usize, Vec<usize>> {
    let mut parents: Vec<_> = (0..positions.len()).collect();
    let mut welded = HashMap::with_capacity(positions.len());
    for (vertex, &position) in positions.iter().enumerate() {
        // The exported surface has coincident splits along its UV and normal seams.
        let key = (position * POSITION_WELD_SCALE)
            .round()
            .as_ivec3()
            .to_array();
        let first = *welded.entry(key).or_insert(vertex);
        join(&mut parents, first, vertex);
    }
    for &[a, b, c] in triangles {
        join(&mut parents, a, b);
        join(&mut parents, a, c);
    }
    let mut islands = BTreeMap::<_, Vec<_>>::new();
    for (triangle, &[a, _, _]) in triangles.iter().enumerate() {
        islands
            .entry(root(&mut parents, a))
            .or_default()
            .push(triangle);
    }
    islands
}

fn root(parents: &mut [usize], mut vertex: usize) -> usize {
    while vertex != parents[vertex] {
        parents[vertex] = parents[parents[vertex]];
        vertex = parents[vertex];
    }
    vertex
}

fn join(parents: &mut [usize], a: usize, b: usize) {
    let a = root(parents, a);
    let b = root(parents, b);
    parents[a.max(b)] = a.min(b);
}

struct Surface<'a> {
    positions: &'a [Vec3],
    uv: &'a [Vec2],
    triangles: &'a [[usize; 3]],
    island: &'a [usize],
    minimum: Vec2,
    span: Vec2,
}

impl<'a> Surface<'a> {
    fn new(
        positions: &'a [Vec3],
        uv: &'a [Vec2],
        triangles: &'a [[usize; 3]],
        island: &'a [usize],
    ) -> Result<Self> {
        let mut minimum = Vec2::splat(f32::INFINITY);
        let mut maximum = Vec2::splat(f32::NEG_INFINITY);
        for vertex in island.iter().flat_map(|&triangle| triangles[triangle]) {
            minimum = minimum.min(uv[vertex]);
            maximum = maximum.max(uv[vertex]);
        }
        let span = maximum - minimum;
        ensure!(span.min_element() > 0.0, "empty hair guide UV chart");
        Ok(Self {
            positions,
            uv,
            triangles,
            island,
            minimum,
            span,
        })
    }

    fn sample(&self, parameter: Vec2) -> Result<Vec3> {
        // These UV charts unwrap each lock around U and run root to tip along V.
        // Barycentric interpolation puts each guide directly on the authored surface.
        let point = self.minimum + self.span * parameter;
        self.island
            .iter()
            .find_map(|&triangle| {
                let vertices = self.triangles[triangle];
                let [a, b, c] = vertices.map(|vertex| self.uv[vertex]);
                let ab = b - a;
                let ac = c - a;
                let area = ab.perp_dot(ac);
                if area.abs() < 1.0e-9 {
                    return None;
                }
                let offset = point - a;
                let wb = offset.perp_dot(ac) / area;
                let wc = ab.perp_dot(offset) / area;
                let wa = 1.0 - wb - wc;
                (wa.min(wb).min(wc) >= -0.0001).then(|| {
                    let [a, b, c] = vertices.map(|vertex| self.positions[vertex]);
                    wa * a + wb * b + wc * c
                })
            })
            .context("uncovered point on hair guide UV chart")
    }

    fn perimeter(&self) -> Result<f32> {
        let mut widest = 0.0_f32;
        for along in [0.35, 0.55, 0.75] {
            let mut previous = self.sample(Vec2::new(0.0, along))?;
            let mut perimeter = 0.0;
            for across in [0.25, 0.5, 0.75, 1.0] {
                let next = self.sample(Vec2::new(across, along))?;
                perimeter += previous.distance(next);
                previous = next;
            }
            widest = widest.max(perimeter);
        }
        Ok(widest)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_back_hair_keeps_its_root_to_tip_coverage_and_other_hair_triangles() {
        let asset = gltf::Gltf::from_slice(crate::HUMANOID_GLB).unwrap();
        let primitive = asset
            .meshes()
            .flat_map(|mesh| mesh.primitives())
            .find(|primitive| primitive.material().name() == Some(crate::mesh::HAIR_MATERIAL))
            .unwrap();
        let guides = prepare(&primitive, asset.blob.as_deref().unwrap()).unwrap();
        assert_eq!(guides.replaced.len(), 11_590);
        for (triangle, replaced) in guides.replaced.into_iter().enumerate() {
            assert_eq!(
                replaced,
                (1749..4173).contains(&triangle) || triangle >= 5934
            );
        }
        assert_eq!(guides.strands.len(), STRAND_BUDGET);
        let mut minimum = Vec3::splat(f32::INFINITY);
        let mut maximum = Vec3::splat(f32::NEG_INFINITY);
        for guide in &guides.strands {
            assert!((1.41..1.48).contains(&guide[0].y));
            assert!((0.73..0.92).contains(&guide[PARTICLES - 1].y));
            assert!(guide.windows(2).all(|pair| pair[0].y > pair[1].y));
            for &point in guide {
                assert!(point.is_finite());
                minimum = minimum.min(point);
                maximum = maximum.max(point);
            }
        }
        assert!(minimum.x < -0.28 && maximum.x > 0.28);
        assert!(minimum.y < 0.75 && maximum.y > 1.47);
        assert!(minimum.z > 0.06 && maximum.z > 0.31);
    }
}
