use file_system::{
    BUROSCH_AVEC_TERMINAL_PATTERN, ColorRampStyle, CssColor, TerminalTestPatternStyle,
};

#[derive(Clone, Copy)]
struct TerminalCell {
    bg: CssColor,
    fg: CssColor,
    ch: char,
}

struct Canvas {
    width: usize,
    height: usize,
    cells: Vec<TerminalCell>,
}

impl Canvas {
    fn new(width: usize, height: usize, background: CssColor) -> Self {
        Self {
            width,
            height,
            cells: vec![
                TerminalCell {
                    bg: background,
                    fg: background,
                    ch: ' ',
                };
                width * height
            ],
        }
    }

    fn fill_rect(&mut self, x: usize, y: usize, width: usize, height: usize, color: CssColor) {
        for row in y..(y + height).min(self.height) {
            for column in x..(x + width).min(self.width) {
                self.set(column, row, ' ', color, color);
            }
        }
    }

    fn set(&mut self, x: usize, y: usize, ch: char, fg: CssColor, bg: CssColor) {
        if x >= self.width || y >= self.height {
            return;
        }
        self.cells[y * self.width + x] = TerminalCell { bg, fg, ch };
    }

    fn draw_text(&mut self, x: usize, y: usize, text: &str, fg: CssColor, bg: CssColor) {
        for (offset, ch) in text.chars().enumerate() {
            if x + offset >= self.width {
                break;
            }
            self.set(x + offset, y, ch, fg, bg);
        }
    }

    fn draw_grid(&mut self, step_x: usize, step_y: usize, line: CssColor, background: CssColor) {
        for y in 0..self.height {
            for x in 0..self.width {
                let on_vertical = x % step_x == 0;
                let on_horizontal = y % step_y == 0;
                let glyph = match (on_vertical, on_horizontal) {
                    (true, true) => '┼',
                    (true, false) => '│',
                    (false, true) => '─',
                    (false, false) => continue,
                };
                self.set(x, y, glyph, line, background);
            }
        }
    }

    fn draw_color_bars(
        &mut self,
        x: usize,
        y: usize,
        width: usize,
        height: usize,
        bars: &[CssColor],
    ) {
        let segment_width = width / bars.len().max(1);
        for (index, color) in bars.iter().copied().enumerate() {
            let left = x + index * segment_width;
            let right = if index + 1 == bars.len() {
                x + width
            } else {
                left + segment_width
            };
            self.fill_rect(left, y, right.saturating_sub(left), height, color);
        }
    }

    fn draw_ramp(&mut self, x: usize, y: usize, width: usize, height: usize, ramp: ColorRampStyle) {
        for offset in 0..width {
            let t = if width <= 1 {
                0.0
            } else {
                offset as f32 / (width - 1) as f32
            };
            let color = ramp.sample(t);
            self.fill_rect(x + offset, y, 1, height, color);
        }
    }

    fn draw_stripes(
        &mut self,
        x: usize,
        y: usize,
        width: usize,
        height: usize,
        first: CssColor,
        second: CssColor,
    ) {
        for offset in 0..width {
            let color = if offset % 2 == 0 { first } else { second };
            self.fill_rect(x + offset, y, 1, height, color);
        }
    }

    fn draw_checker(
        &mut self,
        x: usize,
        y: usize,
        width: usize,
        height: usize,
        blocks_x: usize,
        blocks_y: usize,
        dark: CssColor,
        light: CssColor,
    ) {
        let cell_width = width / blocks_x.max(1);
        let cell_height = height / blocks_y.max(1);
        for row in 0..blocks_y {
            for column in 0..blocks_x {
                let color = if (row + column) % 2 == 0 { light } else { dark };
                self.fill_rect(
                    x + column * cell_width,
                    y + row * cell_height,
                    cell_width.max(1),
                    cell_height.max(1),
                    color,
                );
            }
        }
    }

    fn render(&self) -> String {
        let mut output = String::new();
        let mut current_bg = None;
        let mut current_fg = None;

        for row in 0..self.height {
            for column in 0..self.width {
                let cell = self.cells[row * self.width + column];
                if current_bg != Some(cell.bg) || current_fg != Some(cell.fg) {
                    output.push_str(&ansi_fg(cell.fg));
                    output.push_str(&ansi_bg(cell.bg));
                    current_bg = Some(cell.bg);
                    current_fg = Some(cell.fg);
                }
                output.push(cell.ch);
            }
            output.push_str("\x1b[0m\n");
            current_bg = None;
            current_fg = None;
        }
        output.push_str("\x1b[0m");
        output
    }
}

