use glam::Vec3;
use std::collections::HashMap;

const LEVEL: f32 = 0.5;
// Split every cube along the 0–7 diagonal with matching diagonals on shared faces.
const TETRAHEDRA: [[usize; 4]; 6] = [
    [0, 1, 3, 7],
    [0, 3, 2, 7],
    [0, 2, 6, 7],
    [0, 6, 4, 7],
    [0, 4, 5, 7],
    [0, 5, 1, 7],
];

#[derive(Clone, Copy)]
struct Corner {
    index: usize,
    position: Vec3,
    fill: f32,
}

pub(crate) fn extract(
    extent: [usize; 3],
    origin: Vec3,
    step: f32,
    fill: &[f32],
    positions: &mut Vec<Vec3>,
    triangles: &mut Vec<[u32; 3]>,
) {
    let previous_vertices = positions.len();
    positions.clear();
    triangles.clear();
    let [width, height, depth] = extent;
    let slice = width * height;
    assert_eq!(fill.len(), slice * depth);
    if width < 2 || height < 2 || depth < 2 {
        return;
    }

    let mut edges = HashMap::with_capacity(previous_vertices);
    let offsets = [
        0,
        1,
        width,
        width + 1,
        slice,
        slice + 1,
        slice + width,
        slice + width + 1,
    ];
    for z in 0..depth - 1 {
        for y in 0..height - 1 {
            for x in 0..width - 1 {
                let first = x + y * width + z * slice;
                let samples = offsets.map(|offset| fill[first + offset]);
                if samples.iter().all(|&value| value <= LEVEL)
                    || samples.iter().all(|&value| value > LEVEL)
                    || samples.iter().any(|value| !value.is_finite())
                {
                    continue;
                }
                let corners: [Corner; 8] = std::array::from_fn(|corner| {
                    let point = Vec3::new(
                        (x + (corner & 1)) as f32,
                        (y + ((corner >> 1) & 1)) as f32,
                        (z + (corner >> 2)) as f32,
                    );
                    Corner {
                        index: first + offsets[corner],
                        position: origin + step * point,
                        fill: samples[corner],
                    }
                });
                for tetrahedron in TETRAHEDRA {
                    march_tetrahedron(
                        tetrahedron.map(|corner| corners[corner]),
                        &mut edges,
                        positions,
                        triangles,
                    );
                }
            }
        }
    }
}

fn march_tetrahedron(
    corners: [Corner; 4],
    edges: &mut HashMap<[usize; 2], u32>,
    positions: &mut Vec<Vec3>,
    triangles: &mut Vec<[u32; 3]>,
) {
    let mut inside = [corners[0]; 4];
    let mut outside = inside;
    let mut inside_count = 0;
    let mut outside_count = 0;
    for corner in corners {
        if corner.fill > LEVEL {
            inside[inside_count] = corner;
            inside_count += 1;
        } else {
            outside[outside_count] = corner;
            outside_count += 1;
        }
    }

    let mut edge = |inner: Corner, outer: Corner| {
        // Coincident crossings at an exact-level lattice corner share its index.
        let key = if outer.fill == LEVEL {
            [outer.index; 2]
        } else {
            [inner.index.min(outer.index), inner.index.max(outer.index)]
        };
        *edges.entry(key).or_insert_with(|| {
            let t = (LEVEL - inner.fill) / (outer.fill - inner.fill);
            let index = positions.len() as u32;
            positions.push(inner.position.lerp(outer.position, t));
            index
        })
    };
    let (first, second) = match inside_count {
        1 => (
            [outside[0], outside[1], outside[2]].map(|point| edge(inside[0], point)),
            None,
        ),
        2 => {
            let a = edge(inside[0], outside[0]);
            let b = edge(inside[0], outside[1]);
            let c = edge(inside[1], outside[1]);
            let d = edge(inside[1], outside[0]);
            ([a, b, c], Some([a, c, d]))
        }
        3 => (
            [inside[0], inside[1], inside[2]].map(|point| edge(point, outside[0])),
            None,
        ),
        _ => return,
    };
    for mut triangle in [Some(first), second].into_iter().flatten() {
        let [a, b, c] = triangle.map(|index| positions[index as usize]);
        let normal = (b - a).cross(c - a);
        if normal.length_squared() == 0.0 {
            continue;
        }
        if normal.dot(inside[0].position - a) > 0.0 {
            triangle.swap(1, 2);
        }
        triangles.push(triangle);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn isolated_water_sample_has_a_closed_outward_surface() {
        let mut fill = [0.0; 27];
        fill[13] = 1.0;
        let mut positions = Vec::new();
        let mut triangles = Vec::new();
        extract(
            [3; 3],
            Vec3::ZERO,
            1.0,
            &fill,
            &mut positions,
            &mut triangles,
        );

        assert!(!triangles.is_empty());
        let mut incidence = HashMap::<[u32; 2], u32>::new();
        for [a, b, c] in triangles {
            for (u, v) in [(a, b), (b, c), (c, a)] {
                *incidence.entry([u.min(v), u.max(v)]).or_default() += 1;
            }
            let [a, b, c] = [a, b, c].map(|index| positions[index as usize]);
            assert!((b - a).cross(c - a).dot((a + b + c) / 3.0 - Vec3::ONE) > 0.0);
        }
        assert!(incidence.values().all(|&count| count == 2));
    }
}
