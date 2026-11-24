//! Wing-outline control points extracted from `butter-closed-wings.svg`.

type Point = [f64; 2];

// Each path starts at the wing root. Every cubic contributes two control
// points and an endpoint. A single line closes each path at the hinge.
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
const PATHS: [&[Point]; 2] = [&TOP_WING, &BOTTOM_WING];
pub const PATCH_COUNT: usize = 5;
pub const VERTEX_COUNT: usize = 4 * PATCH_COUNT;
const STRIDE: usize = 3 * size_of::<f32>();

const CONTROL_POINTS: [[f32; 3]; VERTEX_COUNT] = control_points();
pub const VERTEX_BYTES: [u8; VERTEX_COUNT * STRIDE] = vertex_bytes();
pub const INDEX_BYTES: [u8; VERTEX_COUNT * size_of::<u32>()] = index_bytes();

const fn control_points() -> [[f32; 3]; VERTEX_COUNT] {
    let mut controls = [[0.0; 3]; VERTEX_COUNT];
    let mut wing = 0;
    let mut next = 0;
    while wing < PATHS.len() {
        let path = PATHS[wing];
        let root_x = path[0][0];
        let mut curve = 0;
        while curve < path.len() - 1 {
            let mut point = 0;
            while point < 4 {
                controls[next] = control(path[curve + point], root_x, 0.0);
                next += 1;
                point += 1;
            }
            curve += 3;
        }
        let last = control(path[path.len() - 1], root_x, 1.0);
        let first = control(path[0], root_x, 1.0);
        controls[next] = last;
        controls[next + 1] = last;
        controls[next + 2] = first;
        controls[next + 3] = first;
        next += 4;
        wing += 1;
    }
    assert!(next == VERTEX_COUNT);
    controls
}

const fn control([x, y]: Point, root_x: f64, straight_edge: f32) -> [f32; 3] {
    [(x - root_x) as f32, (ROOT_Y - y) as f32, straight_edge]
}

const fn vertex_bytes() -> [u8; VERTEX_COUNT * STRIDE] {
    let mut bytes = [0; VERTEX_COUNT * STRIDE];
    let mut i = 0;
    while i < VERTEX_COUNT * 3 {
        let [a, b, c, d] = CONTROL_POINTS[i / 3][i % 3].to_le_bytes();
        bytes[4 * i] = a;
        bytes[4 * i + 1] = b;
        bytes[4 * i + 2] = c;
        bytes[4 * i + 3] = d;
        i += 1;
    }
    bytes
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mesh_has_three_cubic_patches_and_two_hinge_closures() {
        let [top_first, top_last, top_close, bottom, bottom_close] =
            CONTROL_POINTS.as_chunks::<4>().0
        else {
            panic!("expected five wing control patches");
        };
        assert_eq!(top_first[3], top_last[0]);
        for patch in [top_first, top_last, bottom] {
            assert!(patch.iter().all(|p| p[2] == 0.0));
        }
        for (close, start, end) in [
            (top_close, top_first[0], top_last[3]),
            (bottom_close, bottom[0], bottom[3]),
        ] {
            assert_eq!(close[0], [end[0], end[1], 1.0]);
            assert_eq!(close[1], close[0]);
            assert_eq!(close[2], [start[0], start[1], 1.0]);
            assert_eq!(close[3], close[2]);
            assert_eq!(start[0], 0.0);
            assert_eq!(end[0], 0.0);
        }
        for (i, position) in CONTROL_POINTS.iter().flatten().enumerate() {
            assert_eq!(&VERTEX_BYTES[i * 4..i * 4 + 4], &position.to_le_bytes());
        }
        assert!(
            INDEX_BYTES
                .chunks_exact(4)
                .enumerate()
                .all(|(i, bytes)| { u32::from_le_bytes(bytes.try_into().unwrap()) == i as u32 })
        );
    }
}
