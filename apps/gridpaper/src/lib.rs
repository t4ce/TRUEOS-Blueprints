#![no_std]

//! A statically allocated, double-buffered DIN A4 grid-paper document.
//!
//! The stable raw format is row-major. Each 5 mm cell occupies exactly
//! [`CELL_BYTES`] bytes:
//!
//! - byte 0: primary-glyph UTF-8 byte length (`0..=4`)
//! - byte 1: upper-glyph UTF-8 byte length (`0..=4`)
//! - byte 2: foreground [`Color`]
//! - byte 3: background [`Color`]
//! - byte 4: [`CellStyle`] bits
//! - bytes 5..9: primary-glyph UTF-8 bytes, zero-padded
//! - bytes 9..13: optional upper-glyph UTF-8 bytes, zero-padded
//!
//! Raw access is safe but can create invalid encoded cells. Typed reads report
//! such data as [`CellError`] instead of assuming that arbitrary bytes are valid.

use core::fmt;

pub use trueos::gridpaper::{
    AnimationDefinitionError, AnimationIteration, AnimationTiming, COLOR_KEYFRAME_CAPACITY,
    ColorAnimation, ColorChannels, ColorKeyframe, Rgba8, TEXT_COLOR_ANIMATION_SLOTS,
};

pub const A4_WIDTH_MM: usize = 210;
pub const A4_HEIGHT_MM: usize = 297;
pub const CELL_EDGE_MM: usize = 5;
pub const COLUMNS: usize = 37;
pub const ROWS: usize = 53;
pub const GRID_WIDTH_MM: usize = COLUMNS * CELL_EDGE_MM;
pub const GRID_HEIGHT_MM: usize = ROWS * CELL_EDGE_MM;
pub const GRID_HORIZONTAL_MARGIN_MM: f32 = (A4_WIDTH_MM - GRID_WIDTH_MM) as f32 / 2.0;
pub const GRID_VERTICAL_MARGIN_MM: f32 = (A4_HEIGHT_MM - GRID_HEIGHT_MM) as f32 / 2.0;
pub const FULL_ROWS: usize = ROWS;
pub const FINAL_ROW_HEIGHT_MM: usize = 0;
pub const CELL_COUNT: usize = COLUMNS * ROWS;

pub const GLYPH_UTF8_CAPACITY: usize = 4;
pub const CELL_BYTES: usize = 5 + GLYPH_UTF8_CAPACITY * 2;
pub const ROW_BYTES: usize = COLUMNS * CELL_BYTES;
pub const PAGE_BYTES: usize = CELL_COUNT * CELL_BYTES;
pub const DOUBLE_BUFFER_BYTES: usize = PAGE_BYTES * 2;
pub const DEFAULT_SCALE_PERCENT: u16 = 100;

pub const PRIMARY_LENGTH_OFFSET: usize = 0;
pub const UPPER_LENGTH_OFFSET: usize = 1;
pub const FOREGROUND_OFFSET: usize = 2;
pub const BACKGROUND_OFFSET: usize = 3;
pub const STYLE_OFFSET: usize = 4;
pub const PRIMARY_OFFSET: usize = 5;
pub const UPPER_OFFSET: usize = PRIMARY_OFFSET + GLYPH_UTF8_CAPACITY;

/// A compact palette shared by foreground and background fields.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[repr(u8)]
pub enum Color {
    #[default]
    Default = 0,
    Black = 1,
    Red = 2,
    Green = 3,
    Yellow = 4,
    Blue = 5,
    Magenta = 6,
    Cyan = 7,
    White = 8,
    BrightBlack = 9,
    BrightRed = 10,
    BrightGreen = 11,
    BrightYellow = 12,
    BrightBlue = 13,
    BrightMagenta = 14,
    BrightCyan = 15,
    BrightWhite = 16,
    Transparent = 17,
}

impl TryFrom<u8> for Color {
    type Error = CellError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::Default),
            1 => Ok(Self::Black),
            2 => Ok(Self::Red),
            3 => Ok(Self::Green),
            4 => Ok(Self::Yellow),
            5 => Ok(Self::Blue),
            6 => Ok(Self::Magenta),
            7 => Ok(Self::Cyan),
            8 => Ok(Self::White),
            9 => Ok(Self::BrightBlack),
            10 => Ok(Self::BrightRed),
            11 => Ok(Self::BrightGreen),
            12 => Ok(Self::BrightYellow),
            13 => Ok(Self::BrightBlue),
            14 => Ok(Self::BrightMagenta),
            15 => Ok(Self::BrightCyan),
            16 => Ok(Self::BrightWhite),
            17 => Ok(Self::Transparent),
            other => Err(CellError::InvalidColor(other)),
        }
    }
}

