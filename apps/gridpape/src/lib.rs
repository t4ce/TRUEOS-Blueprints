#![no_std]
extern crate alloc;
use alloc::vec::Vec;

pub const COLUMNS: u32 = 39;
pub const ROWS: u32 = 55;
pub const INDEX_COUNT: u32 = (COLUMNS + ROWS + 2) * 2;

/// Optional `scale N` / `scale Nx` directive, applied before the first frame.
/// Empty/comment-only scripts keep 1x. Reject malformed scales and unknown commands.
pub fn script_scale(script: &str) -> Result<u32, &'static str> {
    let mut scale = 1;
    for line in script.lines() {
        let line = line.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        let mut words = line.split_whitespace();
        if words.next() != Some("scale") {
            return Err("expected scale 1x..8x");
        }
        let value = words.next().ok_or("missing scale")?;
        let value = value.strip_suffix('x').unwrap_or(value);
        scale = value.parse().map_err(|_| "invalid scale")?;
        if !(1..=8).contains(&scale) || words.next().is_some() {
            return Err("expected scale 1x..8x");
        }
    }
    Ok(scale)
}

/// Match Gridpaper's centered 195 x 275 mm grid on a full 210 x 297 mm page.
/// Keep physical sizing on resize: extra area is white, smaller windows clip.
/// Each independent segment is two vertices and two indices, never adjacency.
pub fn geometry(page: (u32, u32), viewport: (u32, u32), scale: u32) -> (Vec<u8>, Vec<u8>) {
    assert!((1..=8).contains(&scale));
    let columns = COLUMNS / scale;
    let rows = ROWS / scale;
    let step = 5.0 * scale as f32;
    let left = (210.0 - columns as f32 * step) / 2.0;
    let top = (297.0 - rows as f32 * step) / 2.0;
    let right = left + columns as f32 * step;
    let bottom = top + rows as f32 * step;
    let mut vertices = Vec::with_capacity(INDEX_COUNT as usize * 12);
    let mut indices = Vec::with_capacity(INDEX_COUNT as usize * 4);
    let mut vertex = |x_mm: f32, y_mm: f32| {
        // Snap to pixel centers for crisp single-sample one-pixel lines.
        let x = (x_mm * page.0 as f32 / 210.0) as u32 as f32 + 0.5;
        let y = (y_mm * page.1 as f32 / 297.0) as u32 as f32 + 0.5;
        for value in [
            2.0 * x / viewport.0 as f32 - 1.0,
            1.0 - 2.0 * y / viewport.1 as f32,
            0.0,
        ] {
            vertices.extend_from_slice(&value.to_le_bytes());
        }
        indices.extend_from_slice(&((indices.len() / 4) as u32).to_le_bytes());
    };
    for column in 0..=columns {
        let x = left + column as f32 * step;
        vertex(x, top);
        vertex(x, bottom);
    }
    for row in 0..=rows {
        let y = top + row as f32 * step;
        vertex(left, y);
        vertex(right, y);
    }
    (vertices, indices)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn scales_parse_and_reduce_line_counts() {
        assert_eq!(script_scale("# default\n"), Ok(1));
        for scale in 1..=8 {
            let script = alloc::format!("scale {scale}x\n");
            assert_eq!(script_scale(&script), Ok(scale));
            assert_eq!(script_scale(&alloc::format!("scale {scale}")), Ok(scale));
            let (v, i) = geometry((840, 1188), (840, 1188), scale);
            let count = ((COLUMNS / scale + ROWS / scale + 2) * 2) as usize;
            assert_eq!(v.len(), count * 12);
            assert_eq!(i.len(), count * 4);
            let x = |n: usize| f32::from_le_bytes(v[n * 12..n * 12 + 4].try_into().unwrap());
            assert!(((x(2) - x(0)) * 420.0 - 20.0 * scale as f32).abs() < 0.001);
        }
        for script in [
            "scale 0",
            "scale 9x",
            "scale -1",
            "scale",
            "scale 2 extra",
            "zoom 2",
        ] {
            assert!(script_scale(script).is_err());
        }
    }
    #[test]
    fn centered_five_mm_grid_has_only_independent_line_pairs() {
        let (vertices, indices) = geometry((840, 1188), (840, 1188), 1);
        assert_eq!(INDEX_COUNT, 192);
        assert_eq!(vertices.len(), 192 * 12);
        assert_eq!(indices.len(), 192 * 4);
        let pixel = |n: usize, axis: usize| {
            let start = n * 12 + axis * 4;
            let clip = f32::from_le_bytes(vertices[start..start + 4].try_into().unwrap());
            if axis == 0 {
                (clip + 1.0) * 420.0
            } else {
                (1.0 - clip) * 594.0
            }
        };
        assert!((pixel(0, 0) - 30.5).abs() < 0.001);
        assert!((pixel(0, 1) - 44.5).abs() < 0.001);
        for column in 0..39 {
            assert!((pixel((column + 1) * 2, 0) - pixel(column * 2, 0) - 20.0).abs() < 0.001);
        }
        for (n, bytes) in indices.chunks_exact(4).enumerate() {
            assert_eq!(u32::from_le_bytes(bytes.try_into().unwrap()), n as u32);
        }
    }
}

