//! Shared Key-4 profile contract and the minimum WorldShowcase terrace.
//! Coordinates are authored c1 units; one of the world's 4³ chunks is 512³ c1.
#![no_std]
extern crate alloc;
pub mod gallery;
use alloc::vec::Vec;

pub const USERNAME: &str = "t4ce";
pub const CHUNK_SIDE: i32 = 512;
pub const TERRACE_TOP: i32 = -88; // Nearest c4 lattice point to 1/3 above the bottom.
pub const THEME_NAMES: [&str; 6] = [
    "sky",
    "underground",
    "black-hole",
    "white-hole",
    "island",
    "city",
];
// Exported from the six pure-world CUBES palettes; checked against the editor assets.
const PALETTE: &[u8; 24] = include_bytes!("../palette.rgba");
const fn color(i: usize) -> [u8; 4] {
    [
        PALETTE[i * 4],
        PALETTE[i * 4 + 1],
        PALETTE[i * 4 + 2],
        PALETTE[i * 4 + 3],
    ]
}
pub const COLORS: [[u8; 4]; 6] = [color(0), color(1), color(2), color(3), color(4), color(5)];
pub const MAX_PLACED: usize = 16_384;
pub const MAX_PROFILE_BYTES: usize = 4 * 1024 * 1024;
pub fn valid_username(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 32
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlacedCube {
    pub center: [f32; 3],
    pub scale: f32,
    pub flags: u32,
}
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    pub version: u8,
    pub username: alloc::string::String,
    pub generation: u64,
    pub revision: u64,
    pub theme: u8,
    pub terrain: Vec<u8>,
    pub placed: Vec<PlacedCube>,
}
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Save {
    pub generation: u64,
    pub revision: u64,
    pub placed: Vec<PlacedCube>,
}
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Create {
    pub theme: u8,
}
impl Profile {
    pub fn new(username: &str, theme: u8, generation: u64) -> Option<Self> {
        if !valid_username(username) {
            return None;
        }
        Some(Self {
            version: 1,
            username: username.into(),
            generation,
            revision: 0,
            theme,
            terrain: generate(theme)?,
            placed: Vec::new(),
        })
    }
    pub fn valid(&self, username: &str) -> bool {
        self.version == 1
            && self.username == username
            && valid_username(username)
            && self.generation != 0
            && theme(&self.terrain) == Some(self.theme)
            && valid_placements(&self.placed)
    }
}
pub fn valid_placements(cubes: &[PlacedCube]) -> bool {
    let half = CHUNK_SIDE as f32 * 0.2 * 0.5;
    cubes.len() <= MAX_PLACED && cubes.iter().all(|c| {
        c.scale.is_finite() && c.scale > 0.0 && c.scale <= 1.2
            && c.center.iter().all(|v| v.is_finite() && v.abs() + c.scale <= half + 0.0001)
            // Ordinary RGB555 or the existing six-material cube flag.
            && ((c.flags & 0x8000 != 0 && c.flags <= 0xffff) || (c.flags & !7 == 0x6000 && c.flags & 7 < 6))
    })
}
/// Same minimum roll (16), c4 lattice, corner cuts and two layers as moduleVoxels.
pub fn generate(theme: u8) -> Option<Vec<u8>> {
    let color = *COLORS.get(theme.checked_sub(1)? as usize)?;
    let mut b = alloc::vec![0; 20];
    b[..4].copy_from_slice(b"CUBE");
    b[4] = 2;
    b[6] = 14;
    b[7] = 12;
    b[10] = 1;
    b[11] = 4;
    b[12..16].copy_from_slice(&0.2f32.to_le_bytes());
    b[16..20].copy_from_slice(&color);
    for layer in 0..2i32 {
        let width = 16 - layer * 2;
        let lo = -width / 2;
        let hi = lo + width;
        let cut = width * 18 / 100;
        for x in lo..hi {
            for z in lo..hi {
                if (x - lo).min(hi - 1 - x) + (z - lo).min(hi - 1 - z) < cut {
                    continue;
                }
                for p in [x * 8, TERRACE_TOP - (layer + 1) * 8, z * 8] {
                    b.extend_from_slice(&(p as i16).to_le_bytes());
                }
                b.extend_from_slice(&[8, 0, 0, 8, 0, 0]);
            }
        }
    }
    let count = ((b.len() - 20) / 12) as u16;
    b[8..10].copy_from_slice(&count.to_le_bytes());
    Some(b)
}
/// The fixed terrain contains exactly one generated terrace; placements are separate.
pub fn theme(bytes: &[u8]) -> Option<u8> {
    if bytes.len() < 20 || bytes.len() > 64 * 1024 {
        return None;
    }
    let theme = COLORS.iter().position(|c| bytes[16..20] == c[..])? as u8 + 1;
    (generate(theme)?.as_slice() == bytes).then_some(theme)
}
