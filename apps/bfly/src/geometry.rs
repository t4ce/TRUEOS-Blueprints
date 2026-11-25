//! Control points for both outlines in the wing SVG.

type Point = [f64; 2];

// Every cubic contributes two control points and an endpoint. The lower
// curve starts outboard, and the SVG supplies the shared hinge line.
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
    [0.3423464298248291, 0.4808717370033264],
    [0.7294665575027466, 0.5889291763305664],
    [0.2280963510274887, 1.109559416770935],
    [0.014813999645411968, 0.4659808874130249],
];
const ROOT_CONNECTOR: [Point; 2] = [
    [0.014814138412475586, 0.43834799528121948],
    [0.014813999645411968, 0.4659808874130249],
];
const ALTERNATE_TOP_WING: [Point; 10] = [
    [1.6468015713617206, 0.4421769976615906],
    [1.8329522758722305, 0.2668195366859436],
    [2.037594050168991, 0.21073514223098755],
    [2.1032925844192505, 0.1899208128452301],
    [2.339706063270569, 0.11502130329608917],
    [2.31382292509079, 0.21733516454696655],
    [2.269503355026245, 0.31247615814208984],
    [2.149497628211975, 0.5700929164886475],
    [2.146865963935852, 0.5294228792190552],
    [2.005320966243744, 0.5341325998306274],
];
const ALTERNATE_BOTTOM_WING: [Point; 7] = [
    [1.6468015713617206, 0.4506330192089081],
    [2.3072643280029297, 0.5287371873855591],
    [2.092701017856598, 0.7310714721679688],
    [2.0366891622543335, 0.7801885604858398],
    [1.907318651676178, 0.8936342597007751],
    [1.7806438952684402, 0.9134736061096191],
    [1.6468015713617206, 0.4667331874370575],
];
const ALTERNATE_ROOT_CONNECTOR: [Point; 2] = [
    [1.6468015713617206, 0.44217699766159058],
    [1.6468015713617206, 0.4667331874370575],
];
const FILL_WING: [Point; 10] = [
    [0.014814138412475586, 0.4383479952812195],
    [0.34511280059814453, 0.019663125276565552],
    [0.7678900957107544, 0.07059453427791595],
    [0.6598089933395386, 0.26178503036499023],
    [0.540329001124129, 0.4731396368969378],
    [0.4631952587281967, 0.495209978976582],
    [0.34304285283852043, 0.48106684481017825],
    [0.7288645270049599, 0.5895540217797547],
    [0.22796827736612274, 1.109172955077798],
    [0.014813999645411968, 0.4659808874130249],
];
const ALTERNATE_FILL_WING: [Point; 16] = [
    [1.6468015713617206, 0.4421769976615906],
    [1.8329522758722305, 0.2668195366859436],
    [2.037594050168991, 0.21073514223098755],
    [2.1032925844192505, 0.1899208128452301],
    [2.339706063270569, 0.11502130329608917],
    [2.31382292509079, 0.21733516454696655],
    [2.269503355026245, 0.31247615814208984],
    [2.150107689919802, 0.5687832946586415],
    [2.1468957539214952, 0.5298286713878677],
    [2.0074689066416354, 0.5340642458566105],
    [2.2043758199961605, 0.6254998372003495],
    [2.0778084769711187, 0.7441308189452613],
    [2.0366891622543335, 0.7801885604858398],
    [1.907318651676178, 0.8936342597007751],
    [1.7806438952684402, 0.9134736061096191],
    [1.6468015713617206, 0.4667331874370575],
];

const ROOT_Y: f64 = 0.45;
const OUTLINES: [([&[Point]; 2], [Point; 2]); 2] = [
    ([&TOP_WING, &BOTTOM_WING], ROOT_CONNECTOR),
    (
        [&ALTERNATE_TOP_WING, &ALTERNATE_BOTTOM_WING],
        ALTERNATE_ROOT_CONNECTOR,
    ),
];
const FILL_OUTLINES: [&[Point]; 2] = [&FILL_WING, &ALTERNATE_FILL_WING];
pub const PATCH_COUNT: usize = 10;
pub const VERTEX_COUNT: usize = 4 * PATCH_COUNT;
pub const FILL_PATCH_COUNT: usize = 10;
pub const FILL_VERTEX_COUNT: usize = 4 * FILL_PATCH_COUNT;
pub const LAYERED_VERTEX_COUNT: usize = FILL_VERTEX_COUNT + VERTEX_COUNT;
const STRIDE: usize = 3 * size_of::<f32>();