/// Fixed document storage. Colour is reserved per cell for future editing;
/// the current raster transport uses black on white.
#[derive(Clone, Copy, Default)]
pub struct Cell {
    pub character: Option<char>,
    pub foreground: u32,
}
pub struct Editor {
    pub cells: Vec<Cell>,
    pub cursor: usize,
    pub columns: usize,
    pub rows: usize,
}
impl Editor {
    pub fn new(scale: u32) -> Self {
        let columns = (COLUMNS / scale) as usize;
        let rows = (ROWS / scale) as usize;
        Self {
            cells: alloc::vec![Cell::default(); columns * rows],
            cursor: 0,
            columns,
            rows,
        }
    }
    pub fn advance(&mut self) {
        self.cursor = (self.cursor + 1).min(self.cells.len() - 1);
    }
    pub fn type_char(&mut self, ch: char) {
        if ch == ' ' {
            self.advance();
        } else if !ch.is_control() {
            self.cells[self.cursor].character = Some(ch);
            self.advance();
        }
    }
    pub fn key(&mut self, key: u16) {
        use trueos::input::*;
        match key {
            KEYBOARD_KEY_BACKSPACE => {
                if self.cursor > 0 {
                    self.cursor -= 1;
                    self.cells[self.cursor].character = None;
                }
            }
            KEYBOARD_KEY_DELETE => self.cells[self.cursor].character = None,
            KEYBOARD_KEY_SPACE => self.advance(),
            KEYBOARD_KEY_ARROW_LEFT => self.cursor = self.cursor.saturating_sub(1),
            KEYBOARD_KEY_ARROW_RIGHT => self.advance(),
            KEYBOARD_KEY_ARROW_UP => {
                if self.cursor >= self.columns {
                    self.cursor -= self.columns;
                }
            }
            KEYBOARD_KEY_ARROW_DOWN => {
                if self.cursor + self.columns < self.cells.len() {
                    self.cursor += self.columns;
                }
            }
            _ => {} // Enter deliberately has no action.
        }
    }
    /// Pixel cell bounds aligned with the independently rendered grid lines.
    pub fn bounds(&self, index: usize, page: (u32, u32), scale: u32) -> (u32, u32, u32, u32) {
        let step = 5.0 * scale as f32;
        let left = (210.0 - self.columns as f32 * step) / 2.0;
        let top = (297.0 - self.rows as f32 * step) / 2.0;
        let x = (left + (index % self.columns) as f32 * step) * page.0 as f32 / 210.0;
        let y = (top + (index / self.columns) as f32 * step) * page.1 as f32 / 297.0;
        let right = (left + (index % self.columns + 1) as f32 * step) * page.0 as f32 / 210.0;
        let bottom = (top + (index / self.columns + 1) as f32 * step) * page.1 as f32 / 297.0;
        (
            x as u32,
            y as u32,
            right as u32 - x as u32,
            bottom as u32 - y as u32,
        )
    }
    pub fn click(&mut self, x: i32, y: i32, page: (u32, u32), scale: u32) -> bool {
        if x < 0 || y < 0 {
            return false;
        }
        for index in 0..self.cells.len() {
            let (left, top, w, h) = self.bounds(index, page, scale);
            if x as u32 >= left && (x as u32) < left + w && y as u32 >= top && (y as u32) < top + h
            {
                self.cursor = index;
                return true;
            }
        }
        false
    }
}

#[cfg(test)]
mod editor_tests {
    use super::*;
    use trueos::input::*;
    #[test]
    fn fixed_budget_and_editing() {
        for scale in 1..=8 {
            let mut e = Editor::new(scale);
            assert_eq!(e.cells.len(), (COLUMNS / scale * (ROWS / scale)) as usize);
            e.type_char('a');
            e.type_char(' ');
            e.type_char('b');
            assert_eq!(e.cursor, 3);
            e.key(KEYBOARD_KEY_ENTER);
            assert_eq!(e.cursor, 3);
            e.key(KEYBOARD_KEY_BACKSPACE);
            assert_eq!(e.cells[2].character, None);
            assert_eq!(e.cursor, 2);
            assert_eq!(e.cells[0].character, Some('a'));
            e.key(KEYBOARD_KEY_ARROW_DOWN);
            assert_eq!(e.cursor, 2 + e.columns);
            e.key(KEYBOARD_KEY_ARROW_UP);
            assert_eq!(e.cursor, 2);
            e.key(KEYBOARD_KEY_ARROW_LEFT);
            assert_eq!(e.cursor, 1);
            e.key(KEYBOARD_KEY_ARROW_RIGHT);
            assert_eq!(e.cursor, 2);
            assert!(e.click(420, 594, (840, 1188), scale));
            let previous = e.cursor;
            assert!(!e.click(-1, -1, (840, 1188), scale));
            assert_eq!(e.cursor, previous);
        }
    }
}
