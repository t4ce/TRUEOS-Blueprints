//! Image gallery v1: six inward-facing slabs and a standard PNG atlas.
//! Tier IDs are the only geometry settings accepted over the wire.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Tier { pub source: u32, pub blocks: u32, pub pixels: u32 }
pub const TIERS: [Tier; 4] = [
    Tier { source: 64, blocks: 12, pixels: 60 },
    Tier { source: 128, blocks: 18, pixels: 90 },
    Tier { source: 256, blocks: 24, pixels: 120 },
    Tier { source: 512, blocks: 32, pixels: 320 },
];
pub const FACES: usize = 6;
pub const HEADER: usize = 16;
pub const MAX_BYTES: usize = 4 * 1024 * 1024;
/// Face order: -Z, +X, +Z, -X, -Y, +Y (north/east/south/west/bottom/top).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Layout { pub tiers: [u8; FACES] }
impl Layout {
    pub fn tier(self, face: usize) -> Tier { TIERS[(self.tiers[face] - 1) as usize] }
    pub fn tile(self) -> u32 { self.tiers.iter().map(|t| TIERS[(*t-1) as usize].pixels).max().unwrap() + 2 }
    pub fn extent(self) -> [u32; 2] { [self.tile()*3, self.tile()*2] }
    /// One duplicated edge texel prevents neighboring pictures bleeding at borders.
    pub fn uv(self, face: usize, uv: [f32; 2]) -> [f32; 2] {
        let tile = self.tile();
        let pixels = self.tier(face).pixels as f32;
        let [w,h] = self.extent().map(|v| v as f32);
        [(face as u32 % 3 * tile + 1) as f32 / w + uv[0]*pixels/w,
         (face as u32 / 3 * tile + 1) as f32 / h + uv[1]*pixels/h]
    }
    pub fn parse(bytes: &[u8]) -> Option<(Self, &[u8])> {
        if bytes.len() <= HEADER || bytes.len() > MAX_BYTES || &bytes[..4] != b"CGA1"
            || bytes[4] != 1 || bytes[5] != 6 || bytes[12..16] != [0;4] { return None; }
        let tiers: [u8;6] = bytes[6..12].try_into().ok()?;
        if tiers.iter().any(|t| !(1..=4).contains(t)) { return None; }
        let layout = Self { tiers };
        let png = &bytes[HEADER..];
        // Bound decoded allocation before invoking the native decoder.
        if png.len() < 33 || &png[..8] != b"\x89PNG\r\n\x1a\n" || png[8..12] != 13u32.to_be_bytes()
            || &png[12..16] != b"IHDR" { return None; }
        let extent = [u32::from_be_bytes(png[16..20].try_into().ok()?), u32::from_be_bytes(png[20..24].try_into().ok()?)];
        if extent != layout.extent() || png[24..29] != [8, 2, 0, 0, 0] { return None; }
        Some((layout, png))
    }
}
