//! Wing outlines extracted from `butter-closed-wings.svg`.

use core::mem::size_of;
use trueos::vgpu::{IndexedBatchDrawV2, IndexedDrawBatchV2, PRIMITIVE_TOPOLOGY_LINE_LOOP};

type Point = [f64; 2];

// Each path starts at the wing root. Every cubic contributes two control
// points and an endpoint. LINE_LOOP supplies each path's final straight edge.
const TOP_WING: [Point; 7] = [
    [0.014814138412475586, 0.4383479952812195],
    [0.34511280059814453, 0.019663125276565552],
    [0.7678900957107544, 0.07059453427791595],
    [0.6598089933395386, 0.26178503036499023],
    [0.459704726934433, 0.6157602667808533],
    [0.37837880849838257, 0.43880602717399597],
    [0.014814138412475586, 0.44999998807907104],
];
const BOTTOM_WING: [Point; 4] = [
    [0.014813999645411968, 0.44999998807907104],
    [0.9448009133338928, 0.3951452970504761],
    [0.24930904805660248, 1.2085520029067993],
    [0.014813999645411968, 0.4659808874130249],
];

const ROOT_Y: f64 = 0.45;
const SEGMENTS: usize = 64;
const PATHS: [&[Point]; 4] = [&TOP_WING, &BOTTOM_WING, &TOP_WING, &BOTTOM_WING];
const VERTEX_COUNT: usize = 2 * (vertex_count(&TOP_WING) + vertex_count(&BOTTOM_WING));
const VERTICES: [[f32; 2]; VERTEX_COUNT] = model_vertices();

pub const STRIDE: usize = 3 * size_of::<f32>();
pub const VERTEX_BYTES: usize = VERTEX_COUNT * STRIDE;
pub const INDEX_BYTES: [u8; VERTEX_COUNT * size_of::<u32>()] = index_bytes();

const fn vertex_count(path: &[Point]) -> usize {
    (path.len() - 1) / 3 * SEGMENTS + 1
}

const fn model_vertices() -> [[f32; 2]; VERTEX_COUNT] {
    let mut vertices = [[0.0; 2]; VERTEX_COUNT];
    let mut next = 0;
    let mut wing = 0;
    while wing < PATHS.len() {
        let path = PATHS[wing];
        let mirror = if wing < 2 { 1.0 } else { -1.0 };
        let root_x = path[0][0];
        vertices[next] = [0.0, (ROOT_Y - path[0][1]) as f32];
        next += 1;
        let mut curve = 0;
        while curve + 3 < path.len() {
            let mut step = 1;
            while step <= SEGMENTS {
                let t = step as f64 / SEGMENTS as f64;
                let u = 1.0 - t;
                let weights = [u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t];
                let mut point = [0.0; 2];
                let mut control = 0;
                while control < 4 {
                    let [x, y] = path[curve + control];
                    point[0] += x * weights[control];
                    point[1] += y * weights[control];
                    control += 1;
                }
                vertices[next] = [
                    ((point[0] - root_x) * mirror) as f32,
                    (ROOT_Y - point[1]) as f32,
                ];
                next += 1;
                step += 1;
            }
            curve += 3;
        }
        wing += 1;
    }
    vertices
}

const fn index_bytes() -> [u8; VERTEX_COUNT * size_of::<u32>()] {
    let mut bytes = [0; VERTEX_COUNT * size_of::<u32>()];
    let mut i = 0;
    while i < VERTEX_COUNT {
        let [a, b, c, d] = (i as u32).to_le_bytes();
        bytes[4 * i] = a;
        bytes[4 * i + 1] = b;
        bytes[4 * i + 2] = c;
        bytes[4 * i + 3] = d;
        i += 1;
    }
    bytes
}

pub fn vertex_bytes(width: u32, height: u32) -> [u8; VERTEX_BYTES] {
    let width = width.max(1) as f32;
    let height = height.max(1) as f32;
    let pixels_per_unit = (width / 1.56).min(height / 0.78);
    let sx = 2.0 * pixels_per_unit / width;
    let sy = 2.0 * pixels_per_unit / height;
    let mut bytes = [0; VERTEX_BYTES];
    for ([x, y], vertex) in VERTICES.into_iter().zip(bytes.chunks_exact_mut(STRIDE)) {
        vertex[0..4].copy_from_slice(&(x * sx).to_le_bytes());
        vertex[4..8].copy_from_slice(&(y * sy).to_le_bytes());
    }
    bytes
}

pub fn batch() -> IndexedDrawBatchV2 {
    let mut batch = IndexedDrawBatchV2 {
        clear_rgba8_srgb: u32::from_le_bytes([6, 8, 10, 255]),
        draw_count: PATHS.len() as u32,
        ..IndexedDrawBatchV2::default()
    };
    let mut first_index = 0;
    for (draw, path) in batch.draws.iter_mut().zip(PATHS) {
        let index_count = vertex_count(path) as u32;
        *draw = IndexedBatchDrawV2 {
            index_count,
            first_index,
            base_vertex: 0,
            rgba8_srgb: u32::from_le_bytes([255, 255, 255, 255]),
            topology: PRIMITIVE_TOPOLOGY_LINE_LOOP,
            reserved: 0,
        };
        first_index += index_count;
    }
    batch
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn baked_wings_share_a_hinge_and_draw_as_four_closed_loops() {
        let batch = batch();
        assert_eq!(batch.draw_count, 4);
        let mut first = 0;
        for (draw, count) in batch.draws.iter().zip([129, 65, 129, 65]) {
            assert_eq!(draw.topology, PRIMITIVE_TOPOLOGY_LINE_LOOP);
            assert_eq!((draw.first_index, draw.index_count), (first, count));
            assert_eq!(VERTICES[first as usize][0], 0.0);
            first += count;
            assert_eq!(VERTICES[first as usize - 1][0], 0.0);
        }
        assert_eq!(first as usize, VERTEX_COUNT);
        let (right, left) = VERTICES.split_at(VERTEX_COUNT / 2);
        assert!(right.iter().zip(left).all(|(&[x, y], &p)| p == [-x, y]));
        assert!(
            INDEX_BYTES
                .chunks_exact(4)
                .enumerate()
                .all(|(i, bytes)| { u32::from_le_bytes(bytes.try_into().unwrap()) == i as u32 })
        );
    }
}