/// Composable style bits stored in a cell's fourth byte.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[repr(transparent)]
pub struct CellStyle(u8);

impl CellStyle {
    pub const NONE: Self = Self(0);
    pub const BOLD: Self = Self(1 << 0);
    pub const STRIKEOUT: Self = Self(1 << 1);
    pub const UNDERLINE: Self = Self(1 << 2);
    pub const ITALIC: Self = Self(1 << 3);
    pub const ALL: Self =
        Self(Self::BOLD.0 | Self::STRIKEOUT.0 | Self::UNDERLINE.0 | Self::ITALIC.0);

    pub const fn from_bits(bits: u8) -> Result<Self, CellError> {
        if bits & !Self::ALL.0 == 0 {
            Ok(Self(bits))
        } else {
            Err(CellError::InvalidStyle(bits))
        }
    }

    pub const fn bits(self) -> u8 {
        self.0
    }

    pub const fn contains(self, flag: Self) -> bool {
        self.0 & flag.0 == flag.0
    }

    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    pub const fn without(self, other: Self) -> Self {
        Self(self.0 & !other.0)
    }
}

impl core::ops::BitOr for CellStyle {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self::Output {
        self.union(rhs)
    }
}

impl core::ops::BitOrAssign for CellStyle {
    fn bitor_assign(&mut self, rhs: Self) {
        *self = self.union(rhs);
    }
}

/// One decoded grid cell. Its two UTF-8 glyph fields are fixed and separate.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Cell {
    primary: [u8; GLYPH_UTF8_CAPACITY],
    primary_len: u8,
    upper: [u8; GLYPH_UTF8_CAPACITY],
    upper_len: u8,
    foreground: Color,
    background: Color,
    style: CellStyle,
}

impl Cell {
    pub const fn blank() -> Self {
        Self {
            primary: [0; GLYPH_UTF8_CAPACITY],
            primary_len: 0,
            upper: [0; GLYPH_UTF8_CAPACITY],
            upper_len: 0,
            foreground: Color::Default,
            background: Color::Default,
            style: CellStyle::NONE,
        }
    }

    pub fn new(
        primary: &str,
        foreground: Color,
        background: Color,
        style: CellStyle,
    ) -> Result<Self, CellError> {
        let mut cell = Self {
            foreground,
            background,
            style,
            ..Self::blank()
        };
        cell.set_primary(primary)?;
        Ok(cell)
    }

    pub fn with_upper(
        primary: &str,
        upper: &str,
        foreground: Color,
        background: Color,
        style: CellStyle,
    ) -> Result<Self, CellError> {
        let mut cell = Self::new(primary, foreground, background, style)?;
        cell.set_upper(upper)?;
        Ok(cell)
    }

    pub fn set_primary(&mut self, primary: &str) -> Result<(), CellError> {
        validate_glyph(GlyphField::Primary, primary)?;
        let bytes = primary.as_bytes();
        self.primary.fill(0);
        self.primary[..bytes.len()].copy_from_slice(bytes);
        self.primary_len = bytes.len() as u8;
        if bytes.is_empty() {
            self.upper.fill(0);
            self.upper_len = 0;
        }
        Ok(())
    }

    pub fn set_upper(&mut self, upper: &str) -> Result<(), CellError> {
        validate_glyph(GlyphField::Upper, upper)?;
        if self.primary_len == 0 && !upper.is_empty() {
            return Err(CellError::UpperWithoutPrimary);
        }
        let bytes = upper.as_bytes();
        self.upper.fill(0);
        self.upper[..bytes.len()].copy_from_slice(bytes);
        self.upper_len = bytes.len() as u8;
        Ok(())
    }

    pub fn primary(&self) -> &str {
        // `Cell` can only be constructed from `str` or the validating decoder.
        core::str::from_utf8(&self.primary[..usize::from(self.primary_len)])
            .expect("gridpaper Cell invariant: primary glyph is UTF-8")
    }

    pub fn upper(&self) -> Option<&str> {
        (self.upper_len != 0).then(|| {
            core::str::from_utf8(&self.upper[..usize::from(self.upper_len)])
                .expect("gridpaper Cell invariant: upper glyph is UTF-8")
        })
    }

    pub const fn foreground(&self) -> Color {
        self.foreground
    }

