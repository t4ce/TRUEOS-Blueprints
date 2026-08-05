//! grid's default configuration.
//!
//! Everything the app needs to lay itself out lives here, in one place, with
//! working defaults. Nothing is required at the call site: `grid` launched N
//! times tiles itself from these values.

/// Tiles across and down. `COLUMNS * ROWS` is the number of instances that get
/// their own cell before placement wraps.
pub const COLUMNS: u32 = 4;
pub const ROWS: u32 = 2;

/// The frame extent. This is the size grid has always opened at; tiling moves
/// frames, it does not resize them.
pub const TILE_WIDTH: u32 = 640;
pub const TILE_HEIGHT: u32 = 360;

/// Gap between tiles, and the margin around the whole wall, in pixels.
pub const TILE_GAP_PX: u32 = 12;
pub const WALL_MARGIN_PX: u32 = 24;

/// Frame background.
pub const CLEAR_RGBA: (u8, u8, u8, u8) = (0, 0, 0, 255);

/// The identity stamp drawn at the top of every tile. White, as specified.
pub const ID_RGBA: (u8, u8, u8, u8) = (255, 255, 255, 255);
/// Cap height of the ID as a fraction of the tile height, then clamped.
pub const ID_HEIGHT_FRACTION: f32 = 1.0 / 4.0;
pub const ID_MIN_PIXELS: f32 = 18.0;
pub const ID_MAX_PIXELS: f32 = 220.0;
/// Distance from the top of the tile to the top of the ID's em box.
pub const ID_TOP_INSET_PX: f32 = 10.0;

/// Fallback extent used only if UI4 cannot report the output size.
pub const FALLBACK_OUTPUT: (u32, u32) = (2560, 1440);

/// Inconsolata is monospace at exactly 500/1000 units per em, so a string's
/// advance is `chars * 0.5 * font_pixels`. FontKernel exposes no advance query
/// to a Blueprint; this identity is what makes centring exact.
pub const INCONSOLATA_ADVANCE_EM: f32 = 0.5;

/// One tile's placement on the wall.
pub struct Tile {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

/// Place tile `index`. The extent is always `TILE_WIDTH x TILE_HEIGHT`; only
/// the origin moves, so eight instances sit beside each other at the size grid
/// has always used.
///
/// Indices beyond `COLUMNS * ROWS` wrap, so a ninth instance shares the first
/// cell rather than landing offscreen.
pub fn tile_for(index: u32, _output: (u32, u32)) -> Tile {
    let cells = COLUMNS.max(1) * ROWS.max(1);
    let slot = index % cells;
    let column = slot % COLUMNS.max(1);
    let row = slot / COLUMNS.max(1);

    Tile {
        x: (WALL_MARGIN_PX + column * (TILE_WIDTH + TILE_GAP_PX)) as i32,
        y: (WALL_MARGIN_PX + row * (TILE_HEIGHT + TILE_GAP_PX)) as i32,
        width: TILE_WIDTH,
        height: TILE_HEIGHT,
    }
}