const CONTROL_POINTS: [[f32; 3]; VERTEX_COUNT] = control_points();
pub const VERTEX_BYTES: [u8; VERTEX_COUNT * STRIDE] = vertex_bytes(&CONTROL_POINTS);
pub const INDEX_BYTES: [u8; VERTEX_COUNT * size_of::<u32>()] = index_bytes();
const FILL_CONTROL_POINTS: [[f32; 3]; FILL_VERTEX_COUNT] = fill_control_points();
pub const FILL_VERTEX_BYTES: [u8; FILL_VERTEX_COUNT * STRIDE] = vertex_bytes(&FILL_CONTROL_POINTS);
pub const FILL_INDEX_BYTES: [u8; FILL_VERTEX_COUNT * size_of::<u32>()] = index_bytes();
pub const LAYERED_VERTEX_BYTES: [u8; LAYERED_VERTEX_COUNT * STRIDE] =
    join_bytes(&FILL_VERTEX_BYTES, &VERTEX_BYTES);
// The two draw views each index their own vertex window starting from zero.
pub const LAYERED_INDEX_BYTES: [u8; LAYERED_VERTEX_COUNT * size_of::<u32>()] =
    join_bytes(&FILL_INDEX_BYTES, &INDEX_BYTES);

const fn control_points() -> [[f32; 3]; VERTEX_COUNT] {
    let mut controls = [[0.0; 3]; VERTEX_COUNT];
    let mut artwork = 0;
    let mut next = 0;
    while artwork < OUTLINES.len() {
        let (paths, [start, end]) = OUTLINES[artwork];
        let root_x = start[0];
        // The low bit identifies the connector. The remaining bits identify the outline.
        let tag = (artwork * 2) as f32;
        let mut wing = 0;
        while wing < paths.len() {
            next = append_cubic_path(&mut controls, next, paths[wing], root_x, tag);
            wing += 1;
        }
        next = append_straight(&mut controls, next, start, end, root_x, tag + 1.0);
        artwork += 1;
    }
    assert!(next == VERTEX_COUNT);
    controls
}

const fn fill_control_points() -> [[f32; 3]; FILL_VERTEX_COUNT] {
    let mut controls = [[0.0; 3]; FILL_VERTEX_COUNT];
    let mut artwork = 0;
    let mut next = 0;
    while artwork < FILL_OUTLINES.len() {
        let path = FILL_OUTLINES[artwork];
        let first = path[0];
        let tag = (artwork * 4) as f32;
        next = append_cubic_path(&mut controls, next, path, first[0], tag);
        next = append_straight(
            &mut controls,
            next,
            path[path.len() - 1],
            first,
            first[0],
            tag + 1.0,
        );
        artwork += 1;
    }
    assert!(next == FILL_VERTEX_COUNT);
    controls
}

const fn append_cubic_path(
    controls: &mut [[f32; 3]],
    mut next: usize,
    path: &[Point],
    root_x: f64,
    tag: f32,
) -> usize {
    let mut curve = 0;
    while curve < path.len() - 1 {
        let mut point = 0;
        while point < 4 {
            controls[next] = control(path[curve + point], root_x, tag);
            next += 1;
            point += 1;
        }
        curve += 3;
    }
    next
}

const fn append_straight(
    controls: &mut [[f32; 3]],
    next: usize,
    start: Point,
    end: Point,
    root_x: f64,
    tag: f32,
) -> usize {
    controls[next] = control(start, root_x, tag);
    controls[next + 1] = controls[next];
    controls[next + 2] = control(end, root_x, tag);
    controls[next + 3] = controls[next + 2];
    next + 4
}

const fn control([x, y]: Point, root_x: f64, tag: f32) -> [f32; 3] {
    [(x - root_x) as f32, (ROOT_Y - y) as f32, tag]
}

