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
    parts: Vec<u8>,
    indices: Vec<u8>,
    radius_squared: f32,
    outline_boxes: Vec<[[f32; 3]; 2]>,
    key_tops: Vec<(usize, [f32; 3], [f32; 3], [f32; 4])>,
}

fn main() {
    let source =
        PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap()).join("Keyboard k70 mini pro.glb");
    println!("cargo:rerun-if-changed={}", source.display());
    let bytes = fs::read(&source).expect("read Keyboard k70 mini pro.glb");
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
    assert_eq!(
        parsed.meshes().count(),
        63,
        "expected K70 chassis and key meshes"
    );
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
    prepare_legends(&source, &bytes, &geometry, &out);
    fs::write(
        out.join("keyboard.outlines.rs"),
        format!(
            "pub const KEYBOARD_BOXES: &[[[f32; 3]; 2]] = &{:?};\n",
            geometry.outline_boxes
        ),
    )
    .expect("write lightweight intro outlines");
    fs::write(out.join("keyboard.vertices.bin"), geometry.vertices)
        .expect("write keyboard vertices");
    fs::write(out.join("keyboard.indices.bin"), geometry.indices).expect("write keyboard indices");
    fs::write(out.join("keyboard.parts.bin"), geometry.parts).expect("write keyboard parts");
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
        let first_vertex = geometry.vertices.len() / 24;
        let first_index = geometry.indices.len() / 4;
        // The K70 export has 62 key meshes and one chassis (mesh 29).
        let part = u8::from(mesh.index() == 29);
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
                geometry.parts.push(part);
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
        // Deliberately simple intro wire boxes, prepared once per model part.
        // No triangle wireframe, extra PBR geometry, or runtime GLB traversal.
        let mut outline_min = [f32::INFINITY; 3];
        let mut outline_max = [f32::NEG_INFINITY; 3];
        for vertex in geometry.vertices[first_vertex * 24..].chunks_exact(24) {
            for axis in 0..3 {
                let value = f32::from_le_bytes(vertex[axis * 4..axis * 4 + 4].try_into().unwrap());
                outline_min[axis] = outline_min[axis].min(value);
                outline_max[axis] = outline_max[axis].max(value);
            }
        }
        geometry.outline_boxes.push([outline_min, outline_max]);
        if mesh.index() != 29 {
            // Intersect the vertical ray at the key's XZ centre with its actual
            // top triangles. The label plane follows this key's sculpt/tilt.
            let value = |i: usize, axis: usize| {
                f32::from_le_bytes(
                    geometry.vertices[i * 24 + axis * 4..i * 24 + axis * 4 + 4]
                        .try_into()
                        .unwrap(),
                )
            };
            let mut min = [f32::INFINITY; 3];
            let mut max = [f32::NEG_INFINITY; 3];
            for i in first_vertex..geometry.vertices.len() / 24 {
                for axis in 0..3 {
                    min[axis] = min[axis].min(value(i, axis));
                    max[axis] = max[axis].max(value(i, axis));
                }
            }
            let x = (min[0] + max[0]) * 0.5;
            let z = (min[2] + max[2]) * 0.5;
            let mut top = None;
            for triangle in geometry.indices[first_index * 4..].chunks_exact(12) {
                let p: [[f32; 3]; 3] = std::array::from_fn(|corner| {
                    let i = u32::from_le_bytes(
                        triangle[corner * 4..corner * 4 + 4].try_into().unwrap(),
                    ) as usize;
                    std::array::from_fn(|axis| value(i, axis))
                });
                let a: [f32; 3] = std::array::from_fn(|i| p[1][i] - p[0][i]);
                let b: [f32; 3] = std::array::from_fn(|i| p[2][i] - p[0][i]);
                let normal = cross(a, b);
                if normal[1] <= 0.0001 {
                    continue;
                }
                let det = a[0] * b[2] - a[2] * b[0];
                if det.abs() < 1e-6 {
                    continue;
                }
                let u = ((x - p[0][0]) * b[2] - (z - p[0][2]) * b[0]) / det;
                let v = (a[0] * (z - p[0][2]) - a[2] * (x - p[0][0])) / det;
                if u < -1e-4 || v < -1e-4 || u + v > 1.0001 {
                    continue;
                }
                let y = p[0][1] + u * a[1] + v * b[1];
                if top
                    .as_ref()
                    .is_none_or(|(point, _): &([f32; 3], [f32; 3])| y > point[1])
                {
                    let length = dot(normal, normal).sqrt();
                    top = Some(([x, y + 0.025, z], normal.map(|n| n / length)));
                }
            }
            let (center, normal) = top.expect("key top at its centre");
            // Use the upward-facing top rather than the wider beveled base.
            // Sculpted tops contain several slightly different face planes.
            // For ISO Enter, these bounds retain the full upper section width.
            let mut bounds = [
                f32::INFINITY,
                f32::NEG_INFINITY,
                f32::INFINITY,
                f32::NEG_INFINITY,
            ];
            for triangle in geometry.indices[first_index * 4..].chunks_exact(12) {
                let points: [[f32; 3]; 3] = std::array::from_fn(|corner| {
                    let i = u32::from_le_bytes(
                        triangle[corner * 4..corner * 4 + 4].try_into().unwrap(),
                    ) as usize;
                    std::array::from_fn(|axis| value(i, axis))
                });
                let a = std::array::from_fn(|axis| points[1][axis] - points[0][axis]);
                let b = std::array::from_fn(|axis| points[2][axis] - points[0][axis]);
                let face = cross(a, b);
                if dot(normal, face) > dot(face, face).sqrt() * 0.9 {
                    for point in points {
                        bounds[0] = bounds[0].min(point[0]);
                        bounds[1] = bounds[1].max(point[0]);
                        bounds[2] = bounds[2].min(point[2]);
                        bounds[3] = bounds[3].max(point[2]);
                    }
                }
            }
            assert!(bounds.iter().all(|v| v.is_finite()), "flat key top bounds");
            geometry
                .key_tops
                .push((mesh.index(), center, normal, bounds));
        }
    }
    for child in node.children() {
        append_node(child, world, bin, geometry);
    }
}

