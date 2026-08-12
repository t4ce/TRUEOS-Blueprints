//! Small deterministic voxel source used to bring up Helio's real mesh path.
//!
//! The implementation is original HelioV code. It deliberately does not copy
//! Stratum's chunk streamer, ECS, integration layer, or mesh builder.

use helio::{MeshUpload, PackedVertex};

const SIDE: usize = 8;
const HEIGHT: usize = 5;

pub struct ChunkBuild {
    pub mesh: MeshUpload,
    pub solid_voxels: usize,
}

/// Build one texture-free terrain chunk in Helio's canonical upload format.
///
/// Only faces adjacent to air are emitted. [`PackedVertex`] does not contain a
/// colour channel; the first rendered milestone will assign colour through a
/// normal Helio material. Texture assets intentionally remain a later,
/// separately measurable capability.
pub fn build_voxel_chunk() -> ChunkBuild {
    let mut solids = [[[false; SIDE]; HEIGHT]; SIDE];
    let mut solid_voxels = 0usize;
    for x in 0..SIDE {
        for z in 0..SIDE {
            let top = 1 + ((x * 17 + z * 11 + (x ^ z) * 3) % (HEIGHT - 1));
            for y in 0..=top {
                solids[x][y][z] = true;
                solid_voxels += 1;
            }
        }
    }

    let mut vertices = Vec::new();
    let mut indices = Vec::new();
    for x in 0..SIDE {
        for y in 0..HEIGHT {
            for z in 0..SIDE {
                if !solids[x][y][z] {
                    continue;
                }
                for face in FACES {
                    let nx = x as i32 + face.neighbour[0];
                    let ny = y as i32 + face.neighbour[1];
                    let nz = z as i32 + face.neighbour[2];
                    let hidden = nx >= 0
                        && nx < SIDE as i32
                        && ny >= 0
                        && ny < HEIGHT as i32
                        && nz >= 0
                        && nz < SIDE as i32
                        && solids[nx as usize][ny as usize][nz as usize];
                    if hidden {
                        continue;
                    }
                    emit_face(
                        &mut vertices,
                        &mut indices,
                        [x as f32, y as f32, z as f32],
                        face,
                    );
                }
            }
        }
    }

    ChunkBuild {
        mesh: MeshUpload { vertices, indices },
        solid_voxels,
    }
}