    pub const fn background(&self) -> Color {
        self.background
    }

    pub const fn style(&self) -> CellStyle {
        self.style
    }

    pub const fn set_foreground(&mut self, color: Color) {
        self.foreground = color;
    }

    pub const fn set_background(&mut self, color: Color) {
        self.background = color;
    }

    pub const fn set_style(&mut self, style: CellStyle) {
        self.style = style;
    }

    fn encode_into(&self, raw: &mut [u8]) {
        debug_assert_eq!(raw.len(), CELL_BYTES);
        raw.fill(0);
        raw[PRIMARY_LENGTH_OFFSET] = self.primary_len;
        raw[UPPER_LENGTH_OFFSET] = self.upper_len;
        raw[FOREGROUND_OFFSET] = self.foreground as u8;
        raw[BACKGROUND_OFFSET] = self.background as u8;
        raw[STYLE_OFFSET] = self.style.bits();
        raw[PRIMARY_OFFSET..PRIMARY_OFFSET + usize::from(self.primary_len)]
            .copy_from_slice(&self.primary[..usize::from(self.primary_len)]);
        raw[UPPER_OFFSET..UPPER_OFFSET + usize::from(self.upper_len)]
            .copy_from_slice(&self.upper[..usize::from(self.upper_len)]);
    }

    fn decode(raw: &[u8]) -> Result<Self, CellError> {
        debug_assert_eq!(raw.len(), CELL_BYTES);
        let primary_len = usize::from(raw[PRIMARY_LENGTH_OFFSET]);
        let upper_len = usize::from(raw[UPPER_LENGTH_OFFSET]);
        let primary = decode_glyph(
            GlyphField::Primary,
            primary_len,
            &raw[PRIMARY_OFFSET..PRIMARY_OFFSET + GLYPH_UTF8_CAPACITY],
        )?;
        let upper = decode_glyph(
            GlyphField::Upper,
            upper_len,
            &raw[UPPER_OFFSET..UPPER_OFFSET + GLYPH_UTF8_CAPACITY],
        )?;
        if primary_len == 0 && upper_len != 0 {
            return Err(CellError::UpperWithoutPrimary);
        }
        Ok(Self {
            primary,
            primary_len: primary_len as u8,
            upper,
            upper_len: upper_len as u8,
            foreground: Color::try_from(raw[FOREGROUND_OFFSET])?,
            background: Color::try_from(raw[BACKGROUND_OFFSET])?,
            style: CellStyle::from_bits(raw[STYLE_OFFSET])?,
        })
    }
}

fn validate_glyph(field: GlyphField, glyph: &str) -> Result<(), CellError> {
    let length = glyph.len();
    if length > GLYPH_UTF8_CAPACITY {
        return Err(CellError::GlyphTooLong {
            field,
            length,
            capacity: GLYPH_UTF8_CAPACITY,
        });
    }
    let characters = glyph.chars().count();
    if characters > 1 {
        return Err(CellError::MultipleGlyphs { field, characters });
    }
    Ok(())
}

fn decode_glyph(
    field: GlyphField,
    length: usize,
    encoded: &[u8],
) -> Result<[u8; GLYPH_UTF8_CAPACITY], CellError> {
    if length > GLYPH_UTF8_CAPACITY {
        return Err(CellError::InvalidGlyphLength {
            field,
            length: length as u8,
        });
    }
    let encoded = &encoded[..length];
    let glyph = core::str::from_utf8(encoded).map_err(|_| CellError::InvalidUtf8(field))?;
    validate_glyph(field, glyph)?;
    let mut stored = [0; GLYPH_UTF8_CAPACITY];
    stored[..length].copy_from_slice(encoded);
    Ok(stored)
}