fn prepare_legends(
    source: &std::path::Path,
    bytes: &[u8],
    geometry: &Geometry,
    out: &std::path::Path,
) {
    use sha2::{Digest, Sha256};
    let path = source.parent().unwrap().join("keyfarm/mapping.json");
    println!("cargo:rerun-if-changed={}", path.display());
    let map: serde_json::Value =
        serde_json::from_slice(&fs::read(path).expect("read audited key map")).unwrap();
    assert_eq!(
        map["model_sha256"].as_str().unwrap(),
        format!("{:x}", Sha256::digest(bytes))
    );
    let mut legends = String::from("pub const LEGENDS: &[Legend] = &[\n");
    let mut keys = String::from("pub const KEYS: &[Key] = &[\n");
    let mut words = String::from("pub const WORD_LEGENDS: &[WordLegend] = &[\n");
    let mut word_count = 0;
    let mut usages = std::collections::BTreeSet::new();
    let mut scalars = std::collections::BTreeSet::new();
    for key in map["keys"].as_array().unwrap() {
        let mesh = key["mesh"].as_u64().unwrap() as usize;
        let (_, center, normal, bounds) = geometry
            .key_tops
            .iter()
            .find(|(id, _, _, _)| *id == mesh)
            .unwrap();
        let usage = key["press"]["scan"].as_u64().map(|scan| {
            assert_eq!(
                scan >> 16,
                7,
                "recorded key must belong to USB HID keyboard page"
            );
            let usage = u8::try_from(scan & 0xffff).expect("8-bit keyboard usage");
            assert!(usages.insert(usage), "duplicate recorded HID usage");
            usage
        });
        keys.push_str(&format!(
            "Key {{ usage: {usage:?}, mesh: {mesh}, center: {center:?}, normal: {normal:?} }},\n"
        ));
        let label = key["press"]["label"].as_str().unwrap();
        let word = match usage {
            Some(0x2b) => Some("TAB"),
            Some(0x39) => Some("CAPS"),
            Some(0xe1 | 0xe5) => Some("SHIFT"),
            Some(0xe0 | 0xe4) => Some("STRG"),
            Some(0xe2) => Some("ALT"),
            Some(0xe6) => Some("ALTGR"),
            Some(0x28) => Some("ENTER"),
            None if label == "Fn" => Some("FN"),
            _ => None,
        };
        if let Some(word) = word {
            let mut anchor = *center;
            anchor[0] = (bounds[0] + bounds[1]) * 0.5;
            if word == "ENTER" {
                // Keep the legend on the full-width upper row of the L shape.
                anchor[2] = geometry
                    .key_tops
                    .iter()
                    .find(|(id, _, _, _)| *id == 17)
                    .unwrap()
                    .1[2];
            }
            anchor[1] -= (normal[0] * (anchor[0] - center[0])
                + normal[2] * (anchor[2] - center[2]))
                / normal[1];
            let width = bounds[1] - bounds[0] - 0.2;
            assert!(width > 0.);
            words.push_str(&format!("WordLegend {{ text: {word:?}, mesh: {mesh}, center: {anchor:?}, normal: {normal:?}, width: {width:?} }},\n"));
            word_count += 1;
        }
        let mut chars = label.chars();
        let Some(ch) = chars.next() else {
            continue;
        };
        if chars.next().is_some() || !ch.is_ascii_alphanumeric() {
            continue;
        }
        let ch = ch.to_ascii_uppercase();
        assert!(scalars.insert(ch), "duplicate alphanumeric key");
        let mesh = key["mesh"].as_u64().unwrap() as usize;
        let (_, center, normal, _) = geometry
            .key_tops
            .iter()
            .find(|(id, _, _, _)| *id == mesh)
            .unwrap();
        assert!(normal[1] > 0.8);
        legends.push_str(&format!(
            "Legend {{ scalar: {ch:?}, mesh: {mesh}, center: {center:?}, normal: {normal:?} }},\n"
        ));
    }
    assert_eq!(scalars.len(), 36, "exactly A-Z and 0-9");
    legends.push_str("];\n");
    fs::write(out.join("keyboard.legends.rs"), legends).unwrap();
    assert_eq!(usages.len(), 61, "61 input keys plus hardware-local Fn");
    keys.push_str("];\n");
    fs::write(out.join("keyboard.keys.rs"), keys).unwrap();
    assert_eq!(
        word_count, 10,
        "requested control-key legends including both Shift/Ctrl keys"
    );
    words.push_str("];\n");
    fs::write(out.join("keyboard.words.rs"), words).unwrap();
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