pub fn mesh_fingerprint(mesh: &MeshUpload) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for vertex in &mesh.vertices {
        for byte in vertex
            .position
            .iter()
            .chain(core::iter::once(&vertex.bitangent_sign))
            .chain(vertex.tex_coords0.iter())
            .chain(vertex.tex_coords1.iter())
            .flat_map(|value| value.to_bits().to_le_bytes())
            .chain(vertex.normal.to_le_bytes())
            .chain(vertex.tangent.to_le_bytes())
        {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    for index in &mesh.indices {
        for byte in index.to_le_bytes() {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    hash
}

/// Project Helio-authored world positions for the first authenticated
/// position3 shader package. Camera ownership stays in the Blueprint and the
/// live UI4 aspect is the only platform input. A later camera-uniform package
/// moves this multiplication into the vertex shader without changing mesh or
/// draw ownership.
pub fn project_clip_positions(positions: &[[f32; 3]], aspect: f32) -> Vec<[f32; 3]> {
    let eye = [13.0, 10.0, 15.0];
    let target = [3.5, 1.7, 3.5];
    let forward = normalize(sub(target, eye));
    let right = normalize(cross(forward, [0.0, 1.0, 0.0]));
    let up = cross(right, forward);
    let tan_half_fov = (46.0_f32.to_radians() * 0.5).tan();
    let aspect = aspect.max(0.01);
    positions
        .iter()
        .map(|position| {
            let relative = sub(*position, eye);
            let depth = dot(relative, forward).max(0.1);
            [
                dot(relative, right) / (depth * tan_half_fov * aspect),
                dot(relative, up) / (depth * tan_half_fov),
                ((depth - 0.1) / (80.0 - 0.1)).clamp(0.0, 1.0),
            ]
        })
        .collect()
}

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
fn normalize(value: [f32; 3]) -> [f32; 3] {
    let length = dot(value, value).sqrt().max(f32::EPSILON);
    [value[0] / length, value[1] / length, value[2] / length]
}

#[derive(Clone, Copy)]
struct Face {
    neighbour: [i32; 3],
    normal: [f32; 3],
    corners: [[f32; 3]; 4],
}

const FACES: [Face; 6] = [
    Face {
        neighbour: [1, 0, 0],
        normal: [1.0, 0.0, 0.0],
        corners: [
            [1.0, 0.0, 0.0],
            [1.0, 1.0, 0.0],
            [1.0, 1.0, 1.0],
            [1.0, 0.0, 1.0],
        ],
    },
    Face {
        neighbour: [-1, 0, 0],
        normal: [-1.0, 0.0, 0.0],
        corners: [
            [0.0, 0.0, 1.0],
            [0.0, 1.0, 1.0],
            [0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0],
        ],
    },
    Face {
        neighbour: [0, 1, 0],
        normal: [0.0, 1.0, 0.0],
        corners: [
            [0.0, 1.0, 0.0],
            [0.0, 1.0, 1.0],
            [1.0, 1.0, 1.0],
            [1.0, 1.0, 0.0],
        ],
    },
    Face {
        neighbour: [0, -1, 0],
        normal: [0.0, -1.0, 0.0],
        corners: [
            [0.0, 0.0, 1.0],
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [1.0, 0.0, 1.0],
        ],
    },
    Face {
        neighbour: [0, 0, 1],
        normal: [0.0, 0.0, 1.0],
        corners: [
            [1.0, 0.0, 1.0],
            [1.0, 1.0, 1.0],
            [0.0, 1.0, 1.0],
            [0.0, 0.0, 1.0],
        ],
    },
    Face {
        neighbour: [0, 0, -1],
        normal: [0.0, 0.0, -1.0],
        corners: [
            [0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [1.0, 1.0, 0.0],
            [1.0, 0.0, 0.0],
        ],
    },
];

fn emit_face(
    vertices: &mut Vec<PackedVertex>,
    indices: &mut Vec<u32>,
    origin: [f32; 3],
    face: Face,
) {
    let base = vertices.len() as u32;
    for (corner_index, corner) in face.corners.into_iter().enumerate() {
        let uv = match corner_index {
            0 => [0.0, 0.0],
            1 => [0.0, 1.0],
            2 => [1.0, 1.0],
            _ => [1.0, 0.0],
        };
        vertices.push(PackedVertex::from_components(
            [
                origin[0] + corner[0],
                origin[1] + corner[1],
                origin[2] + corner[2],
            ],
            face.normal,
            uv,
            [1.0, 0.0, 0.0],
            1.0,
        ));
    }
    indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chunk_is_face_culled_and_deterministic() {
        let first = build_voxel_chunk();
        let second = build_voxel_chunk();
        assert!(first.solid_voxels > 0);
        assert_eq!(first.mesh.indices.len() % 6, 0);
        assert!(first.mesh.vertices.len() < first.solid_voxels * 24);
        assert_eq!(
            mesh_fingerprint(&first.mesh),
            mesh_fingerprint(&second.mesh)
        );
    }

    #[test]
    fn projection_is_finite_and_tracks_live_aspect() {
        let positions = [[0.0, 0.0, 0.0], [7.0, 4.0, 7.0]];
        let wide = project_clip_positions(&positions, 16.0 / 9.0);
        let square = project_clip_positions(&positions, 1.0);
        assert!(wide.iter().flatten().all(|component| component.is_finite()));
        assert_ne!(wide[0][0], square[0][0]);
        assert_eq!(wide[0][1], square[0][1]);
    }
}