impl Default for Cell {
    fn default() -> Self {
        Self::blank()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CellError {
    OutOfBounds {
        column: usize,
        row: usize,
    },
    GlyphTooLong {
        field: GlyphField,
        length: usize,
        capacity: usize,
    },
    MultipleGlyphs {
        field: GlyphField,
        characters: usize,
    },
    InvalidGlyphLength {
        field: GlyphField,
        length: u8,
    },
    InvalidUtf8(GlyphField),
    UpperWithoutPrimary,
    InvalidColor(u8),
    InvalidStyle(u8),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GlyphField {
    Primary,
    Upper,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AnimationTargetError {
    TransparentForeground,
}

impl fmt::Display for AnimationTargetError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TransparentForeground => {
                formatter.write_str("transparent is not an active text animation selector")
            }
        }
    }
}

impl fmt::Display for CellError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::OutOfBounds { column, row } => {
                write!(
                    formatter,
                    "cell ({column}, {row}) is outside {COLUMNS}x{ROWS}"
                )
            }
            Self::GlyphTooLong {
                field,
                length,
                capacity,
            } => {
                write!(
                    formatter,
                    "{field} glyph uses {length} UTF-8 bytes; capacity is {capacity}"
                )
            }
            Self::MultipleGlyphs { field, characters } => {
                write!(
                    formatter,
                    "{field} field contains {characters} characters; expected at most one"
                )
            }
            Self::InvalidGlyphLength { field, length } => {
                write!(
                    formatter,
                    "raw {field} glyph has invalid UTF-8 length {length}"
                )
            }
            Self::InvalidUtf8(field) => write!(formatter, "raw {field} glyph is not valid UTF-8"),
            Self::UpperWithoutPrimary => {
                formatter.write_str("upper glyph requires a primary glyph")
            }
            Self::InvalidColor(color) => write!(formatter, "raw cell has invalid color {color}"),
            Self::InvalidStyle(style) => {
                write!(formatter, "raw cell has invalid style bits 0x{style:02x}")
            }
        }
    }
}

impl fmt::Display for GlyphField {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Primary => "primary",
            Self::Upper => "upper",
        })
    }
}

/// Determines when a dirty edit buffer becomes the published snapshot.
///
/// A zero threshold disables that part of a cadence. Time-based publication is
/// evaluated by [`GridPaper::tick`] and when an [`EditSession`] is finished.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum SnapshotCadence {
    #[default]
    Manual,
    EveryEdits(u32),
    EveryMillis(u64),
    EveryEditsOrMillis {
        edits: u32,
        millis: u64,
    },
}

impl SnapshotCadence {
    const fn is_due(self, edit_batches: u32, elapsed_ms: u64) -> bool {
        match self {
            Self::Manual => false,
            Self::EveryEdits(edits) => edits != 0 && edit_batches >= edits,
            Self::EveryMillis(millis) => millis != 0 && elapsed_ms >= millis,
            Self::EveryEditsOrMillis { edits, millis } => {
                (edits != 0 && edit_batches >= edits) || (millis != 0 && elapsed_ms >= millis)
            }
        }
    }
}

/// Chooses the work performed after the active buffers are exchanged.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum PublishMode {
    /// Copy the new snapshot into the next edit buffer. This keeps incremental
    /// edits intuitive at the cost of copying [`PAGE_BYTES`] bytes per publish.
    #[default]
    PreserveIncrementalEdits,
    /// Exchange buffer indices in O(1). A producer using this mode must rewrite
    /// a complete page before every publish.
    SwapOnly,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct GridPaperConfig {
    pub cadence: SnapshotCadence,
    pub publish_mode: PublishMode,
    /// Monotonic timestamp used as the origin for time-based cadence.
    pub initial_time_ms: u64,
}

/// Publication result returned by edit completion, ticks, and manual publish.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PublishEvent {
    Unchanged { generation: u64 },
    Deferred { generation: u64 },
    Published { generation: u64 },
}

impl PublishEvent {
    pub const fn generation(self) -> u64 {
        match self {
            Self::Unchanged { generation }
            | Self::Deferred { generation }
            | Self::Published { generation } => generation,
        }
    }

    pub const fn was_published(self) -> bool {
        matches!(self, Self::Published { .. })
    }
}

/// Two fixed page buffers plus small snapshot metadata. No heap is used.
pub struct GridPaper {
    buffers: [[u8; PAGE_BYTES]; 2],
    published_index: usize,
    generation: u64,
    scale_percent: u16,
    text_color_animations: [Option<ColorAnimation>; TEXT_COLOR_ANIMATION_SLOTS],
    animation_generation: u64,
    edit_batches: u32,
    last_publish_ms: u64,
    dirty: bool,
    config: GridPaperConfig,
}

impl GridPaper {
    pub const fn new(config: GridPaperConfig) -> Self {
        Self {
            buffers: [[0; PAGE_BYTES]; 2],
            published_index: 0,
            generation: 0,
            scale_percent: DEFAULT_SCALE_PERCENT,
            text_color_animations: [None; TEXT_COLOR_ANIMATION_SLOTS],
            animation_generation: 0,
            edit_batches: 0,
            last_publish_ms: config.initial_time_ms,
            dirty: false,
            config,
        }
    }

