extern crate alloc;

use crate::roster::{Keyboard, Mouse, Tablet};
use alloc::{vec, vec::Vec};

pub const MAX_KEYBOARDS: usize = 32;
pub const MAX_MICE: usize = 32;
pub const MAX_TABLETS: usize = 32;
pub const MAX_INSTANCES: usize = MAX_KEYBOARDS + MAX_MICE + MAX_TABLETS;
const PALETTE_WIDTH: usize = 256;
const PALETTE_HEIGHT: usize = 2;

/// Shared by device transforms, captions, key legends and pressed-key patches.
pub fn row_spacing(radius: f32) -> f32 {
    radius * 1.1
}

/// Prepare a single immutable PBR mesh. U comes from the GPU's compacted
/// original instance index; V selects the key or chassis material row.
pub fn prepare_mesh(source_vertices: &[u8], source_parts: &[u8]) -> Vec<u8> {
    assert_eq!(source_vertices.len() % 24, 0);
    assert_eq!(source_parts.len(), source_vertices.len() / 24);
    let mut vertices = Vec::with_capacity(source_parts.len() * 48);
    for (source, &part) in source_vertices.chunks_exact(24).zip(source_parts) {
        assert!(part <= 1);
        vertices.extend_from_slice(source);
        let normal = [
            read_f32(source, 12),
            read_f32(source, 16),
            read_f32(source, 20),
        ];
        let tangent = if normal[0].abs() > normal[2].abs() {
            [-normal[1], normal[0], 0.0]
        } else {
            [0.0, -normal[2], normal[1]]
        };
        let length = libm::sqrtf(tangent.iter().map(|value| value * value).sum());
        let tangent = if length > 0.0 {
            tangent.map(|value| value / length)
        } else {
            [1.0, 0.0, 0.0]
        };
        for value in [
            0.0,
            (f32::from(part) + 0.5) / 2.0,
            tangent[0],
            tangent[1],
            tangent[2],
            1.0,
        ] {
            vertices.extend_from_slice(&value.to_le_bytes());
        }
    }
    vertices
}

pub struct SceneGeometry {
    pub vertices: Vec<u8>,
    pub indices: Vec<u8>,
    pub keyboard_index_count: u32,
    pub mouse: MouseShape,
}

/// A single resident cuboid shared by every mouse, beside the keyboard's right
/// edge. Dimensions come from the actual model and its regular key pitch.
pub struct MouseShape {
    center: [f32; 3],
    half_size: [f32; 3],
    radius: f32,
    keyboard_caption: [f32; 3],
}

impl MouseShape {
    pub fn half_size(&self) -> [f32; 3] {
        self.half_size
    }
}

pub fn prepare_geometry(source: &[u8], parts: &[u8], indices: &[u8]) -> SceneGeometry {
    let mut vertices = prepare_mesh(source, parts);
    let mut min = [f32::INFINITY; 3];
    let mut max = [f32::NEG_INFINITY; 3];
    for vertex in source.chunks_exact(24) {
        for axis in 0..3 {
            let value = read_f32(vertex, axis * 4);
            min[axis] = min[axis].min(value);
            max[axis] = max[axis].max(value);
        }
    }
    let key_x = |scalar| {
        crate::legends::LEGENDS
            .iter()
            .find(|legend| legend.scalar == scalar)
            .unwrap()
            .center[0]
    };
    let pitch = (key_x('2') - key_x('1')).abs();
    let half_size = [
        pitch * 2.0,
        (max[1] - min[1]) * 0.5,
        (max[2] - min[2]) * 0.5,
    ];
    let mouse = MouseShape {
        center: [
            max[0] + pitch + half_size[0],
            (min[1] + max[1]) * 0.5,
            (min[2] + max[2]) * 0.5,
        ],
        half_size,
        radius: libm::sqrtf(half_size.iter().map(|v| v * v).sum()),
        keyboard_caption: [0., max[1] + 0.025, min[2] - pitch],
    };
    let keyboard_index_count = (indices.len() / 4) as u32;
    let mut indices = indices.to_vec();
    let mut cube = Vec::with_capacity(24 * 24);
    for (normal, corners) in [
        (
            [1., 0., 0.],
            [[1., -1., -1.], [1., 1., -1.], [1., 1., 1.], [1., -1., 1.]],
        ),
        (
            [-1., 0., 0.],
            [
                [-1., -1., 1.],
                [-1., 1., 1.],
                [-1., 1., -1.],
                [-1., -1., -1.],
            ],
        ),
        (
            [0., 1., 0.],
            [[-1., 1., -1.], [-1., 1., 1.], [1., 1., 1.], [1., 1., -1.]],
        ),
        (
            [0., -1., 0.],
            [
                [-1., -1., 1.],
                [-1., -1., -1.],
                [1., -1., -1.],
                [1., -1., 1.],
            ],
        ),
        (
            [0., 0., 1.],
            [[1., -1., 1.], [1., 1., 1.], [-1., 1., 1.], [-1., -1., 1.]],
        ),
        (
            [0., 0., -1.],
            [
                [-1., -1., -1.],
                [-1., 1., -1.],
                [1., 1., -1.],
                [1., -1., -1.],
            ],
        ),
    ] {
        let base = (source.len() / 24 + cube.len() / 24) as u32;
        for corner in corners {
            for value in (0..3)
                .map(|axis| corner[axis] * mouse.half_size[axis])
                .chain(normal)
            {
                cube.extend_from_slice(&value.to_le_bytes());
            }
        }
        for index in [0, 1, 2, 0, 2, 3] {
            indices.extend_from_slice(&(base + index).to_le_bytes());
        }
    }
    vertices.extend_from_slice(&prepare_mesh(&cube, &[0; 24]));
    SceneGeometry {
        vertices,
        indices,
        keyboard_index_count,
        mouse,
    }
}

