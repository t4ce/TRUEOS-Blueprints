use alloc::vec::Vec;
use microgames::Rgb8;

include!(concat!(env!("OUT_DIR"), "/chess_assets.rs"));

const MAX_PER_PIECE: usize = 192;

#[derive(Clone, Copy)]
pub struct AssetCube {
    pub center: [f32; 3],
    pub scale: f32,
    pub color: Rgb8,
}

pub struct ChessAssets {
    pieces: [Option<Vec<AssetCube>>; 6],
}

impl ChessAssets {
    pub fn new() -> Self {
        Self {
            pieces: core::array::from_fn(|i| {
                CHESS_ASSET_BYTES[i].and_then(|bytes| decode(bytes).ok())
            }),
        }
    }

    pub fn piece(&self, index: usize) -> Option<&[AssetCube]> {
        self.pieces[index].as_deref()
    }

    pub fn loaded(&self) -> usize {
        self.pieces.iter().filter(|p| p.is_some()).count()
    }
}

pub(crate) fn decode(bytes: &[u8]) -> Result<Vec<AssetCube>, &'static str> {
    if bytes.len() < 16
        || &bytes[..4] != b"CUBE"
        || !matches!(bytes[4], 1 | 2)
        || bytes[5] != 0
        || bytes[6] == 0
        || bytes[6] >= 100
        || bytes[11] != 4
    {
        return Err("chess asset header");
    }
    let version = bytes[4];
    let stride = if version == 1 { 8 } else { 12 };
    if bytes[7] as usize != stride {
        return Err("chess asset stride");
    }
    let count = u16::from_le_bytes([bytes[8], bytes[9]]) as usize;
    let colors = bytes[10] as usize;
    let unit = f32::from_le_bytes(bytes[12..16].try_into().unwrap());
    let start = 16 + colors * 4;
    if count == 0
        || count > 16384
        || colors == 0
        || !unit.is_finite()
        || unit <= 0.0
        || bytes.len() != start + count * stride
    {
        return Err("chess asset size");
    }
    if bytes[16..start].chunks_exact(4).any(|p| p[3] != 255) {
        return Err("chess asset alpha");
    }
    let mut cubes = Vec::new();
    for record in bytes[start..].chunks_exact(stride) {
        let (origin, side, color, tier) = if version == 1 {
            if record[6] != 0 || record[7] != 0 {
                return Err("chess asset reserved");
            }
            (
                [
                    record[0] as i8 as i32,
                    record[1] as i8 as i32,
                    record[2] as i8 as i32,
                ],
                record[3] as i32,
                record[4] as usize,
                record[3] as i32,
            )
        } else {
            if record[10] != 0 || record[11] != 0 {
                return Err("chess asset reserved");
            }
            let origin = core::array::from_fn(|a| {
                i16::from_le_bytes([record[a * 2], record[a * 2 + 1]]) as i32
            });
            (
                origin,
                record[6] as i32,
                record[7] as usize,
                record[9] as i32,
            )
        };
        if color >= colors
            || tier <= 0
            || side < tier
            || side > 32
            || side % tier != 0
            || side / tier > 4
            || (version == 1 && side > 4)
        {
            return Err("chess asset record");
        }
        if version == 2 && (17..=30).contains(&record[8]) {
            continue;
        }
        let p = &bytes[16 + color * 4..][..4];
        let tint = Rgb8::new(p[0], p[1], p[2]);
        let n = side / tier;
        for ix in 0..n {
            for iy in 0..n {
                for iz in 0..n {
                    let center = [
                        (origin[0] as f32 + (ix * tier) as f32 + tier as f32 * 0.5) * unit,
                        (origin[1] as f32 + (iy * tier) as f32 + tier as f32 * 0.5) * unit,
                        (origin[2] as f32 + (iz * tier) as f32 + tier as f32 * 0.5) * unit,
                    ];
                    let gap = if version == 1 {
                        bytes[6] as f32 / 100.0
                    } else {
                        bytes[6] as f32 / 1000.0
                    };
                    let scale = (tier as f32 - gap) * unit * 0.5;
                    if !scale.is_finite() || scale <= 0.0 {
                        return Err("chess asset scale");
                    }
                    cubes.push(AssetCube {
                        center,
                        scale,
                        color: tint,
                    });
                }
            }
        }
    }
    if cubes.is_empty() {
        return Err("chess asset empty");
    }
    let mut lo = [f32::INFINITY; 3];
    let mut hi = [f32::NEG_INFINITY; 3];
    for cube in &cubes {
        for a in 0..3 {
            lo[a] = lo[a].min(cube.center[a] - cube.scale);
            hi[a] = hi[a].max(cube.center[a] + cube.scale);
        }
    }
    let extent = (0..3).map(|a| hi[a] - lo[a]).fold(0.0f32, f32::max);
    if !extent.is_finite() || extent <= 0.0 {
        return Err("chess asset bounds");
    }
    let factor = 0.94 / extent;
    for cube in &mut cubes {
        for a in 0..3 {
            cube.center[a] = (cube.center[a] - (lo[a] + hi[a]) * 0.5) * factor;
        }
        cube.scale *= factor;
    }
    if cubes.len() > MAX_PER_PIECE {
        let len = cubes.len();
        cubes = (0..MAX_PER_PIECE)
            .map(|i| cubes[i * len / MAX_PER_PIECE])
            .collect();
    }
    Ok(cubes)
}
