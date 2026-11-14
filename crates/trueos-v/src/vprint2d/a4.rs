//! App-owned DIN A4 page layout and PWG Raster encoding, at a fixed 300 dpi.
//!
//! These pixels represent document coordinates, independent of the screen's
//! dimensions, zoom, DPI, cursor, selection and window decorations.

extern crate alloc;
use super::{Error, JobId};
use alloc::{format, vec, vec::Vec};

pub const DPI: u32 = 300;
pub const WIDTH: u32 = 2480;
pub const HEIGHT: u32 = 3508;
pub const MAX_DOCUMENT_BYTES: usize = 32 * 1024 * 1024;
pub const MAX_PAGES: u32 = 64;
const HEADER_BYTES: usize = 1796;

pub fn mm_to_px(mm: f32) -> u32 {
    if !mm.is_finite() || mm <= 0.0 {
        return 0;
    }
    (mm as f64 * f64::from(DPI) / 25.4 + 0.5) as u32
}

pub struct Page {
    rgb: Vec<u8>,
}

impl Page {
    pub fn white() -> Result<Self, Error> {
        let count = WIDTH as usize * HEIGHT as usize * 3;
        let mut rgb = Vec::new();
        rgb.try_reserve_exact(count).map_err(|_| Error::Transport)?;
        rgb.resize(count, 255);
        Ok(Self { rgb })
    }
    pub fn rgb(&self) -> &[u8] {
        &self.rgb
    }
    pub fn rgb_mut(&mut self) -> &mut [u8] {
        &mut self.rgb
    }
    pub fn clear(&mut self) {
        self.rgb.fill(255);
    }

    fn pixel(&mut self, x: u32, y: u32, color: [u8; 3]) {
        if x < WIDTH && y < HEIGHT {
            let at = (y as usize * WIDTH as usize + x as usize) * 3;
            self.rgb[at..at + 3].copy_from_slice(&color);
        }
    }

    pub fn line(&mut self, x0: u32, y0: u32, x1: u32, y1: u32, color: [u8; 3]) {
        // Document producers only need in-page lines; reject huge coordinates
        // before the bounded Bresenham loop rather than spending time clipping.
        if x0 >= WIDTH || x1 >= WIDTH || y0 >= HEIGHT || y1 >= HEIGHT {
            return;
        }
        let (mut x, mut y) = (x0 as i32, y0 as i32);
        let (x1, y1) = (x1 as i32, y1 as i32);
        let dx = (x1 - x).abs();
        let dy = -(y1 - y).abs();
        let sx = if x < x1 { 1 } else { -1 };
        let sy = if y < y1 { 1 } else { -1 };
        let mut error = dx + dy;
        loop {
            self.pixel(x as u32, y as u32, color);
            if x == x1 && y == y1 {
                break;
            }
            let doubled = 2 * error;
            if doubled >= dy {
                error += dy;
                x += sx;
            }
            if doubled <= dx {
                error += dx;
                y += sy;
            }
        }
    }

    /// Place straight RGBA source pixels into a document rectangle. The entire
    /// source maps to that rectangle, even when the page clips its right edge.
    pub fn blit_rgba(
        &mut self,
        rgba: &[u8],
        sw: u32,
        sh: u32,
        x: u32,
        y: u32,
        width: u32,
        height: u32,
    ) -> Result<(), Error> {
        let required = (sw as usize)
            .checked_mul(sh as usize)
            .and_then(|n| n.checked_mul(4))
            .ok_or(Error::InvalidDocument)?;
        if sw == 0 || sh == 0 || width == 0 || height == 0 || rgba.len() != required {
            return Err(Error::InvalidDocument);
        }
        for dy in 0..height.min(HEIGHT.saturating_sub(y)) {
            let sy = (u64::from(dy) * u64::from(sh) / u64::from(height)) as usize;
            for dx in 0..width.min(WIDTH.saturating_sub(x)) {
                let sx = (u64::from(dx) * u64::from(sw) / u64::from(width)) as usize;
                let src = (sy * sw as usize + sx) * 4;
                let dst = ((y + dy) as usize * WIDTH as usize + (x + dx) as usize) * 3;
                let alpha = u32::from(rgba[src + 3]);
                for channel in 0..3 {
                    self.rgb[dst + channel] = ((u32::from(rgba[src + channel]) * alpha
                        + u32::from(self.rgb[dst + channel]) * (255 - alpha)
                        + 127)
                        / 255) as u8;
                }
            }
        }
        Ok(())
    }

