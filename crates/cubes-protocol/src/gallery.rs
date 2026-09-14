//! Image gallery v8: six inward-facing c1 grids plus a sparse-frame palette.
//! Tier IDs encode regular grids of unit cubes; positions use the c1 lattice.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Tier {
    pub source: u32,
    pub blocks: u32,
    pub pixels: u32,
}
pub const TIERS: [Tier; 3] = [
    Tier {
        source: 48,
        blocks: 6,
        pixels: 6,
    },
    Tier {
        source: 128,
        blocks: 8,
        pixels: 16,
    },
    Tier {
        source: 256,
        blocks: 16,
        pixels: 128,
    },
];
impl Tier {
    /// These three presets align exactly; no extra texture crop is necessary.
    pub const fn grid(self) -> u32 {
        self.pixels
    }
}
pub const VERSION: u8 = 8;
pub const C1: f32 = 0.2;
pub const CUBE_SIDE: i32 = 1;
pub const WORLD_HALF_C1: i32 = 1024;
/// Gallery centers sit at the nearest c1 half-cell to 10% of the world radius.
pub const GALLERY_DISTANCE_TWICE_C1: i32 = 205;
/// The center landmark uses the same c4 cubes as platforms and pathways.
pub const CENTER_CUBE_SIDE_C1: i32 = 8;
pub const CENTER_CUBES_PER_AXIS: i32 = 3;
/// Right, up, inward, in integer c1 axes. Right × up = inward.
pub const BASES: [[[i32; 3]; 3]; 6] = [
    [[1, 0, 0], [0, 1, 0], [0, 0, 1]],
    [[0, 0, 1], [0, 1, 0], [-1, 0, 0]],
    [[-1, 0, 0], [0, 1, 0], [0, 0, -1]],
    [[0, 0, -1], [0, 1, 0], [1, 0, 0]],
    [[1, 0, 0], [0, 0, -1], [0, 1, 0]],
    [[1, 0, 0], [0, 0, 1], [0, -1, 0]],
];
pub const FACES: usize = 6;
pub const HEADER: usize = 16;
pub const MAX_BYTES: usize = 4 * 1024 * 1024;
/// Face order: -Z, +X, +Z, -X, -Y, +Y (north/east/south/west/bottom/top).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Layout {
    pub tiers: [u8; FACES],
}
impl Layout {
    pub fn tier(self, face: usize) -> Tier {
        TIERS[(self.tiers[face] - 1) as usize]
    }
    /// Expand the compact face descriptor into an integer cube minimum.
    /// Rows run bottom to top, columns image-right to image-left, matching
    /// the reference cube's +Y/+Z axes. The depth cell is inside the world.
    pub fn cube_min(self, face: usize, row: u32, column: u32) -> [i32; 3] {
        let count = self.tier(face).blocks as i32;
        assert!(row < count as u32 && column < count as u32);
        let [u, v, n] = BASES[face];
        core::array::from_fn(|a| {
            let twice_center = -n[a] * GALLERY_DISTANCE_TWICE_C1
                + u[a] * (count - 1 - 2 * column as i32)
                + v[a] * (2 * row as i32 + 1 - count);
            (twice_center - 1) / 2
        })
    }
    pub fn tile(self) -> u32 {
        self.tiers
            .iter()
            .map(|t| TIERS[(*t - 1) as usize].grid())
            .max()
            .unwrap()
            + 2
    }
    /// The final row contains the white texel used by the center landmark.
    pub fn extent(self) -> [u32; 2] {
        [self.tile() * 3, self.tile() * 2 + 1]
    }
    pub fn white_uv(self) -> [f32; 2] {
        let [w, h] = self.extent().map(|v| v as f32);
        [0.5 / w, (self.tile() as f32 * 2. + 0.5) / h]
    }
    /// Palette texels for the VFX catalog follow the landmark's white texel.
    pub fn vfx_uv(self, palette: u8) -> [f32; 2] {
        let [w, h] = self.extent().map(|v| v as f32);
        [
            (palette as f32 + 1.5) / w,
            (self.tile() as f32 * 2. + 0.5) / h,
        ]
    }
    /// One duplicated edge texel prevents neighboring pictures bleeding at borders.
    pub fn uv(self, face: usize, uv: [f32; 2]) -> [f32; 2] {
        let tile = self.tile();
        let pixels = self.tier(face).grid() as f32;
        let [w, h] = self.extent().map(|v| v as f32);
        [
            (face as u32 % 3 * tile + 1) as f32 / w + uv[0] * pixels / w,
            (face as u32 / 3 * tile + 1) as f32 / h + uv[1] * pixels / h,
        ]
    }
    pub fn parse(bytes: &[u8]) -> Option<(Self, &[u8])> {
        if bytes.len() <= HEADER
            || bytes.len() > MAX_BYTES
            || &bytes[..4] != b"CGA1"
            || bytes[4] != VERSION
            || bytes[5] != 6
            || bytes[12] != CUBE_SIDE as u8
            || bytes[13] != 1
            || bytes[14..16] != (WORLD_HALF_C1 as u16).to_le_bytes()
        {
            return None;
        }
        let tiers: [u8; 6] = bytes[6..12].try_into().ok()?;
        if tiers.iter().any(|t| !(1..=3).contains(t)) {
            return None;
        }
        let layout = Self { tiers };
        let png = &bytes[HEADER..];
        // Bound decoded allocation before invoking the native decoder.
        if png.len() < 33
            || &png[..8] != b"\x89PNG\r\n\x1a\n"
            || png[8..12] != 13u32.to_be_bytes()
            || &png[12..16] != b"IHDR"
        {
            return None;
        }
        let extent = [
            u32::from_be_bytes(png[16..20].try_into().ok()?),
            u32::from_be_bytes(png[20..24].try_into().ok()?),
        ];
        if extent != layout.extent() || png[24..29] != [8, 2, 0, 0, 0] {
            return None;
        }
        Some((layout, png))
    }
}