    pub const fn generation(&self) -> u64 {
        self.generation
    }

    /// Returns the page-wide font/render scale, where 100 means 100%.
    pub const fn scale_percent(&self) -> u16 {
        self.scale_percent
    }

    /// Sets the page-wide font/render scale, where 100 means 100%.
    pub fn set_scale_percent(&mut self, scale_percent: u16) {
        self.scale_percent = scale_percent;
    }

    /// Assign a CSS-like paint program to every active text cell using this
    /// foreground color. The page bytes and resident font geometry are unchanged.
    pub fn set_text_color_animation(
        &mut self,
        selector: Color,
        animation: Option<ColorAnimation>,
    ) -> Result<(), AnimationTargetError> {
        if selector == Color::Transparent {
            return Err(AnimationTargetError::TransparentForeground);
        }
        let slot = selector as usize;
        if self.text_color_animations[slot] != animation {
            self.text_color_animations[slot] = animation;
            self.animation_generation = self.animation_generation.wrapping_add(1).max(1);
        }
        Ok(())
    }

    pub const fn text_color_animation(&self, selector: Color) -> Option<ColorAnimation> {
        match selector {
            Color::Transparent => None,
            _ => self.text_color_animations[selector as usize],
        }
    }

    pub fn clear_text_color_animations(&mut self) {
        if self.text_color_animations.iter().any(Option::is_some) {
            self.text_color_animations = [None; TEXT_COLOR_ANIMATION_SLOTS];
            self.animation_generation = self.animation_generation.wrapping_add(1).max(1);
        }
    }

    pub const fn animation_generation(&self) -> u64 {
        self.animation_generation
    }

    pub const fn is_dirty(&self) -> bool {
        self.dirty
    }

    pub const fn config(&self) -> GridPaperConfig {
        self.config
    }

    pub fn set_cadence(&mut self, cadence: SnapshotCadence) {
        self.config.cadence = cadence;
    }

    pub fn set_publish_mode(&mut self, publish_mode: PublishMode) {
        self.config.publish_mode = publish_mode;
    }

    /// Starts a write transaction against the non-published buffer.
    ///
    /// Finishing or dropping the session counts as one edit batch if any mutable
    /// accessor was used. Explicit [`EditSession::finish`] reports whether that
    /// batch caused a configured snapshot publication.
    pub fn edit(&mut self, now_ms: u64) -> EditSession<'_> {
        EditSession {
            page: Some(self),
            now_ms,
            dirty: false,
        }
    }

    /// Borrows the currently published, immutable buffer and generation.
    pub fn snapshot(&self) -> Snapshot<'_> {
        Snapshot {
            raw: &self.buffers[self.published_index],
            generation: self.generation,
            scale_percent: self.scale_percent,
            text_color_animations: &self.text_color_animations,
            animation_generation: self.animation_generation,
        }
    }

    /// Checks a time/edit cadence and publishes if it is due.
    pub fn tick(&mut self, now_ms: u64) -> PublishEvent {
        if !self.dirty {
            return PublishEvent::Unchanged {
                generation: self.generation,
            };
        }
        if self.snapshot_due(now_ms) {
            self.publish(now_ms)
        } else {
            PublishEvent::Deferred {
                generation: self.generation,
            }
        }
    }

    /// Publishes a dirty buffer immediately, independent of the cadence.
    pub fn publish(&mut self, now_ms: u64) -> PublishEvent {
        if !self.dirty {
            return PublishEvent::Unchanged {
                generation: self.generation,
            };
        }

        self.published_index ^= 1;
        self.generation = self.generation.wrapping_add(1);
        self.edit_batches = 0;
        self.last_publish_ms = now_ms;
        self.dirty = false;

        if self.config.publish_mode == PublishMode::PreserveIncrementalEdits {
            self.copy_published_to_edit();
        }

        PublishEvent::Published {
            generation: self.generation,
        }
    }

    fn edit_index(&self) -> usize {
        self.published_index ^ 1
    }

    fn finish_edit(&mut self, dirty: bool, now_ms: u64) -> PublishEvent {
        if !dirty {
            return PublishEvent::Unchanged {
                generation: self.generation,
            };
        }

        self.dirty = true;
        self.edit_batches = self.edit_batches.saturating_add(1);
        self.tick(now_ms)
    }

    fn snapshot_due(&self, now_ms: u64) -> bool {
        self.config.cadence.is_due(
            self.edit_batches,
            now_ms.saturating_sub(self.last_publish_ms),
        )
    }

    fn copy_published_to_edit(&mut self) {
        if self.published_index == 0 {
            let (published, edit) = self.buffers.split_at_mut(1);
            edit[0].copy_from_slice(&published[0]);
        } else {
            let (edit, published) = self.buffers.split_at_mut(1);
            edit[0].copy_from_slice(&published[0]);
        }
    }
}