fn ansi_fg(color: CssColor) -> String {
    format!("\x1b[38;2;{};{};{}m", color.r, color.g, color.b)
}

fn ansi_bg(color: CssColor) -> String {
    format!("\x1b[48;2;{};{};{}m", color.r, color.g, color.b)
}

fn main() {
    let style = BUROSCH_AVEC_TERMINAL_PATTERN;
    let mut canvas = Canvas::new(
        style.terminal_width_chars as usize,
        style.terminal_height_rows as usize,
        style.background,
    );

    draw_pattern(&mut canvas, style);
    println!("{}", canvas.render());
}

fn draw_pattern(canvas: &mut Canvas, style: TerminalTestPatternStyle) {
    let width = style.terminal_width_chars as usize;
    let height = style.terminal_height_rows as usize;

    canvas.draw_grid(4, 2, style.grid.color, style.background);

    let bar_left = 10;
    let bar_width = width - 20;
    canvas.draw_color_bars(bar_left, 6, bar_width, 5, &style.primary_bars);
    canvas.draw_ramp(bar_left, 11, bar_width, 4, style.grayscale);

    canvas.draw_stripes(bar_left, 15, 18, 4, style.black, style.white);
    canvas.draw_checker(
        bar_left + 18,
        15,
        8,
        4,
        2,
        2,
        style.background.lerp(style.black, 0.35),
        style.background.lerp(style.white, 0.25),
    );
    canvas.draw_checker(
        bar_left + bar_width - 26,
        15,
        8,
        4,
        2,
        2,
        style.background.lerp(style.black, 0.35),
        style.background.lerp(style.white, 0.25),
    );
    canvas.draw_stripes(
        bar_left + bar_width - 18,
        15,
        18,
        4,
        style.white,
        style.black,
    );

    let panel_left = 12;
    let panel_top = 19;
    let panel_width = width - 24;
    canvas.fill_rect(panel_left, panel_top, panel_width, 4, style.panel);
    canvas.draw_text(
        panel_left + panel_width / 2 - 4,
        panel_top + 1,
        "BUROSCH",
        style.text_blue,
        style.panel,
    );
    canvas.draw_text(
        panel_left + 14,
        panel_top + 2,
        "AVEC",
        style.text_blue,
        style.panel,
    );
    canvas.draw_text(
        panel_left + 20,
        panel_top + 2,
        "Audio Video Equipment Check",
        style.text_dark,
        style.panel,
    );
    canvas.draw_text(
        panel_left + panel_width - 18,
        panel_top + 2,
        "3840 x 2160",
        style.text_dark,
        style.panel,
    );

    let ramp_left = 14;
    let ramp_width = width - 28;
    canvas.draw_ramp(ramp_left, 25, ramp_width, 2, style.red_ramp);
    canvas.draw_ramp(ramp_left, 27, ramp_width, 2, style.green_ramp);
    canvas.draw_ramp(ramp_left, 29, ramp_width, 2, style.blue_ramp);

    canvas.draw_text(2, 1, style.name, style.white, style.background);
    canvas.draw_text(
        2,
        height.saturating_sub(2),
        "CLI approximation of docs/video.png using tile_debug_style.rs",
        style.white,
        style.background,
    );

    draw_markers(canvas, style);
    draw_circle_hint(
        canvas,
        width / 2,
        height / 2,
        18,
        style.grid.color,
        style.background,
    );
}

fn draw_markers(canvas: &mut Canvas, style: TerminalTestPatternStyle) {
    let w = canvas.width;
    let h = canvas.height;
    let positions = [(w / 2, 0), (w / 2, h - 1), (0, h / 2), (w - 1, h / 2)];

    for &(x, y) in &positions {
        canvas.set(x, y, '▲', style.accent_green, style.background);
    }
}

fn draw_circle_hint(
    canvas: &mut Canvas,
    center_x: usize,
    center_y: usize,
    radius: usize,
    fg: CssColor,
    bg: CssColor,
) {
    let radius = radius as f32;
    for y in 0..canvas.height {
        for x in 0..canvas.width {
            let dx = x as f32 - center_x as f32;
            let dy = (y as f32 - center_y as f32) * 1.8;
            let distance = (dx * dx + dy * dy).sqrt();
            if (distance - radius).abs() < 0.7 {
                canvas.set(x, y, '·', fg, bg);
            }
        }
    }
}
