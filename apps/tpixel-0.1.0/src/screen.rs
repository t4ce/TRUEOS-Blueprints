use std::io::{self, Write};

use crossterm::{
    cursor::MoveTo,
    queue,
    style::{
        Attribute, Color, Print, ResetColor, SetAttribute, SetBackgroundColor, SetForegroundColor,
    },
    terminal::{Clear, ClearType},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Style {
    pub fg: Color,
    pub bg: Color,
    pub bold: bool,
    pub dim: bool,
    pub underline: bool,
}

impl Style {
    pub const fn new(fg: Color, bg: Color) -> Self {
        Self {
            fg,
            bg,
            bold: false,
            dim: false,
            underline: false,
        }
    }

    pub const fn bold(mut self) -> Self {
        self.bold = true;
        self
    }

    pub const fn dim(mut self) -> Self {
        self.dim = true;
        self
    }

    pub const fn underline(mut self) -> Self {
        self.underline = true;
        self
    }
}

impl Default for Style {
    fn default() -> Self {
        Self::new(Color::Reset, Color::Reset)
    }
}

pub fn terminal_cell_width(ch: char) -> u16 {
    match ch {
        '🗃' | '🗂' | '◆' | '◇' | '★' | '☰' | '・' => 2,
        _ => 1,
    }
}

pub fn text_cell_width(text: &str) -> usize {
    text.chars()
        .map(|ch| terminal_cell_width(ch) as usize)
        .sum()
}

pub fn clip_text_cells(text: &str, maximum: usize) -> String {
    if maximum == 0 {
        return String::new();
    }
    if text_cell_width(text) <= maximum {
        return text.to_owned();
    }

    let ellipsis = '…';
    let ellipsis_width = terminal_cell_width(ellipsis) as usize;
    let target = maximum.saturating_sub(ellipsis_width);
    let mut result = String::new();
    let mut width = 0usize;
    for ch in text.chars() {
        let next = terminal_cell_width(ch) as usize;
        if width + next > target {
            break;
        }
        result.push(ch);
        width += next;
    }
    if maximum >= ellipsis_width {
        result.push(ellipsis);
    }
    result
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cell {
    pub ch: char,
    pub style: Style,
    // 2 = lead cell of a wide glyph, 1 = ordinary glyph, 0 = continuation.
    width: u8,
}

impl Default for Cell {
    fn default() -> Self {
        Self {
            ch: ' ',
            style: Style::default(),
            width: 1,
        }
    }
}

impl Cell {
    fn wide(ch: char, style: Style) -> Self {
        Self {
            ch,
            style,
            width: 2,
        }
    }

    fn continuation(style: Style) -> Self {
        Self {
            ch: ' ',
            style,
            width: 0,
        }
    }

    fn is_continuation(self) -> bool {
        self.width == 0
    }

    fn is_wide(self) -> bool {
        self.width == 2
    }
}

#[derive(Clone, Debug)]
pub struct Frame {
    width: u16,
    height: u16,
    cells: Vec<Cell>,
}

impl Frame {
    pub fn new(width: u16, height: u16) -> Self {
        Self {
            width,
            height,
            cells: vec![Cell::default(); width as usize * height as usize],
        }
    }

    pub fn width(&self) -> u16 {
        self.width
    }

    pub fn height(&self) -> u16 {
        self.height
    }

    fn index(&self, x: u16, y: u16) -> usize {
        y as usize * self.width as usize + x as usize
    }

    fn erase_occupant(&mut self, x: u16, y: u16) {
        if x >= self.width || y >= self.height {
            return;
        }
        let index = self.index(x, y);
        let cell = self.cells[index];
        if cell.is_continuation() {
            if x > 0 {
                let lead_index = self.index(x - 1, y);
                self.cells[lead_index] = Cell::default();
            }
            self.cells[index] = Cell::default();
        } else if cell.is_wide() {
            self.cells[index] = Cell::default();
            if x + 1 < self.width {
                let continuation = self.index(x + 1, y);
                self.cells[continuation] = Cell::default();
            }
        }
    }

    pub fn put(&mut self, x: u16, y: u16, ch: char, style: Style) {
        if x >= self.width || y >= self.height {
            return;
        }
        self.erase_occupant(x, y);
        let index = self.index(x, y);
        self.cells[index] = Cell {
            ch,
            style,
            width: 1,
        };
    }

    pub fn put_i32(&mut self, x: i32, y: i32, ch: char, style: Style) {
        if x < 0 || y < 0 {
            return;
        }
        self.put(x as u16, y as u16, ch, style);
    }

    pub fn put_display_char(&mut self, x: u16, y: u16, ch: char, style: Style) -> u16 {
        if x >= self.width || y >= self.height {
            return terminal_cell_width(ch);
        }

        let wanted = terminal_cell_width(ch);
        if wanted == 2 && x + 1 < self.width {
            self.erase_occupant(x, y);
            self.erase_occupant(x + 1, y);
            let lead = self.index(x, y);
            let continuation = self.index(x + 1, y);
            self.cells[lead] = Cell::wide(ch, style);
            self.cells[continuation] = Cell::continuation(style);
            2
        } else {
            self.put(x, y, ch, style);
            1
        }
    }

    pub fn put_str(&mut self, x: u16, y: u16, text: &str, style: Style) {
        if y >= self.height {
            return;
        }
        let mut column = x;
        for ch in text.chars() {
            if column >= self.width {
                break;
            }
            let advance = self.put_display_char(column, y, ch, style);
            column = column.saturating_add(advance);
        }
    }

    pub fn put_str_i32(&mut self, x: i32, y: i32, text: &str, style: Style) {
        if y < 0 || y >= self.height as i32 {
            return;
        }
        let mut column = x;
        for ch in text.chars() {
            let advance = terminal_cell_width(ch) as i32;
            if column >= 0 && column < self.width as i32 {
                self.put_display_char(column as u16, y as u16, ch, style);
            }
            column += advance;
            if column >= self.width as i32 {
                break;
            }
        }
    }

    pub fn fill_rect(
        &mut self,
        x: u16,
        y: u16,
        width: u16,
        height: u16,
        ch: char,
        style: Style,
    ) {
        let right = x.saturating_add(width).min(self.width);
        let bottom = y.saturating_add(height).min(self.height);
        for row in y..bottom {
            for column in x..right {
                self.put(column, row, ch, style);
            }
        }
    }

    pub fn hline_i32(&mut self, x: i32, y: i32, width: i32, ch: char, style: Style) {
        for column in x..x.saturating_add(width.max(0)) {
            self.put_i32(column, y, ch, style);
        }
    }

    pub fn vline_i32(&mut self, x: i32, y: i32, height: i32, ch: char, style: Style) {
        for row in y..y.saturating_add(height.max(0)) {
            self.put_i32(x, row, ch, style);
        }
    }

    fn cell(&self, x: u16, y: u16) -> Cell {
        self.cells[self.index(x, y)]
    }
}

#[derive(Default)]
pub struct Renderer {
    previous: Option<Frame>,
}

impl Renderer {
    pub fn invalidate(&mut self) {
        self.previous = None;
    }

    pub fn present<W: Write>(&mut self, out: &mut W, next: Frame) -> io::Result<()> {
        match self.previous.as_ref() {
            None => {
                queue!(out, ResetColor, MoveTo(0, 0), Clear(ClearType::All))?;
                self.paint_nonblank(out, &next)?;
            }
            Some(previous)
                if previous.width() != next.width() || previous.height() != next.height() =>
            {
                queue!(out, ResetColor, MoveTo(0, 0), Clear(ClearType::All))?;
                self.paint_all(out, &next)?;
            }
            Some(previous) => self.paint_diff(out, previous, &next)?,
        }

        queue!(out, ResetColor, SetAttribute(Attribute::Reset))?;
        out.flush()?;
        self.previous = Some(next);
        Ok(())
    }

    fn paint_nonblank<W: Write>(&self, out: &mut W, frame: &Frame) -> io::Result<()> {
        for y in 0..frame.height() {
            let mut x = 0;
            while x < frame.width() {
                let cell = frame.cell(x, y);
                if cell.is_continuation() {
                    x += 1;
                    continue;
                }
                if cell.is_wide() {
                    if cell != Cell::default() {
                        paint_run(out, x, y, cell.style, &cell.ch.to_string())?;
                    }
                    x = x.saturating_add(2);
                    continue;
                }
                if cell == Cell::default() {
                    x += 1;
                    continue;
                }

                let start = x;
                let style = cell.style;
                let mut text = String::new();
                while x < frame.width() {
                    let candidate = frame.cell(x, y);
                    if candidate == Cell::default()
                        || candidate.style != style
                        || candidate.is_continuation()
                        || candidate.is_wide()
                    {
                        break;
                    }
                    text.push(candidate.ch);
                    x += 1;
                }
                paint_run(out, start, y, style, &text)?;
            }
        }
        Ok(())
    }

    fn paint_all<W: Write>(&self, out: &mut W, frame: &Frame) -> io::Result<()> {
        for y in 0..frame.height() {
            self.paint_range(out, frame, y, 0, frame.width())?;
        }
        Ok(())
    }

    fn paint_diff<W: Write>(&self, out: &mut W, previous: &Frame, next: &Frame) -> io::Result<()> {
        let width = next.width() as usize;
        let mut dirty = vec![false; width];
        for y in 0..next.height() {
            dirty.fill(false);

            for x in 0..next.width() {
                if previous.cell(x, y) != next.cell(x, y) {
                    mark_footprint(&mut dirty, previous, x, y);
                    mark_footprint(&mut dirty, next, x, y);
                }
            }

            let mut x = 0usize;
            while x < width {
                if !dirty[x] {
                    x += 1;
                    continue;
                }
                let start = x;
                while x < width && dirty[x] {
                    x += 1;
                }
                self.paint_range(out, next, y, start as u16, x as u16)?;
            }
        }
        Ok(())
    }

    fn paint_range<W: Write>(
        &self,
        out: &mut W,
        frame: &Frame,
        y: u16,
        start: u16,
        end: u16,
    ) -> io::Result<()> {
        let mut x = start;
        while x < end {
            let cell = frame.cell(x, y);
            if cell.is_continuation() {
                x += 1;
                continue;
            }
            if cell.is_wide() {
                paint_run(out, x, y, cell.style, &cell.ch.to_string())?;
                x = x.saturating_add(2);
                continue;
            }

            let run_start = x;
            let style = cell.style;
            let mut text = String::new();
            while x < end {
                let candidate = frame.cell(x, y);
                if candidate.style != style || candidate.is_continuation() || candidate.is_wide() {
                    break;
                }
                text.push(candidate.ch);
                x += 1;
            }
            paint_run(out, run_start, y, style, &text)?;
        }
        Ok(())
    }
}

fn mark_footprint(dirty: &mut [bool], frame: &Frame, x: u16, y: u16) {
    let width = frame.width() as usize;
    let cell = frame.cell(x, y);
    let x = x as usize;

    if cell.is_continuation() {
        if x > 0 {
            dirty[x - 1] = true;
        }
        if x < width {
            dirty[x] = true;
        }
    } else if cell.is_wide() {
        if x < width {
            dirty[x] = true;
        }
        if x + 1 < width {
            dirty[x + 1] = true;
        }
    } else if x < width {
        dirty[x] = true;
    }
}

fn paint_run<W: Write>(out: &mut W, x: u16, y: u16, style: Style, text: &str) -> io::Result<()> {
    if text.is_empty() {
        return Ok(());
    }
    queue!(
        out,
        MoveTo(x, y),
        SetAttribute(Attribute::Reset),
        SetForegroundColor(style.fg),
        SetBackgroundColor(style.bg),
    )?;
    if style.bold {
        queue!(out, SetAttribute(Attribute::Bold))?;
    }
    if style.dim {
        queue!(out, SetAttribute(Attribute::Dim))?;
    }
    if style.underline {
        queue!(out, SetAttribute(Attribute::Underlined))?;
    }
    queue!(out, Print(text))
}

#[cfg(test)]
mod tests {
    use super::{clip_text_cells, text_cell_width, Frame, Style};

    #[test]
    fn clips_to_terminal_cells() {
        assert_eq!(clip_text_cells("abcdef", 4), "abc…");
        assert_eq!(clip_text_cells("ab", 4), "ab");
    }

    #[test]
    fn frame_accepts_text() {
        let mut frame = Frame::new(8, 2);
        frame.put_str(0, 0, "tpixel", Style::default());
        assert_eq!(text_cell_width("tpixel"), 6);
    }
}