impl Default for GridPaper {
    fn default() -> Self {
        Self::new(GridPaperConfig::default())
    }
}

/// A stable read view of one published generation.
pub struct Snapshot<'a> {
    raw: &'a [u8; PAGE_BYTES],
    generation: u64,
    scale_percent: u16,
    text_color_animations: &'a [Option<ColorAnimation>; TEXT_COLOR_ANIMATION_SLOTS],
    animation_generation: u64,
}

impl<'a> Snapshot<'a> {
    pub const fn generation(&self) -> u64 {
        self.generation
    }

    /// Returns the page-wide font/render scale captured by this view.
    pub const fn scale_percent(&self) -> u16 {
        self.scale_percent
    }

    pub const fn text_color_animations(
        &self,
    ) -> &[Option<ColorAnimation>; TEXT_COLOR_ANIMATION_SLOTS] {
        self.text_color_animations
    }

    pub const fn animation_generation(&self) -> u64 {
        self.animation_generation
    }

    /// Typed cell access (access level 1).
    pub fn cell(&self, column: usize, row: usize) -> Result<Cell, CellError> {
        Cell::decode(self.cell_bytes(column, row)?)
    }

    /// Encoded cell access, useful for small zero-copy operations.
    pub fn cell_bytes(&self, column: usize, row: usize) -> Result<&[u8], CellError> {
        let offset = cell_offset(column, row)?;
        Ok(&self.raw[offset..offset + CELL_BYTES])
    }

    /// Row-level raw access (access level 2).
    pub fn row_bytes(&self, row: usize) -> Result<&[u8], CellError> {
        let offset = row_offset(row)?;
        Ok(&self.raw[offset..offset + ROW_BYTES])
    }

    /// Whole-page raw access (access level 3).
    pub const fn raw(&self) -> &[u8; PAGE_BYTES] {
        self.raw
    }
}

/// A mutable transaction over the non-published page buffer.
pub struct EditSession<'a> {
    page: Option<&'a mut GridPaper>,
    now_ms: u64,
    dirty: bool,
}

impl EditSession<'_> {
    /// Typed cell access (access level 1).
    pub fn cell(&self, column: usize, row: usize) -> Result<Cell, CellError> {
        Cell::decode(self.cell_bytes(column, row)?)
    }

    pub fn set_cell(&mut self, column: usize, row: usize, cell: Cell) -> Result<(), CellError> {
        cell.encode_into(self.cell_bytes_mut(column, row)?);
        Ok(())
    }

    pub fn cell_bytes(&self, column: usize, row: usize) -> Result<&[u8], CellError> {
        let offset = cell_offset(column, row)?;
        let raw = self.edit_raw();
        Ok(&raw[offset..offset + CELL_BYTES])
    }

    pub fn cell_bytes_mut(&mut self, column: usize, row: usize) -> Result<&mut [u8], CellError> {
        let offset = cell_offset(column, row)?;
        self.dirty = true;
        let raw = self.edit_raw_mut();
        Ok(&mut raw[offset..offset + CELL_BYTES])
    }

    /// Row-level raw access (access level 2).
    pub fn row_bytes(&self, row: usize) -> Result<&[u8], CellError> {
        let offset = row_offset(row)?;
        let raw = self.edit_raw();
        Ok(&raw[offset..offset + ROW_BYTES])
    }

    pub fn row_bytes_mut(&mut self, row: usize) -> Result<&mut [u8], CellError> {
        let offset = row_offset(row)?;
        self.dirty = true;
        let raw = self.edit_raw_mut();
        Ok(&mut raw[offset..offset + ROW_BYTES])
    }

    /// Whole-page raw access (access level 3).
    pub fn raw(&self) -> &[u8; PAGE_BYTES] {
        self.edit_raw()
    }

    pub fn raw_mut(&mut self) -> &mut [u8; PAGE_BYTES] {
        self.dirty = true;
        self.edit_raw_mut()
    }

    /// Completes this batch and returns its publication outcome.
    pub fn finish(mut self) -> PublishEvent {
        self.finish_inner()
    }

    fn edit_raw(&self) -> &[u8; PAGE_BYTES] {
        let page = self
            .page
            .as_deref()
            .expect("gridpaper EditSession invariant: page is present");
        &page.buffers[page.edit_index()]
    }

    fn edit_raw_mut(&mut self) -> &mut [u8; PAGE_BYTES] {
        let page = self
            .page
            .as_deref_mut()
            .expect("gridpaper EditSession invariant: page is present");
        let edit_index = page.edit_index();
        &mut page.buffers[edit_index]
    }

    fn finish_inner(&mut self) -> PublishEvent {
        let page = self
            .page
            .take()
            .expect("gridpaper EditSession can only finish once");
        page.finish_edit(self.dirty, self.now_ms)
    }
}