    pub fn fit_image_centered(
        &mut self,
        rgba: &[u8],
        sw: u32,
        sh: u32,
        margin_mm: u32,
    ) -> Result<(), Error> {
        let margin = mm_to_px(margin_mm as f32);
        let available_w = WIDTH
            .checked_sub(margin.saturating_mul(2))
            .ok_or(Error::InvalidDocument)?;
        let available_h = HEIGHT
            .checked_sub(margin.saturating_mul(2))
            .ok_or(Error::InvalidDocument)?;
        if sw == 0 || sh == 0 || available_w == 0 || available_h == 0 {
            return Err(Error::InvalidDocument);
        }
        let (width, height) =
            if u64::from(sw) * u64::from(available_h) > u64::from(sh) * u64::from(available_w) {
                (
                    available_w,
                    (u64::from(sh) * u64::from(available_w) / u64::from(sw)).max(1) as u32,
                )
            } else {
                (
                    (u64::from(sw) * u64::from(available_h) / u64::from(sh)).max(1) as u32,
                    available_h,
                )
            };
        self.blit_rgba(
            rgba,
            sw,
            sh,
            (WIDTH - width) / 2,
            (HEIGHT - height) / 2,
            width,
            height,
        )
    }

    pub fn submit(&self) -> Result<JobId, Error> {
        let mut document = Document::new();
        document.push(self)?;
        document.submit()
    }
}

/// One IPP job can contain several A4 pages, e.g. a wrapped Edit document.
pub struct Document {
    bytes: Vec<u8>,
    pages: u32,
    headers: Vec<usize>,
}
impl Default for Document {
    fn default() -> Self {
        Self::new()
    }
}
impl Document {
    pub fn new() -> Self {
        Self {
            bytes: b"RaS2".to_vec(),
            pages: 0,
            headers: Vec::new(),
        }
    }
    pub fn pages(&self) -> u32 {
        self.pages
    }
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
    pub fn push(&mut self, page: &Page) -> Result<(), Error> {
        if self.pages == MAX_PAGES {
            return Err(Error::InvalidDocument);
        }
        // Append atomically: a size error must not leave a partial extra page.
        let original = self.bytes.len();
        let result = encode_page(&mut self.bytes, page.rgb());
        if result.is_err() {
            self.bytes.truncate(original);
        } else {
            self.pages += 1;
            self.headers.push(original);
            for header in &self.headers {
                self.bytes[header + 452..header + 456].copy_from_slice(&self.pages.to_be_bytes());
            }
        }
        result
    }
    pub fn submit(&self) -> Result<JobId, Error> {
        if self.pages == 0 {
            return Err(Error::InvalidDocument);
        }
        super::submit_a4_pwg(&self.bytes)
    }
}

fn page_header() -> [u8; HEADER_BYTES] {
    let mut header = [0u8; HEADER_BYTES];
    fn number(header: &mut [u8], offset: usize, n: u32) {
        header[offset..offset + 4].copy_from_slice(&n.to_be_bytes());
    }
    fn string(header: &mut [u8], offset: usize, value: &[u8]) {
        header[offset..offset + value.len()].copy_from_slice(value);
    }
    string(&mut header, 0, b"PwgRaster");
    string(&mut header, 192, b"text-and-graphics");
    for (offset, value) in [
        (276, DPI),
        (280, DPI),
        (352, 595),
        (356, 842),
        (372, WIDTH),
        (376, HEIGHT),
        (384, 8),
        (388, 24),
        (392, WIDTH * 3),
        (396, 0),
        (400, 19),
        (420, 3),
        (452, 1),
        (456, 1),
        (460, 1),
        (484, 4),
    ] {
        number(&mut header, offset, value);
    }
    string(&mut header, 1668, b"perceptual");
    string(&mut header, 1732, b"iso_a4_210x297mm");
    header
}

fn encode_page(output: &mut Vec<u8>, rgb: &[u8]) -> Result<(), Error> {
    if rgb.len() != WIDTH as usize * HEIGHT as usize * 3
        || output.len() + HEADER_BYTES > MAX_DOCUMENT_BYTES
    {
        return Err(Error::InvalidDocument);
    }
    output.extend_from_slice(&page_header());
    let stride = WIDTH as usize * 3;
    let mut row = 0usize;
    while row < HEIGHT as usize {
        let line = &rgb[row * stride..(row + 1) * stride];
        let mut repeat = 1usize;
        while repeat < 256
            && row + repeat < HEIGHT as usize
            && &rgb[(row + repeat) * stride..(row + repeat + 1) * stride] == line
        {
            repeat += 1;
        }
        output.push((repeat - 1) as u8);
        encode_line(output, line);
        if output.len() > MAX_DOCUMENT_BYTES {
            return Err(Error::InvalidDocument);
        }
        row += repeat;
    }
    Ok(())
}