const fn vertex_bytes<const N: usize>(controls: &[[f32; 3]]) -> [u8; N] {
    assert!(N == controls.len() * STRIDE);
    let mut bytes = [0; N];
    let mut i = 0;
    while i < controls.len() * 3 {
        let [a, b, c, d] = controls[i / 3][i % 3].to_le_bytes();
        bytes[4 * i] = a;
        bytes[4 * i + 1] = b;
        bytes[4 * i + 2] = c;
        bytes[4 * i + 3] = d;
        i += 1;
    }
    bytes
}

const fn index_bytes<const N: usize>() -> [u8; N] {
    let mut bytes = [0; N];
    let mut i = 0;
    while i < N / size_of::<u32>() {
        let [a, b, c, d] = (i as u32).to_le_bytes();
        bytes[4 * i] = a;
        bytes[4 * i + 1] = b;
        bytes[4 * i + 2] = c;
        bytes[4 * i + 3] = d;
        i += 1;
    }
    bytes
}

const fn join_bytes<const N: usize>(first: &[u8], second: &[u8]) -> [u8; N] {
    let mut bytes = [0; N];
    let (start, end) = bytes.split_at_mut(first.len());
    start.copy_from_slice(first);
    end.copy_from_slice(second);
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mesh_preserves_both_authored_outlines_and_connectors() {
        let (patches, remainder) = CONTROL_POINTS.as_chunks::<4>();
        assert!(remainder.is_empty());
        assert_eq!(patches.len(), 10);
        for (artwork, group) in [&patches[..4], &patches[4..]].into_iter().enumerate() {
            let (paths, [start, end]) = OUTLINES[artwork];
            let root_x = start[0];
            let tag = (artwork * 2) as f32;
            let (connector, curves) = group.split_last().unwrap();
            let [start, end] = [
                control(start, root_x, tag + 1.0),
                control(end, root_x, tag + 1.0),
            ];
            assert_eq!(*connector, [start, start, end, end]);
            let expected = paths.into_iter().flat_map(|path| {
                (0..path.len() - 1).step_by(3).map(move |index| {
                    core::array::from_fn(|point| control(path[index + point], root_x, tag))
                })
            });
            assert!(curves.iter().copied().eq(expected));
        }
        assert!(patches[2][0][0] > 0.3);
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

    #[test]
    fn fill_mesh_closes_each_wing_at_its_authored_endpoints() {
        let (patches, remainder) = FILL_CONTROL_POINTS.as_chunks::<4>();
        assert!(remainder.is_empty());
        assert_eq!(patches.len(), FILL_PATCH_COUNT);
        let mut next = 0;
        for (artwork, path) in FILL_OUTLINES.into_iter().enumerate() {
            let root_x = path[0][0];
            let tag = (artwork * 4) as f32;
            for start in (0..path.len() - 1).step_by(3) {
                assert_eq!(
                    patches[next],
                    core::array::from_fn(|i| control(path[start + i], root_x, tag)),
                );
                next += 1;
            }
            let last = control(path[path.len() - 1], root_x, tag + 1.0);
            let first = control(path[0], root_x, tag + 1.0);
            assert_eq!(patches[next], [last, last, first, first]);
            next += 1;
        }
        assert_eq!(next, FILL_PATCH_COUNT);
        for (i, position) in FILL_CONTROL_POINTS.iter().flatten().enumerate() {
            assert_eq!(
                &FILL_VERTEX_BYTES[i * 4..i * 4 + 4],
                &position.to_le_bytes()
            );
        }
        assert!(
            FILL_INDEX_BYTES
                .chunks_exact(4)
                .enumerate()
                .all(|(i, bytes)| { u32::from_le_bytes(bytes.try_into().unwrap()) == i as u32 })
        );
    }

    #[test]
    fn layered_mesh_preserves_both_locally_indexed_views() {
        let (fill, outline) = LAYERED_VERTEX_BYTES.split_at(FILL_VERTEX_BYTES.len());
        assert_eq!(fill, FILL_VERTEX_BYTES);
        assert_eq!(outline, VERTEX_BYTES);
        let (fill, outline) = LAYERED_INDEX_BYTES.split_at(FILL_INDEX_BYTES.len());
        assert_eq!(fill, FILL_INDEX_BYTES);
        assert_eq!(outline, INDEX_BYTES);
    }
}