impl Drop for EditSession<'_> {
    fn drop(&mut self) {
        if self.page.is_some() {
            let _ = self.finish_inner();
        }
    }
}

pub const fn row_height_mm(row: usize) -> Option<usize> {
    if row < ROWS { Some(CELL_EDGE_MM) } else { None }
}

fn cell_offset(column: usize, row: usize) -> Result<usize, CellError> {
    if column >= COLUMNS || row >= ROWS {
        return Err(CellError::OutOfBounds { column, row });
    }
    Ok((row * COLUMNS + column) * CELL_BYTES)
}

fn row_offset(row: usize) -> Result<usize, CellError> {
    if row >= ROWS {
        return Err(CellError::OutOfBounds { column: 0, row });
    }
    Ok(row * ROW_BYTES)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn styled_cell(text: &str) -> Cell {
        Cell::new(
            text,
            Color::BrightBlue,
            Color::White,
            CellStyle::BOLD | CellStyle::UNDERLINE | CellStyle::ITALIC,
        )
        .unwrap()
    }

    #[test]
    fn a4_geometry_and_static_sizes_are_exact() {
        assert_eq!(COLUMNS, 37);
        assert_eq!(FULL_ROWS, 53);
        assert_eq!(ROWS, 53);
        assert_eq!(CELL_COUNT, 1_961);
        assert_eq!(GRID_WIDTH_MM, 185);
        assert_eq!(GRID_HEIGHT_MM, 265);
        assert_eq!(GRID_HORIZONTAL_MARGIN_MM, 12.5);
        assert_eq!(GRID_VERTICAL_MARGIN_MM, 16.0);
        assert_eq!(FINAL_ROW_HEIGHT_MM, 0);
        assert_eq!(CELL_BYTES, 13);
        assert_eq!(PAGE_BYTES, 25_493);
        assert_eq!(DOUBLE_BUFFER_BYTES, 50_986);
        assert_eq!(row_height_mm(52), Some(5));
        assert_eq!(row_height_mm(53), None);
    }

    #[test]
    fn global_scale_defaults_to_100_percent_and_has_a_getter_setter() {
        let mut page = GridPaper::default();
        assert_eq!(page.scale_percent(), DEFAULT_SCALE_PERCENT);
        assert_eq!(page.snapshot().scale_percent(), DEFAULT_SCALE_PERCENT);

        page.set_scale_percent(125);
        assert_eq!(page.scale_percent(), 125);
        assert_eq!(page.snapshot().scale_percent(), 125);
    }

    #[test]
    fn text_color_animation_is_static_metadata_not_page_bytes() {
        let mut page = GridPaper::default();
        let before = *page.snapshot().raw();
        let animation = ColorAnimation::transition(
            Rgba8::new(255, 0, 0, 255),
            Rgba8::new(0, 0, 255, 255),
            ColorChannels::RGB,
            2_000,
            AnimationTiming::EaseInOutSine,
            AnimationIteration::Alternate,
        )
        .unwrap();

        page.set_text_color_animation(Color::BrightBlue, Some(animation))
            .unwrap();
        assert_eq!(
            page.text_color_animation(Color::BrightBlue),
            Some(animation)
        );
        assert_eq!(page.animation_generation(), 1);
        assert_eq!(page.snapshot().animation_generation(), 1);
        assert_eq!(*page.snapshot().raw(), before);
        assert_eq!(
            page.set_text_color_animation(Color::Transparent, Some(animation)),
            Err(AnimationTargetError::TransparentForeground)
        );
    }

    #[test]
    fn typed_primary_and_upper_glyphs_round_trip_through_snapshot() {
        let mut page = GridPaper::default();
        let expected = Cell::with_upper(
            "x",
            "²",
            Color::BrightBlue,
            Color::White,
            CellStyle::BOLD | CellStyle::UNDERLINE | CellStyle::ITALIC,
        )
        .unwrap();

        let event = {
            let mut edit = page.edit(5);
            edit.set_cell(20, 29, expected).unwrap();
            edit.finish()
        };
        assert_eq!(event, PublishEvent::Deferred { generation: 0 });
        assert_eq!(page.publish(5), PublishEvent::Published { generation: 1 });

        let actual = page.snapshot().cell(20, 29).unwrap();
        assert_eq!(actual, expected);
        assert_eq!(actual.primary(), "x");
        assert_eq!(actual.upper(), Some("²"));
        assert!(actual.style().contains(CellStyle::BOLD));
        assert!(actual.style().contains(CellStyle::UNDERLINE));
        assert!(!actual.style().contains(CellStyle::STRIKEOUT));
    }

    #[test]
    fn invalid_raw_bytes_are_rejected_by_typed_access() {
        let mut page = GridPaper::default();
        {
            let mut edit = page.edit(0);
            let raw = edit.cell_bytes_mut(0, 0).unwrap();
            raw[PRIMARY_LENGTH_OFFSET] = 1;
            raw[FOREGROUND_OFFSET] = Color::Red as u8;
            raw[BACKGROUND_OFFSET] = Color::Black as u8;
            raw[STYLE_OFFSET] = 0;
            raw[PRIMARY_OFFSET] = 0xff;
            edit.finish();
        }
        page.publish(0);
        assert_eq!(
            page.snapshot().cell(0, 0),
            Err(CellError::InvalidUtf8(GlyphField::Primary))
        );
    }

    #[test]
    fn edit_count_cadence_publishes_on_configured_batch() {
        let mut page = GridPaper::new(GridPaperConfig {
            cadence: SnapshotCadence::EveryEdits(2),
            ..GridPaperConfig::default()
        });

        let first = {
            let mut edit = page.edit(1);
            edit.set_cell(0, 0, styled_cell("1")).unwrap();
            edit.finish()
        };
        assert_eq!(first, PublishEvent::Deferred { generation: 0 });

        let second = {
            let mut edit = page.edit(2);
            edit.set_cell(1, 0, styled_cell("2")).unwrap();
            edit.finish()
        };
        assert_eq!(second, PublishEvent::Published { generation: 1 });
        assert_eq!(page.snapshot().cell(0, 0).unwrap().primary(), "1");
        assert_eq!(page.snapshot().cell(1, 0).unwrap().primary(), "2");
    }

    #[test]
    fn millisecond_cadence_can_be_driven_by_tick() {
        let mut page = GridPaper::new(GridPaperConfig {
            cadence: SnapshotCadence::EveryMillis(16),
            initial_time_ms: 100,
            ..GridPaperConfig::default()
        });
        {
            let mut edit = page.edit(105);
            edit.set_cell(0, 0, styled_cell("t")).unwrap();
        }
        assert_eq!(page.tick(115), PublishEvent::Deferred { generation: 0 });
        assert_eq!(page.tick(116), PublishEvent::Published { generation: 1 });
    }

    #[test]
    fn preserve_mode_keeps_incremental_edits_between_publications() {
        let mut page = GridPaper::default();
        {
            let mut edit = page.edit(0);
            edit.set_cell(0, 0, styled_cell("k")).unwrap();
        }
        page.publish(0);
        {
            let mut edit = page.edit(1);
            edit.set_cell(1, 0, styled_cell("n")).unwrap();
        }
        page.publish(1);

        assert_eq!(page.snapshot().cell(0, 0).unwrap().primary(), "k");
        assert_eq!(page.snapshot().cell(1, 0).unwrap().primary(), "n");
    }

    #[test]
    fn row_and_page_access_are_zero_copy_views_of_the_same_bytes() {
        let mut page = GridPaper::default();
        let cell = styled_cell("r");
        {
            let mut edit = page.edit(0);
            edit.set_cell(3, 2, cell).unwrap();
            let cell_start = 3 * CELL_BYTES;
            assert_eq!(
                &edit.row_bytes(2).unwrap()[cell_start..cell_start + CELL_BYTES],
                edit.cell_bytes(3, 2).unwrap()
            );
            edit.finish();
        }
        page.publish(0);

        let snapshot = page.snapshot();
        let absolute = (2 * COLUMNS + 3) * CELL_BYTES;
        assert_eq!(
            &snapshot.raw()[absolute..absolute + CELL_BYTES],
            snapshot.cell_bytes(3, 2).unwrap()
        );
    }
}
