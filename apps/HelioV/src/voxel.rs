//! Deterministic, bounded voxel-world source for HelioV's real mesh path.
//!
//! The implementation is original HelioV code. It deliberately does not copy
//! Stratum's chunk streamer, ECS, integration layer, or mesh builder.

use helio::{FlyCamera, MeshUpload, PackedVertex, PerspectiveLens};

pub const CHUNK_SIDE: usize = 8;
pub const WORLD_CHUNKS: usize = 6;
pub const WORLD_SIDE: usize = CHUNK_SIDE * WORLD_CHUNKS;
pub const WORLD_HEIGHT: usize = 28;
const WATER_LEVEL: usize = 4;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WorldChunk {
    pub coord: [u32; 2],
    pub center: [f32; 3],
    pub radius: f32,
    pub solid_voxels: u32,
    pub first_index: u32,
    pub index_count: u32,
}

pub struct WorldBuild {
    pub mesh: MeshUpload,
    pub solid_voxels: usize,
    pub water_voxels: usize,
    pub landmark_voxels: usize,
    pub chunks: Vec<WorldChunk>,
}

/// One top-facing quad used only by the opt-in sampler bring-up package. It
/// keeps vertex fetch, interpolation, index fetch and UI4 presentation real
/// while removing 41k vertices and world visibility from the failure domain.
#[cfg(feature = "texture-bringup-fixed-load")]
pub fn texture_bringup_quad() -> MeshUpload {
    let mut vertices = Vec::with_capacity(4);
    let mut indices = Vec::with_capacity(6);
    emit_face(&mut vertices, &mut indices, [18.0, 7.0, 18.0], FACES[2]);
    for vertex in &mut vertices {
        vertex.position[0] = 18.0 + (vertex.position[0] - 18.0) * 12.0;
        vertex.position[2] = 18.0 + (vertex.position[2] - 18.0) * 12.0;
    }
    MeshUpload { vertices, indices }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Block {
    Air,
    Terrain,
    Water,
    Landmark,
}

impl Block {
    const fn is_solid(self) -> bool {
        !matches!(self, Self::Air)
    }
}

struct VoxelWorld {
    blocks: Vec<Block>,
    landmark_voxels: usize,
}

impl VoxelWorld {
    fn new() -> Self {
        Self {
            blocks: vec![Block::Air; WORLD_SIDE * WORLD_HEIGHT * WORLD_SIDE],
            landmark_voxels: 0,
        }
    }

    const fn index(x: usize, y: usize, z: usize) -> usize {
        (x * WORLD_HEIGHT + y) * WORLD_SIDE + z
    }

    fn get(&self, x: usize, y: usize, z: usize) -> Block {
        self.blocks[Self::index(x, y, z)]
    }

    fn get_checked(&self, x: i32, y: i32, z: i32) -> Block {
        if x < 0
            || x >= WORLD_SIDE as i32
            || y < 0
            || y >= WORLD_HEIGHT as i32
            || z < 0
            || z >= WORLD_SIDE as i32
        {
            Block::Air
        } else {
            self.get(x as usize, y as usize, z as usize)
        }
    }

    fn set(&mut self, x: usize, y: usize, z: usize, block: Block) {
        if x < WORLD_SIDE && y < WORLD_HEIGHT && z < WORLD_SIDE {
            self.blocks[Self::index(x, y, z)] = block;
        }
    }

    fn set_landmark(&mut self, x: i32, y: i32, z: i32) {
        if x < 0
            || x >= WORLD_SIDE as i32
            || y < 0
            || y >= WORLD_HEIGHT as i32
            || z < 0
            || z >= WORLD_SIDE as i32
        {
            return;
        }
        let index = Self::index(x as usize, y as usize, z as usize);
        if self.blocks[index] != Block::Landmark {
            self.landmark_voxels += 1;
            self.blocks[index] = Block::Landmark;
        }
    }

    fn terrain_top(&self, x: usize, z: usize) -> usize {
        (0..WORLD_HEIGHT)
            .rev()
            .find(|&y| self.get(x, y, z) == Block::Terrain)
            .unwrap_or(0)
    }
}

/// Build a texture-free 6x6 chunk world in Helio's canonical upload format.
///
/// Chunk metadata and index ranges remain explicit even while the current VMX
/// frontier submits one indexed draw. Faces are culled across chunk borders.
pub fn build_voxel_world() -> WorldBuild {
    let mut world = VoxelWorld::new();
    for x in 0..WORLD_SIDE {
        for z in 0..WORLD_SIDE {
            let top = terrain_height(x, z);
            for y in 0..=top {
                world.set(x, y, z, Block::Terrain);
            }
            if top < WATER_LEVEL {
                for y in top + 1..=WATER_LEVEL {
                    world.set(x, y, z, Block::Water);
                }
            }
        }
    }

    add_leaning_tower(&mut world, WORLD_SIDE as i32 / 2, WORLD_SIDE as i32 / 2);
    for (x, z) in [(9, 12), (36, 10), (10, 36), (37, 35)] {
        add_house(&mut world, x, z);
    }
    for (x, z) in [
        (6, 23),
        (12, 6),
        (16, 31),
        (20, 10),
        (29, 38),
        (34, 18),
        (41, 26),
        (43, 42),
    ] {
        add_tree(&mut world, x, z);
    }

    let mut vertices = Vec::new();
    let mut indices = Vec::new();
    let mut chunks = Vec::with_capacity(WORLD_CHUNKS * WORLD_CHUNKS);
    for chunk_x in 0..WORLD_CHUNKS {
        for chunk_z in 0..WORLD_CHUNKS {
            let first_index = indices.len() as u32;
            let mut chunk_solids = 0u32;
            for x in chunk_x * CHUNK_SIDE..(chunk_x + 1) * CHUNK_SIDE {
                for y in 0..WORLD_HEIGHT {
                    for z in chunk_z * CHUNK_SIDE..(chunk_z + 1) * CHUNK_SIDE {
                        if !world.get(x, y, z).is_solid() {
                            continue;
                        }
                        chunk_solids += 1;
                        for face in FACES {
                            if world
                                .get_checked(
                                    x as i32 + face.neighbour[0],
                                    y as i32 + face.neighbour[1],
                                    z as i32 + face.neighbour[2],
                                )
                                .is_solid()
                            {
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
            let index_count = indices.len() as u32 - first_index;
            chunks.push(WorldChunk {
                coord: [chunk_x as u32, chunk_z as u32],
                center: [
                    (chunk_x * CHUNK_SIDE) as f32 + CHUNK_SIDE as f32 * 0.5,
                    WORLD_HEIGHT as f32 * 0.5,
                    (chunk_z * CHUNK_SIDE) as f32 + CHUNK_SIDE as f32 * 0.5,
                ],
                radius: ((CHUNK_SIDE * CHUNK_SIDE * 2 + WORLD_HEIGHT * WORLD_HEIGHT) as f32).sqrt()
                    * 0.5,
                solid_voxels: chunk_solids,
                first_index,
                index_count,
            });
        }
    }

    let solid_voxels = world.blocks.iter().filter(|block| block.is_solid()).count();
    let water_voxels = world
        .blocks
        .iter()
        .filter(|block| **block == Block::Water)
        .count();
    WorldBuild {
        mesh: MeshUpload { vertices, indices },
        solid_voxels,
        water_voxels,
        landmark_voxels: world.landmark_voxels,
        chunks,
    }
}

fn terrain_height(x: usize, z: usize) -> usize {
    let broad = value_noise(x, z, 12, 0x51a7) as usize;
    let detail = value_noise(x, z, 5, 0xc319) as usize;
    let mut height = 3 + broad * 6 / 255 + detail * 3 / 255;
    let river_z = 8 + x * 5 / 8 + ((x / 7) & 1) * 2;
    let river_distance = z.abs_diff(river_z.min(WORLD_SIDE - 1));
    if river_distance <= 2 {
        height = height.min(2 + river_distance / 2);
    }
    height.min(WORLD_HEIGHT - 2)
}

fn value_noise(x: usize, z: usize, cell: usize, seed: u32) -> u32 {
    let x0 = x / cell;
    let z0 = z / cell;
    let tx = smooth_q8(((x % cell) as u32 * 256) / cell as u32);
    let tz = smooth_q8(((z % cell) as u32 * 256) / cell as u32);
    let a = lerp_q8(
        hash_byte(x0 as u32, z0 as u32, seed),
        hash_byte(x0 as u32 + 1, z0 as u32, seed),
        tx,
    );
    let b = lerp_q8(
        hash_byte(x0 as u32, z0 as u32 + 1, seed),
        hash_byte(x0 as u32 + 1, z0 as u32 + 1, seed),
        tx,
    );
    lerp_q8(a, b, tz)
}

fn smooth_q8(value: u32) -> u32 {
    let value = value.min(256);
    value * value * (768 - 2 * value) / (256 * 256)
}

fn lerp_q8(a: u32, b: u32, t: u32) -> u32 {
    (a * (256 - t) + b * t + 128) / 256
}

fn hash_byte(x: u32, z: u32, seed: u32) -> u32 {
    let mut value = x
        .wrapping_mul(0x9e37_79b9)
        .wrapping_add(z.wrapping_mul(0x85eb_ca6b))
        .wrapping_add(seed);
    value ^= value >> 16;
    value = value.wrapping_mul(0x7feb_352d);
    value ^= value >> 15;
    value = value.wrapping_mul(0x846c_a68b);
    (value ^ (value >> 16)) & 0xff
}

fn add_leaning_tower(world: &mut VoxelWorld, x: i32, z: i32) {
    let base = world.terrain_top(x as usize, z as usize) as i32 + 1;
    for level in 0..18 {
        let centre_x = x + level / 5;
        for dx in -3i32..=3 {
            for dz in -3i32..=3 {
                let wall = dx.abs() == 3 || dz.abs() == 3;
                let doorway = level < 3 && dz == 3 && dx.abs() <= 1;
                let window =
                    level % 5 == 3 && ((dx.abs() == 3 && dz == 0) || (dz.abs() == 3 && dx == 0));
                if wall && !doorway && !window {
                    world.set_landmark(centre_x + dx, base + level, z + dz);
                }
            }
        }
    }
    let top_x = x + 17 / 5;
    for dx in -4i32..=4 {
        for dz in -4i32..=4 {
            if dx.abs() == 4 || dz.abs() == 4 || (dx.abs() <= 3 && dz.abs() <= 3) {
                world.set_landmark(top_x + dx, base + 18, z + dz);
            }
        }
    }
    for dx in [-4, -2, 0, 2, 4] {
        world.set_landmark(top_x + dx, base + 19, z - 4);
        world.set_landmark(top_x + dx, base + 19, z + 4);
    }
    for dz in [-2, 0, 2] {
        world.set_landmark(top_x - 4, base + 19, z + dz);
        world.set_landmark(top_x + 4, base + 19, z + dz);
    }
}

fn add_house(world: &mut VoxelWorld, x: i32, z: i32) {
    let base = world.terrain_top(x as usize, z as usize) as i32 + 1;
    for y in 0..4 {
        for dx in -2i32..=2 {
            for dz in -2i32..=2 {
                let wall = dx.abs() == 2 || dz.abs() == 2;
                let door = dz == 2 && dx == 0 && y < 2;
                if wall && !door {
                    world.set_landmark(x + dx, base + y, z + dz);
                }
            }
        }
    }
    for roof in 0i32..=2 {
        let extent = 3 - roof;
        for dx in -extent..=extent {
            world.set_landmark(x + dx, base + 4 + roof, z - extent);
            world.set_landmark(x + dx, base + 4 + roof, z + extent);
        }
    }
}

fn add_tree(world: &mut VoxelWorld, x: i32, z: i32) {
    let base = world.terrain_top(x as usize, z as usize) as i32 + 1;
    for y in 0..4 {
        world.set_landmark(x, base + y, z);
    }
    for dy in 2..=5 {
        let radius = if dy == 5 { 1 } else { 2 };
        for dx in -radius..=radius {
            for dz in -radius..=radius {
                if dx * dx + dz * dz <= radius * radius + 1 {
                    world.set_landmark(x + dx, base + dy, z + dz);
                }
            }
        }
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
/// live UI4 aspect and shared Helio fly-camera pose are its inputs. A later
/// camera-uniform package moves this multiplication into the vertex shader
/// without changing mesh, controller, or draw ownership.
pub fn project_clip_positions(
    positions: &[[f32; 3]],
    aspect: f32,
    camera: &FlyCamera,
    lens: PerspectiveLens,
) -> Vec<[f32; 3]> {
    let eye = camera.position().to_array();
    let basis = camera.basis();
    let forward = basis.forward.to_array();
    let right = basis.right.to_array();
    // CameraBasis::up is the world movement axis. Projection needs the
    // pitched camera-up axis or mouse pitch shears the world vertically.
    let up = basis
        .right
        .cross(basis.forward)
        .normalize_or_zero()
        .to_array();
    let tan_half_fov = (lens.fov_y_radians * 0.5).tan();
    let aspect = aspect.max(0.01);
    positions
        .iter()
        .map(|position| {
            let relative = sub(*position, eye);
            let depth = dot(relative, forward).max(lens.near);
            [
                dot(relative, right) / (depth * tan_half_fov * aspect),
                dot(relative, up) / (depth * tan_half_fov),
                ((depth - lens.near) / (lens.far - lens.near).max(f32::EPSILON)).clamp(0.0, 1.0),
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
    fn world_is_face_culled_chunked_and_deterministic() {
        let first = build_voxel_world();
        let second = build_voxel_world();
        assert!(first.solid_voxels > 0);
        assert_eq!(first.chunks.len(), WORLD_CHUNKS * WORLD_CHUNKS);
        assert!(first.landmark_voxels > 500);
        assert!(first.water_voxels > 0);
        assert_eq!(first.mesh.indices.len() % 6, 0);
        assert!(first.mesh.vertices.len() < first.solid_voxels * 24);
        assert_eq!(
            first
                .chunks
                .iter()
                .map(|chunk| chunk.solid_voxels as usize)
                .sum::<usize>(),
            first.solid_voxels,
        );
        assert_eq!(
            first
                .chunks
                .iter()
                .map(|chunk| chunk.index_count as usize)
                .sum::<usize>(),
            first.mesh.indices.len(),
        );
        assert_eq!(
            mesh_fingerprint(&first.mesh),
            mesh_fingerprint(&second.mesh)
        );
    }

    #[test]
    fn projection_is_finite_and_tracks_live_aspect() {
        let positions = [[0.0, 0.0, 0.0], [47.0, 27.0, 47.0]];
        let camera = FlyCamera::look_at(
            glam::Vec3::new(65.0, 42.0, 68.0),
            glam::Vec3::new(24.0, 7.0, 24.0),
            Default::default(),
        );
        let lens = PerspectiveLens {
            fov_y_radians: 48.0_f32.to_radians(),
            near: 0.1,
            far: 180.0,
        };
        let wide = project_clip_positions(&positions, 16.0 / 9.0, &camera, lens);
        let square = project_clip_positions(&positions, 1.0, &camera, lens);
        assert!(wide.iter().flatten().all(|component| component.is_finite()));
        assert_ne!(wide[0][0], square[0][0]);
        assert_eq!(wide[0][1], square[0][1]);
    }
}
