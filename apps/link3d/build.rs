// Reuses TRUEOS-Picasso-Example's build.rs `prepare` image-free glTF reader
// path. Bake the active scene's node transforms; keep authored units/origin.
use std::{env, fs, path::PathBuf};

type Matrix = [[f32; 4]; 4];
const IDENTITY: Matrix = [
    [1.0, 0.0, 0.0, 0.0],
    [0.0, 1.0, 0.0, 0.0],
    [0.0, 0.0, 1.0, 0.0],
    [0.0, 0.0, 0.0, 1.0],
];

#[derive(Default)]
struct Geometry {
    vertices: Vec<u8>,
    indices: Vec<u8>,
    radius_squared: f32,
}

fn main() {
    let source = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap()).join("Keyboard OP.glb");
    println!("cargo:rerun-if-changed={}", source.display());
    let bytes = fs::read(&source).expect("read Keyboard OP.glb");
    let parsed = gltf::Gltf::from_slice(&bytes).expect("validate keyboard glTF");
    assert!(
        parsed.materials().next().is_none(),
        "keyboard baseline requires image-free default material"
    );
    assert!(
        parsed.skins().next().is_none(),
        "skinned geometry is unsupported"
    );
    assert!(
        parsed.animations().next().is_none(),
        "animated geometry is unsupported"
    );
    assert_eq!(
        parsed.buffers().count(),
        1,
        "keyboard must use one embedded BIN buffer"
    );
    assert!(matches!(
        parsed.buffers().next().unwrap().source(),
        gltf::buffer::Source::Bin
    ));
    let bin = parsed.blob.as_deref().expect("keyboard GLB BIN chunk");
    let scene = parsed
        .default_scene()
        .expect("keyboard requires an active default scene");
    let mut geometry = Geometry::default();
    for node in scene.nodes() {
        append_node(node, IDENTITY, bin, &mut geometry);
    }
    assert!(
        !geometry.vertices.is_empty() && !geometry.indices.is_empty(),
        "empty keyboard scene"
    );
    let vertex_count = geometry.vertices.len() / 24;
    let index_count = geometry.indices.len() / 4;
    let radius = geometry.radius_squared.sqrt();
    assert!(
        radius.is_finite() && radius > 0.0,
        "invalid keyboard bounds"
    );
    let out = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    fs::write(out.join("keyboard.vertices.bin"), geometry.vertices)
        .expect("write keyboard vertices");
    fs::write(out.join("keyboard.indices.bin"), geometry.indices).expect("write keyboard indices");
    fs::write(out.join("prepared_keyboard.rs"), format!(
        "pub const VERTEX_COUNT: u32 = {vertex_count};\npub const INDEX_COUNT: u32 = {index_count};\npub const RADIUS: f32 = {radius:?};\n",
    )).expect("write keyboard metadata");
}

fn append_node(node: gltf::Node<'_>, parent: Matrix, bin: &[u8], geometry: &mut Geometry) {
    assert!(
        node.skin().is_none() && node.weights().is_none(),
        "skin/morph nodes are unsupported"
    );
    let world = multiply(parent, node.transform().matrix());
    assert!(
        world.iter().flatten().all(|value| value.is_finite()),
        "non-finite node transform"
    );
    if let Some(mesh) = node.mesh() {
        assert!(mesh.weights().is_none(), "morph meshes are unsupported");
        let [a, b, c] =
            [world[0], world[1], world[2]].map(|column| [column[0], column[1], column[2]]);
        let cofactors = [cross(b, c), cross(c, a), cross(a, b)];
        let determinant = dot(a, cofactors[0]);
        assert!(
            determinant.is_finite() && determinant > 0.0,
            "singular/mirrored node transforms are unsupported"
        );
        for primitive in mesh.primitives() {
            assert_eq!(
                primitive.mode(),
                gltf::mesh::Mode::Triangles,
                "keyboard requires triangle lists"
            );
            assert!(
                primitive.material().index().is_none(),
                "authored materials are unsupported"
            );
            assert!(
                primitive.morph_targets().next().is_none(),
                "morph primitives are unsupported"
            );
            let reader = primitive.reader(|buffer| (buffer.index() == 0).then_some(bin));
            let positions: Vec<_> = reader
                .read_positions()
                .expect("keyboard POSITION")
                .collect();
            let normals: Vec<_> = reader.read_normals().expect("keyboard NORMAL").collect();
            assert!(
                !positions.is_empty() && positions.len() == normals.len(),
                "invalid vertex attribute counts"
            );
            let base =
                u32::try_from(geometry.vertices.len() / 24).expect("vertex count exceeds u32");
            let count = u32::try_from(positions.len()).expect("primitive vertex count exceeds u32");
            for (position, normal) in positions.into_iter().zip(normals) {
                let position: [f32; 3] = std::array::from_fn(|row| {
                    world[3][row]
                        + (0..3)
                            .map(|column| world[column][row] * position[column])
                            .sum::<f32>()
                });
                // Cofactor columns / determinant are the inverse transpose.
                let normal: [f32; 3] = std::array::from_fn(|row| {
                    (0..3)
                        .map(|column| cofactors[column][row] * normal[column])
                        .sum::<f32>()
                        / determinant
                });
                let normal_length = dot(normal, normal).sqrt();
                assert!(
                    position.into_iter().all(f32::is_finite),
                    "non-finite vertex position"
                );
                assert!(
                    normal_length.is_finite() && normal_length > 0.0,
                    "invalid vertex normal"
                );
                let normal = normal.map(|component| component / normal_length);
                geometry.radius_squared = geometry.radius_squared.max(dot(position, position));
                for component in position.into_iter().chain(normal) {
                    geometry.vertices.extend(component.to_le_bytes());
                }
            }
            let indices: Vec<_> = reader
                .read_indices()
                .map(|values| values.into_u32().collect())
                .unwrap_or_else(|| (0..count).collect());
            assert!(
                !indices.is_empty() && indices.len().is_multiple_of(3),
                "invalid triangle index count"
            );
            // The retained material renderer handles its native viewport and
            // front-face convention. Keep the original glTF triangle order.
            for index in indices {
                assert!(index < count, "primitive index exceeds vertex count");
                geometry.indices.extend(
                    base.checked_add(index)
                        .expect("index exceeds u32")
                        .to_le_bytes(),
                );
            }
        }
    }
    for child in node.children() {
        append_node(child, world, bin, geometry);
    }
}

fn multiply(left: Matrix, right: Matrix) -> Matrix {
    std::array::from_fn(|column| {
        std::array::from_fn(|row| {
            (0..4)
                .map(|axis| left[axis][row] * right[column][axis])
                .sum()
        })
    })
}

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a.into_iter().zip(b).map(|(left, right)| left * right).sum()
}