fn encode_line(output: &mut Vec<u8>, rgb: &[u8]) {
    let pixels = rgb.len() / 3;
    let pixel = |i: usize| &rgb[i * 3..i * 3 + 3];
    let mut cursor = 0usize;
    while cursor < pixels {
        let mut repeat = 1usize;
        while repeat < 128 && cursor + repeat < pixels && pixel(cursor + repeat) == pixel(cursor) {
            repeat += 1;
        }
        if repeat >= 2 {
            output.push((repeat - 1) as u8);
            output.extend_from_slice(pixel(cursor));
            cursor += repeat;
        } else {
            let start = cursor;
            cursor += 1;
            while cursor < pixels
                && cursor - start < 128
                && (cursor + 1 == pixels || pixel(cursor) != pixel(cursor + 1))
            {
                cursor += 1;
            }
            let count = cursor - start;
            output.push(if count == 1 { 0 } else { (257 - count) as u8 });
            output.extend_from_slice(&rgb[start * 3..cursor * 3]);
        }
    }
}

/// Fixed 10-point monospaced text on A4 with 10 mm margins. Unicode scalars
/// use the same native Microfont glyph service as Paper. Tabs advance to4 cells;
/// long lines wrap; form feeds and overflow create another A4 page.
pub fn submit_text(text: &str) -> Result<JobId, Error> {
    let document = render_text_with(text, |ch| {
        let path = format!("vFile:microfont-grid/1/{:x}", ch as u32);
        crate::vfs_async::block_on(crate::vfs_async::read_file(path.as_bytes()))
            .map_err(|_| Error::Transport)
    })?;
    document.submit()
}

pub fn render_text_with(
    text: &str,
    mut glyph: impl FnMut(char) -> Result<Vec<u8>, Error>,
) -> Result<Document, Error> {
    const CELL_W: u32 = 25; // 6 pt at300dpi, rounded
    const CELL_H: u32 = 50; // 12 pt line advance
    const GLYPH_H: u32 = 42; // 10 pt
    let margin = mm_to_px(10.0);
    let columns = (WIDTH - 2 * margin) / CELL_W;
    let rows = (HEIGHT - 2 * margin) / CELL_H;
    let mut page = Page::white()?;
    let mut document = Document::new();
    let mut cache: Vec<(char, Vec<u8>)> = Vec::new();
    let (mut column, mut row) = (0u32, 0u32);
    let mut dirty = false;
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\u{000c}' {
            // An explicit page break at the bottom of a full page replaces
            // its deferred overflow; it must not eject another blank page.
            document.push(&page)?;
            page.clear();
            column = 0;
            row = 0;
            dirty = false;
            continue;
        }
        // Commit overflow before consuming the next document character,
        // including a blank line. A trailing newline alone does not add a page.
        if row >= rows {
            document.push(&page)?;
            page.clear();
            row = 0;
            dirty = false;
        }
        if ch == '\r' || ch == '\n' {
            if ch == '\r' && chars.peek() == Some(&'\n') {
                chars.next();
            }
            column = 0;
            row += 1;
            dirty = true;
            continue;
        }
        if ch == '\t' {
            column = (column / 4 + 1) * 4;
            dirty = true;
            continue;
        }
        if ch.is_control() {
            continue;
        }
        if column >= columns {
            column = 0;
            row += 1;
        }
        if row >= rows {
            document.push(&page)?;
            page.clear();
            row = 0;
        }
        if ch != ' ' {
            if !cache.iter().any(|(code, _)| *code == ch) {
                let bytes = glyph(ch)?;
                if cache.len() == 512 {
                    cache.remove(0);
                }
                cache.push((ch, bytes));
            }
            let bytes = &cache.iter().find(|(code, _)| *code == ch).unwrap().1;
            if bytes.len() < 8 {
                return Err(Error::InvalidDocument);
            }
            let w = u32::from_le_bytes(bytes[..4].try_into().unwrap());
            let h = u32::from_le_bytes(bytes[4..8].try_into().unwrap());
            page.blit_rgba(
                &bytes[8..],
                w,
                h,
                margin + column * CELL_W,
                margin + row * CELL_H,
                CELL_W,
                GLYPH_H,
            )?;
        }
        column += 1;
        dirty = true;
    }
    if dirty || document.pages() == 0 {
        document.push(&page)?;
    }
    Ok(document)
}

#[cfg(test)]
#[path = "a4_tests.rs"]
mod tests;