pub struct RowInstances {
    pub seeds: Vec<u8>,
    pub count: u32,
    pub center: [f32; 3],
    pub radius: f32,
    pub captions: Vec<BucketCaption>,
}

pub struct BucketCaption {
    /// Global instance owning this anchor (keyboard, then mouse, then tablet).
    pub instance: usize,
    pub combo_id: u32,
    pub center: [f32; 3],
    pub kind: CaptionKind,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum CaptionKind {
    Bucket,
    Mouse,
    Tablet,
}

/// Roster updates upload only 64-byte transforms and a tiny color atlas.
/// No vertex or index data is duplicated for additional keyboards.
pub fn prepare_row(keyboards: &[Keyboard], radius: f32) -> RowInstances {
    assert!(keyboards.len() <= MAX_KEYBOARDS);
    let spacing = row_spacing(radius);
    let span = keyboards.len().saturating_sub(1) as f32 * spacing;
    let mut row = RowInstances {
        seeds: Vec::with_capacity(keyboards.len() * 64),
        count: keyboards.len() as u32,
        center: [0.0, 0.0, span * 0.5],
        radius: radius + span * 0.5,
        captions: Vec::new(),
    };
    for index in 0..keyboards.len() {
        let z = index as f32 * spacing;
        for value in [
            0.0, 0.0, z, 1.0, 1.0, 1.0, 0.0, 0.0, 0.0, 1.0, radius, 0.0, 0.0, z,
        ] {
            row.seeds.extend_from_slice(&value.to_le_bytes());
        }
        row.seeds.extend_from_slice(&0u32.to_le_bytes()); // Keyboard index range.
        row.seeds
            .extend_from_slice(&((index as u32) << 16).to_le_bytes());
    }
    row
}

/// Keep keyboard positions stable for their legends and feedback. A mouse joins
/// its nonzero combo's keyboard row; unmatched mice get their own rows.
pub fn prepare_scene(
    keyboards: &[Keyboard],
    mice: &[Mouse],
    tablets: &[Tablet],
    radius: f32,
    mouse: &MouseShape,
) -> RowInstances {
    assert!(mice.len() <= MAX_MICE);
    assert!(tablets.len() <= MAX_TABLETS);
    let mut row = prepare_row(keyboards, radius);
    for (index, keyboard) in keyboards.iter().enumerate() {
        if keyboard.combo_id == 0 || !row.captions.iter().any(|c| c.combo_id == keyboard.combo_id) {
            let mut center = mouse.keyboard_caption;
            center[2] += index as f32 * row_spacing(radius);
            row.captions.push(BucketCaption {
                instance: index,
                combo_id: keyboard.combo_id,
                center,
                kind: CaptionKind::Bucket,
            });
        }
    }
    let mut occupied = vec![false; keyboards.len()];
    let mut next_row = keyboards.len();
    let mut bounds = Vec::with_capacity(keyboards.len() + mice.len() + tablets.len());
    for index in 0..keyboards.len() {
        bounds.push(([0., 0., index as f32 * row_spacing(radius)], radius));
    }
    for (index, device) in mice.iter().enumerate() {
        let paired = keyboards.iter().enumerate().position(|(i, keyboard)| {
            !occupied[i] && device.combo_id != 0 && device.combo_id == keyboard.combo_id
        });
        let slot = if let Some(slot) = paired {
            occupied[slot] = true;
            slot
        } else {
            let slot = next_row;
            next_row += 1;
            slot
        };
        let [x, y, z] = [
            mouse.center[0],
            mouse.center[1],
            mouse.center[2] + slot as f32 * row_spacing(radius),
        ];
        for value in [x, y, z, 1., 1., 1., 0., 0., 0., 1., mouse.radius, x, y, z] {
            row.seeds.extend_from_slice(&value.to_le_bytes());
        }
        row.seeds
            .extend_from_slice(&u32::from(!keyboards.is_empty()).to_le_bytes());
        row.seeds
            .extend_from_slice(&((index as u32) << 16).to_le_bytes());
        bounds.push(([x, y, z], mouse.radius));
        if device.combo_id == 0
            || !row
                .captions
                .iter()
                .any(|c| c.kind == CaptionKind::Bucket && c.combo_id == device.combo_id)
        {
            let mut center = mouse.keyboard_caption;
            center[0] = x;
            center[2] += slot as f32 * row_spacing(radius);
            row.captions.push(BucketCaption {
                instance: keyboards.len() + index,
                combo_id: device.combo_id,
                center,
                kind: CaptionKind::Bucket,
            });
        }
        row.captions.push(BucketCaption {
            instance: keyboards.len() + index,
            combo_id: device.combo_id,
            center: [x, y + mouse.half_size[1] + 0.025, z],
            kind: CaptionKind::Mouse,
        });
    }
    for (index, device) in tablets.iter().enumerate() {
        let paired = keyboards
            .iter()
            .position(|keyboard| device.combo_id != 0 && device.combo_id == keyboard.combo_id);
        let slot = paired.unwrap_or_else(|| {
            let slot = next_row;
            next_row += 1;
            slot
        });
        let x = mouse.center[0] + mouse.half_size[0] * 2.5;
        let y = mouse.center[1] - mouse.half_size[1] * 0.75;
        let z = mouse.center[2] + slot as f32 * row_spacing(radius);
        for value in [x, y, z, 1., 0.25, 1., 0., 0., 0., 1., mouse.radius, x, y, z] {
            row.seeds.extend_from_slice(&value.to_le_bytes());
        }
        row.seeds
            .extend_from_slice(&u32::from(!keyboards.is_empty()).to_le_bytes());
        row.seeds
            .extend_from_slice(&(((mice.len() + index) as u32) << 16).to_le_bytes());
        bounds.push(([x, y, z], mouse.radius));
        if device.combo_id == 0
            || !row
                .captions
                .iter()
                .any(|c| c.kind == CaptionKind::Bucket && c.combo_id == device.combo_id)
        {
            let mut center = mouse.keyboard_caption;
            center[0] = x;
            center[2] += slot as f32 * row_spacing(radius);
            row.captions.push(BucketCaption {
                instance: keyboards.len() + mice.len() + index,
                combo_id: device.combo_id,
                center,
                kind: CaptionKind::Bucket,
            });
        }
        row.captions.push(BucketCaption {
            instance: keyboards.len() + mice.len() + index,
            combo_id: device.combo_id,
            center: [x, y + mouse.half_size[1] * 0.25 + 0.025, z],
            kind: CaptionKind::Tablet,
        });
    }
    row.count = (keyboards.len() + mice.len() + tablets.len()) as u32;
    if !mice.is_empty() || !tablets.is_empty() {
        let mut min = [f32::INFINITY; 3];
        let mut max = [f32::NEG_INFINITY; 3];
        for (center, radius) in &bounds {
            for axis in 0..3 {
                min[axis] = min[axis].min(center[axis] - radius);
                max[axis] = max[axis].max(center[axis] + radius);
            }
        }
        row.center = core::array::from_fn(|axis| (min[axis] + max[axis]) * 0.5);
        row.radius = bounds
            .iter()
            .map(|(center, radius)| {
                libm::sqrtf(
                    (0..3)
                        .map(|axis| {
                            let delta = center[axis] - row.center[axis];
                            delta * delta
                        })
                        .sum(),
                ) + radius
            })
            .fold(0., f32::max);
    }
    row
}

fn read_f32(bytes: &[u8], offset: usize) -> f32 {
    f32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
}

/// Column = original global seed index (keyboards, then mice); rows = colored
/// keys/mice and gray chassis. Culling keeps this index across both draw groups.
pub fn palette_bmp(keyboards: &[Keyboard], mice: &[Mouse], tablets: &[Tablet]) -> Vec<u8> {
    assert!(keyboards.len() <= MAX_KEYBOARDS);
    assert!(mice.len() <= MAX_MICE);
    assert!(tablets.len() <= MAX_TABLETS);
    material_bmp(Some((keyboards, mice, tablets)))
}

pub fn metallic_roughness_bmp() -> Vec<u8> {
    material_bmp(None)
}

fn material_bmp(devices: Option<(&[Keyboard], &[Mouse], &[Tablet])>) -> Vec<u8> {
    let stride = PALETTE_WIDTH * 3;
    let mut bmp = vec![0; 54 + stride * PALETTE_HEIGHT];
    bmp[..2].copy_from_slice(b"BM");
    for (offset, value) in [
        (2, (54 + stride * PALETTE_HEIGHT) as u32),
        (10, 54),
        (14, 40),
        (18, PALETTE_WIDTH as u32),
        (22, PALETTE_HEIGHT as u32),
        (34, (stride * PALETTE_HEIGHT) as u32),
    ] {
        bmp[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }
    bmp[26..28].copy_from_slice(&1u16.to_le_bytes());
    bmp[28..30].copy_from_slice(&24u16.to_le_bytes());
    for row in 0..PALETTE_HEIGHT {
        for column in 0..PALETTE_WIDTH {
            let [r, g, b, _] = match devices {
                Some((keyboards, mice, tablets)) if row == 0 => keyboards
                    .get(column)
                    .map(|keyboard| keyboard.rgba)
                    .or_else(|| {
                        column
                            .checked_sub(keyboards.len())
                            .and_then(|i| mice.get(i))
                            .map(|mouse| mouse.rgba)
                    })
                    .or_else(|| {
                        column
                            .checked_sub(keyboards.len() + mice.len())
                            .and_then(|i| tablets.get(i))
                            .map(|tablet| tablet.rgba)
                    })
                    .unwrap_or([128, 128, 128, 255]),
                Some(_) => [128, 128, 128, 255],
                None if row == 0 => [255, 255, 0, 255],
                None => [255, 102, 255, 255],
            };
            // Positive-height BMP storage is bottom-up; UV row 0 is keys.
            let at = 54 + (PALETTE_HEIGHT - 1 - row) * stride + column * 3;
            bmp[at..at + 3].copy_from_slice(&[b, g, r]);
        }
    }
    bmp
}

#[cfg(test)]
mod tests {
    use super::*;
    use trueos::hid::InputComboColor;

    fn keyboard(index: u8) -> Keyboard {
        Keyboard {
            endpoint: [1, index as u32, 0],
            combo_id: index as u32 + 1,
            rgba: InputComboColor::from_index(index % InputComboColor::COUNT).rgba(),
        }
    }
    fn pixel(bmp: &[u8], column: usize, row: usize) -> [u8; 3] {
        let at = 54 + (1 - row) * 768 + column * 3;
        [bmp[at + 2], bmp[at + 1], bmp[at]]
    }

    fn geometry() -> SceneGeometry {
        prepare_geometry(
            include_bytes!(concat!(env!("OUT_DIR"), "/keyboard.vertices.bin")),
            include_bytes!(concat!(env!("OUT_DIR"), "/keyboard.parts.bin")),
            include_bytes!(concat!(env!("OUT_DIR"), "/keyboard.indices.bin")),
        )
    }

    fn mouse(index: u8, combo_id: u32) -> Mouse {
        Mouse {
            endpoint: [2, index as u32, 1],
            combo_id,
            rgba: InputComboColor::from_index(index % InputComboColor::COUNT).rgba(),
        }
    }

    #[test]
    fn tablets_keep_distinct_slots_shared_geometry_and_palette_with_all_96_devices() {
        let geometry = geometry();
        for count in [0, 1, 32] {
            let keyboards: Vec<_> = (0..count).map(keyboard).collect();
            let mice: Vec<_> = (0..count).map(|i| mouse(i, i as u32 + 1)).collect();
            let tablets: Vec<_> = (0..count)
                .map(|i| Tablet {
                    endpoint: [3, i as u32, 1],
                    combo_id: i as u32 + 1,
                    rgba: InputComboColor::from_index((i + 1) % InputComboColor::COUNT).rgba(),
                })
                .collect();
            let row = prepare_scene(&keyboards, &mice, &tablets, 20., &geometry.mouse);
            let colors = palette_bmp(&keyboards, &mice, &tablets);
            assert_eq!(row.count, count as u32 * 3);
            for caption in &row.captions {
                assert!(caption.instance < row.count as usize);
                let seed = &row.seeds[caption.instance * 64..][..64];
                assert_eq!(caption.center[0], read_f32(seed, 0));
            }
            for (index, tablet) in tablets.iter().enumerate() {
                let mouse_seed = &row.seeds[(keyboards.len() + index) * 64..][..64];
                let tablet_seed = &row.seeds[(keyboards.len() + mice.len() + index) * 64..][..64];
                assert_eq!(read_f32(mouse_seed, 8), read_f32(tablet_seed, 8));
                assert!(
                    read_f32(tablet_seed, 0) - geometry.mouse.half_size[0]
                        > read_f32(mouse_seed, 0) + geometry.mouse.half_size[0]
                );
                assert_eq!(read_f32(tablet_seed, 16), 0.25);
                assert!(
                    (read_f32(tablet_seed, 4)
                        - geometry.mouse.half_size[1] * 0.25
                        - (read_f32(mouse_seed, 4) - geometry.mouse.half_size[1]))
                        .abs()
                        < 1e-5
                );
                assert_eq!(
                    u32::from_le_bytes(tablet_seed[56..60].try_into().unwrap()),
                    1
                );
                assert_eq!(
                    u32::from_le_bytes(tablet_seed[60..64].try_into().unwrap()),
                    ((mice.len() + index) as u32) << 16
                );
                assert_eq!(
                    pixel(&colors, keyboards.len() + mice.len() + index, 0),
                    tablet.rgba[..3]
                );
            }
            let only_tablets = prepare_scene(&[], &[], &tablets, 20., &geometry.mouse);
            assert_eq!(only_tablets.count, tablets.len() as u32);
            for seed in only_tablets.seeds.chunks_exact(64) {
                assert_eq!(u32::from_le_bytes(seed[56..60].try_into().unwrap()), 0);
            }
        }
    }

    #[test]
    fn shared_mouse_cuboid_has_keyboard_height_depth_and_outward_faces() {
        let scene = geometry();
        let source = include_bytes!(concat!(env!("OUT_DIR"), "/keyboard.vertices.bin"));
        let source_indices = include_bytes!(concat!(env!("OUT_DIR"), "/keyboard.indices.bin"));
        let vertex_count = source.len() / 24;
        assert_eq!(scene.vertices.len(), (vertex_count + 24) * 48);
        assert_eq!(scene.indices.len(), source_indices.len() + 36 * 4);
        assert_eq!(&scene.indices[..source_indices.len()], source_indices);
        let mut min = [f32::INFINITY; 3];
        let mut max = [f32::NEG_INFINITY; 3];
        for vertex in source.chunks_exact(24) {
            for axis in 0..3 {
                min[axis] = min[axis].min(read_f32(vertex, axis * 4));
                max[axis] = max[axis].max(read_f32(vertex, axis * 4));
            }
        }
        for axis in [1, 2] {
            assert!(
                (scene.mouse.center[axis] - scene.mouse.half_size[axis] - min[axis]).abs() < 1e-5
            );
            assert!(
                (scene.mouse.center[axis] + scene.mouse.half_size[axis] - max[axis]).abs() < 1e-5
            );
        }
        assert!(scene.mouse.center[0] - scene.mouse.half_size[0] > max[0]);
        let pitch = (crate::legends::LEGENDS
            .iter()
            .find(|l| l.scalar == '2')
            .unwrap()
            .center[0]
            - crate::legends::LEGENDS
                .iter()
                .find(|l| l.scalar == '1')
                .unwrap()
                .center[0])
            .abs();
        assert!((scene.mouse.half_size[0] * 2. - pitch * 4.).abs() < 1e-5);
        for triangle in scene.indices[source_indices.len()..].chunks_exact(12) {
            let indices: [usize; 3] = core::array::from_fn(|i| {
                u32::from_le_bytes(triangle[i * 4..i * 4 + 4].try_into().unwrap()) as usize
            });
            assert!(
                indices
                    .iter()
                    .all(|&i| i >= vertex_count && i < vertex_count + 24)
            );
            let point = |i: usize| {
                core::array::from_fn::<_, 3, _>(|axis| {
                    read_f32(&scene.vertices[i * 48..], axis * 4)
                })
            };
            let [a, b, c] = indices.map(point);
            let ab: [f32; 3] = core::array::from_fn(|i| b[i] - a[i]);
            let ac: [f32; 3] = core::array::from_fn(|i| c[i] - a[i]);
            let cross = [
                ab[1] * ac[2] - ab[2] * ac[1],
                ab[2] * ac[0] - ab[0] * ac[2],
                ab[0] * ac[1] - ab[1] * ac[0],
            ];
            assert!(
                (0..3)
                    .map(|i| cross[i] * read_f32(&scene.vertices[indices[0] * 48..], 12 + i * 4))
                    .sum::<f32>()
                    > 0.
            );
        }
    }

    #[test]
    fn mice_pair_by_combo_and_unpaired_mice_get_distinct_rows() {
        let geometry = geometry();
        let keyboards = [keyboard(0), keyboard(1)];
        let mice = [mouse(0, 2), mouse(1, 1), mouse(2, 0), mouse(3, 1)];
        let row = prepare_scene(&keyboards, &mice, &[], 20., &geometry.mouse);
        assert_eq!(row.count, 6);
        // Shared keyboard/mouse buckets have one caption with the actual ID.
        assert_eq!(
            row.captions
                .iter()
                .filter(|c| c.kind == CaptionKind::Bucket)
                .map(|c| c.combo_id)
                .collect::<Vec<_>>(),
            [1, 2, 0]
        );
        assert!((row.captions[1].center[2] - row.captions[0].center[2] - 22.).abs() < 1e-5);
        assert_eq!(&row.seeds[..128], &prepare_row(&keyboards, 20.).seeds);
        let colors = palette_bmp(&keyboards, &mice, &[]);
        for (index, slot) in [1, 0, 2, 3].into_iter().enumerate() {
            let seed = &row.seeds[(2 + index) * 64..][..64];
            assert!(
                (read_f32(seed, 8) - geometry.mouse.center[2] - slot as f32 * 22.).abs() < 1e-5
            );
            assert_eq!(u32::from_le_bytes(seed[56..60].try_into().unwrap()), 1);
            assert_eq!(
                u32::from_le_bytes(seed[60..64].try_into().unwrap()),
                (index as u32) << 16
            );
            assert_eq!(pixel(&colors, 2 + index, 0), mice[index].rgba[..3]);
            let distance = libm::sqrtf(
                (0..3)
                    .map(|axis| {
                        let delta = read_f32(seed, axis * 4) - row.center[axis];
                        delta * delta
                    })
                    .sum(),
            );
            assert!(distance + geometry.mouse.radius <= row.radius + 1e-4);
        }
    }

    #[test]
    fn zero_keyboard_and_full_rosters_keep_valid_groups_and_palette_columns() {
        let geometry = geometry();
        for (keyboard_count, mouse_count) in [(0, 0), (0, 2), (2, 0), (32, 32)] {
            let keyboards: Vec<_> = (0..keyboard_count).map(keyboard).collect();
            let mice: Vec<_> = (0..mouse_count).map(|i| mouse(i, i as u32 + 1)).collect();
            let row = prepare_scene(&keyboards, &mice, &[], 20., &geometry.mouse);
            assert_eq!(row.count as usize, keyboards.len() + mice.len());
            assert_eq!(row.seeds.len(), row.count as usize * 64);
            assert!(row.radius.is_finite() && row.radius > 0.);
            if row.count == 0 {
                assert!(row.captions.is_empty());
            }
            if keyboards.is_empty() && !mice.is_empty() {
                assert_eq!(
                    row.captions
                        .iter()
                        .filter(|c| c.kind == CaptionKind::Bucket)
                        .count(),
                    mice.len()
                );
            }
            let colors = palette_bmp(&keyboards, &mice, &[]);
            for (index, device) in mice.iter().enumerate() {
                let seed = &row.seeds[(keyboards.len() + index) * 64..][..64];
                assert_eq!(
                    u32::from_le_bytes(seed[56..60].try_into().unwrap()),
                    u32::from(!keyboards.is_empty())
                );
                assert_eq!(pixel(&colors, keyboards.len() + index, 0), device.rgba[..3]);
            }
        }
    }

    #[test]
    fn k70_geometry_is_prepared_once_with_chassis_material_and_no_duplication() {
        let source = include_bytes!(concat!(env!("OUT_DIR"), "/keyboard.vertices.bin"));
        let parts = include_bytes!(concat!(env!("OUT_DIR"), "/keyboard.parts.bin"));
        let mesh = prepare_mesh(source, parts);
        assert_eq!(parts.len(), 36152);
        assert_eq!(parts.iter().filter(|&&part| part == 1).count(), 88);
        assert_eq!(mesh.len(), parts.len() * 48);
        for ((vertex, source), &part) in mesh
            .chunks_exact(48)
            .zip(source.chunks_exact(24))
            .zip(parts)
        {
            assert_eq!(&vertex[..24], source);
            assert_eq!(read_f32(vertex, 28), if part == 1 { 0.75 } else { 0.25 });
            let normal: [f32; 3] = core::array::from_fn(|i| read_f32(vertex, 12 + i * 4));
            let tangent: [f32; 3] = core::array::from_fn(|i| read_f32(vertex, 32 + i * 4));
            assert!(
                normal
                    .iter()
                    .zip(tangent)
                    .map(|(a, b)| a * b)
                    .sum::<f32>()
                    .abs()
                    < 0.0001
            );
        }
        for count in [0, 1, 2, 32] {
            let keyboards: Vec<_> = (0..count).map(keyboard).collect();
            let row = prepare_row(&keyboards, 20.0);
            assert_eq!(row.seeds.len(), count as usize * 64);
            assert_eq!(row.count, count as u32);
            for (index, seed) in row.seeds.chunks_exact(64).enumerate() {
                let z = index as f32 * 22.0;
                assert_eq!(read_f32(seed, 8), z);
                assert_eq!(read_f32(seed, 52), z);
                assert_eq!(read_f32(seed, 40), 20.0);
                assert_eq!(
                    u32::from_le_bytes(seed[60..64].try_into().unwrap()),
                    (index as u32) << 16
                );
                assert!((z - row.center[2]).abs() + 20.0 <= row.radius + 0.001);
            }
        }
    }

    #[test]
    fn compaction_keeps_exact_combo_colors_and_chassis_properties() {
        let keyboards: Vec<_> = (0..32).map(keyboard).collect();
        let colors = palette_bmp(&keyboards, &[], &[]);
        let properties = metallic_roughness_bmp();
        assert_eq!(colors.len(), 1590);
        assert_eq!(&colors[..2], b"BM");
        assert_eq!(u32::from_le_bytes(colors[18..22].try_into().unwrap()), 256);
        assert_eq!(u32::from_le_bytes(colors[22..26].try_into().unwrap()), 2);
        // VF fetches the GPU compaction row's low byte as R8_UNORM.
        // Even when preceding boards are culled, the original index selects
        // its own exact 8-bit color rather than the compacted draw slot.
        for original in [0usize, 7, 16, 31] {
            let u = original as f32 / 255.0;
            let texel = libm::floorf(u * 256.0) as usize;
            assert_eq!(texel, original);
            assert_eq!(pixel(&colors, texel, 0), keyboards[original].rgba[..3]);
            assert_eq!(pixel(&colors, texel, 1), [128; 3]);
            assert_eq!(pixel(&properties, texel, 0), [255, 255, 0]);
            assert_eq!(pixel(&properties, texel, 1), [255, 102, 255]);
        }
        assert_eq!(pixel(&palette_bmp(&[], &[], &[]), 0, 0), [128; 3]);
    }
}
