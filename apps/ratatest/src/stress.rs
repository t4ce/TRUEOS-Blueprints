//! Bounded, deterministic replay of real vendored backend bytes into TRUEOS.
//!
//! The equivalence workload deliberately uses ASCII. TRUEOS currently assigns
//! one cell per Unicode scalar, so this must not certify wide grapheme support.
use std::{
    cell::RefCell,
    io::{self, Write},
    rc::Rc,
    time::Instant,
};

use ratatui::{
    Frame, Terminal, TerminalOptions, Viewport,
    backend::{Backend, ClearType, CrosstermBackend, WindowSize},
    layout::{Position, Rect, Size},
    style::{Color, Modifier, Style},
    symbols::border,
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph},
};
use trueos_terminal::{Cell, CellStyle, Terminal as Screen, TerminalColor};

const SIZES: [(u16, u16); 4] = [(20, 5), (80, 24), (40, 12), (120, 40)];
const MAX_FRAMES: usize = 10_000;
const MAX_FAILURES: usize = 8;
const ASCII_BORDER: border::Set<'static> = border::Set {
    top_left: "+",
    top_right: "+",
    bottom_left: "+",
    bottom_right: "+",
    vertical_left: "|",
    vertical_right: "|",
    horizontal_top: "-",
    horizontal_bottom: "-",
};

#[derive(Clone, Default)]
struct Capture(Rc<RefCell<Vec<u8>>>);

impl Write for Capture {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0.borrow_mut().extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Capture {
    fn take(&self) -> Vec<u8> {
        std::mem::take(&mut *self.0.borrow_mut())
    }
}

// Only geometry is virtual: every draw, clear, cursor command, and flush still
// uses the real vendored CrosstermBackend. Fixed-viewport resize otherwise asks
// the execution host's TTY for its size, which is absent in headless CI.
struct HeadlessBackend {
    inner: CrosstermBackend<Capture>,
    size: Size,
}

impl Backend for HeadlessBackend {
    type Error = io::Error;

    fn draw<'a, I>(&mut self, content: I) -> io::Result<()>
    where
        I: Iterator<Item = (u16, u16, &'a ratatui::buffer::Cell)>,
    {
        self.inner.draw(content)
    }

    fn hide_cursor(&mut self) -> io::Result<()> {
        self.inner.hide_cursor()
    }

    fn show_cursor(&mut self) -> io::Result<()> {
        self.inner.show_cursor()
    }

    fn get_cursor_position(&mut self) -> io::Result<Position> {
        Err(io::Error::other(
            "headless replay does not issue cursor queries",
        ))
    }

    fn set_cursor_position<P: Into<Position>>(&mut self, position: P) -> io::Result<()> {
        self.inner.set_cursor_position(position)
    }

    fn clear(&mut self) -> io::Result<()> {
        self.inner.clear()
    }

    fn clear_region(&mut self, clear_type: ClearType) -> io::Result<()> {
        self.inner.clear_region(clear_type)
    }

    fn size(&self) -> io::Result<Size> {
        Ok(self.size)
    }

    fn window_size(&mut self) -> io::Result<WindowSize> {
        Ok(WindowSize {
            columns_rows: self.size,
            pixels: Size::ZERO,
        })
    }

    fn flush(&mut self) -> io::Result<()> {
        Backend::flush(&mut self.inner)
    }
}

type BackendTerminal = Terminal<HeadlessBackend>;

fn terminal(area: Rect) -> io::Result<(BackendTerminal, Capture)> {
    let capture = Capture::default();
    let terminal = Terminal::with_options(
        HeadlessBackend {
            inner: CrosstermBackend::new(capture.clone()),
            size: area.as_size(),
        },
        TerminalOptions {
            viewport: Viewport::Fixed(area),
        },
    )?;
    Ok((terminal, capture))
}

/// Frame transport splits both CSI and (in the separate probe) UTF-8 sequences.
fn replay(screen: &mut Screen, bytes: &[u8], phase: usize) {
    let widths = [1, 3, 7];
    let mut offset = 0;
    let mut chunk = phase;
    while offset < bytes.len() {
        let end = (offset + widths[chunk % widths.len()]).min(bytes.len());
        screen.feed(&bytes[offset..end]);
        offset = end;
        chunk += 1;
    }
}

#[derive(Debug, PartialEq, Eq)]
struct Report {
    frames: usize,
    bytes: usize,
    full_bytes: usize,
    cells: usize,
    resizes: usize,
    digest: u64,
    failed_frames: usize,
    failures: Vec<String>,
}

impl Default for Report {
    fn default() -> Self {
        Self {
            frames: 0,
            bytes: 0,
            full_bytes: 0,
            cells: 0,
            resizes: 0,
            digest: 0xcbf29ce484222325,
            failed_frames: 0,
            failures: Vec::new(),
        }
    }
}

impl Report {
    fn hash(&mut self, bytes: &[u8]) {
        for byte in bytes {
            self.digest ^= u64::from(*byte);
            self.digest = self.digest.wrapping_mul(0x100000001b3);
        }
    }

    fn failure(&mut self, message: String) {
        if self.failures.len() < MAX_FAILURES {
            self.failures.push(message);
        }
    }
}

/// Run two complete passes, with actual resizes, and check deterministic output.
/// `frames` is the number of frames at each of the four terminal sizes.
pub fn run(frames: usize) -> io::Result<()> {
    if !(1..=MAX_FRAMES).contains(&frames) {
        return Err(io::Error::other(format!(
            "--stress-check frames per size must be in 1..={MAX_FRAMES}"
        )));
    }
    crossterm::style::force_color_output(true);
    let started = Instant::now();
    transport_probe()?;
    let first = check(frames)?;
    let second = check(frames)?;
    let deterministic = first == second;
    let saved = first.full_bytes as i128 - first.bytes as i128;
    let saving = 100.0 * saved as f64 / first.full_bytes.max(1) as f64;
    println!(
        "ASCII backend/parser stress: {} frames/pass, {} checked cells/pass, 2 passes, {} actual resizes/pass",
        first.frames, first.cells, first.resizes
    );
    println!("sizes=20x5,80x24,40x12,120x40; chunks=1,3,7 bytes; UTF-8 transport probe=PASS");
    println!(
        "diff bytes={} full-redraw bytes={} saved={} ({saving:.2}%) [one pass]",
        first.bytes, first.full_bytes, saved
    );
    println!(
        "elapsed={:.3}s hash={:016x} deterministic={} mismatching frames/pass={}",
        started.elapsed().as_secs_f64(),
        first.digest,
        if deterministic { "PASS" } else { "FAIL" },
        first.failed_frames
    );
    for failure in &first.failures {
        eprintln!("  {failure}");
    }
    if first.failed_frames != 0 || !deterministic {
        return Err(io::Error::other("backend/parser stress validation failed"));
    }
    Ok(())
}

fn check(frames: usize) -> io::Result<Report> {
    let (mut terminal, capture) = terminal(Rect::new(0, 0, SIZES[0].0, SIZES[0].1))?;
    let mut screen = Screen::new(SIZES[0].0 as usize, SIZES[0].1 as usize);
    let mut report = Report::default();
    for (size_index, (cols, rows)) in SIZES.into_iter().enumerate() {
        let area = Rect::new(0, 0, cols, rows);
        if size_index != 0 {
            screen.resize(cols as usize, rows as usize);
            terminal.backend_mut().size = area.as_size();
            terminal.resize(area)?;
            report.resizes += 1;
        }
        for step in 0..frames {
            let sequence = step + size_index * frames;
            let drawn = terminal.draw(|frame| dashboard(frame, step))?;
            // Also compare against the intended buffer: two paths using the same
            // parser alone could agree while both render the wrong style/glyph.
            let expected: Vec<Cell> = drawn
                .buffer
                .content
                .iter()
                .map(project)
                .collect::<io::Result<_>>()?;
            let bytes = capture.take();
            replay(&mut screen, &bytes, sequence);
            let (mut full, full_capture) = terminal_for_redraw(area)?;
            full.draw(|frame| dashboard(frame, step))?;
            let full_bytes = full_capture.take();
            let mut reference = Screen::new(cols as usize, rows as usize);
            replay(&mut reference, &full_bytes, sequence + 1);

            let mut failed = false;
            for (name, actual) in [("diff", screen.cells()), ("full", reference.cells())] {
                if let Some((index, (actual, expected))) = actual
                    .iter()
                    .zip(&expected)
                    .enumerate()
                    .find(|(_, (a, b))| a != b)
                {
                    failed = true;
                    report.failure(format!(
                        "{cols}x{rows} frame={step} {name} at {},{} actual={actual:?} expected={expected:?}",
                        index % cols as usize,
                        index / cols as usize
                    ));
                }
            }
            if screen.cells() != reference.cells() {
                failed = true;
                report.failure(format!(
                    "{cols}x{rows} frame={step}: diff != fresh full redraw"
                ));
            }
            report.failed_frames += usize::from(failed);
            report.frames += 1;
            report.cells += expected.len();
            report.bytes += bytes.len();
            report.full_bytes += full_bytes.len();
            // Hash actual backend bytes and visible cells, with explicit field
            // encoding so the digest does not depend on Rust's Debug output.
            report.hash(&(bytes.len() as u64).to_le_bytes());
            report.hash(&bytes);
            for cell in screen.cells() {
                report.hash(&(cell.glyph as u32).to_le_bytes());
                hash_color(&mut report, cell.style.foreground);
                hash_color(&mut report, cell.style.background);
                report.hash(&[
                    u8::from(cell.style.underline),
                    u8::from(cell.style.reversed),
                ]);
            }
        }
    }
    Ok(report)
}

fn terminal_for_redraw(area: Rect) -> io::Result<(BackendTerminal, Capture)> {
    let (mut terminal, capture) = terminal(area)?;
    terminal.backend_mut().clear()?;
    Ok((terminal, capture))
}

fn project(cell: &ratatui::buffer::Cell) -> io::Result<Cell> {
    if cell.symbol().len() != 1 || !cell.symbol().is_ascii() {
        return Err(io::Error::other(format!(
            "non-ASCII symbol {:?} entered ASCII equivalence workload",
            cell.symbol()
        )));
    }
    Ok(Cell {
        glyph: cell.symbol().chars().next().unwrap_or(' '),
        style: CellStyle {
            foreground: color(cell.fg),
            background: color(cell.bg),
            underline: cell.modifier.contains(Modifier::UNDERLINED),
            reversed: cell.modifier.contains(Modifier::REVERSED),
        },
    })
}

fn color(color: Color) -> TerminalColor {
    match color {
        Color::Reset => TerminalColor::Default,
        Color::Indexed(index) => TerminalColor::Indexed(index),
        Color::Rgb(red, green, blue) => TerminalColor::Rgb { red, green, blue },
        Color::Black => TerminalColor::Indexed(0),
        Color::Red => TerminalColor::Indexed(1),
        Color::Green => TerminalColor::Indexed(2),
        Color::Yellow => TerminalColor::Indexed(3),
        Color::Blue => TerminalColor::Indexed(4),
        Color::Magenta => TerminalColor::Indexed(5),
        Color::Cyan => TerminalColor::Indexed(6),
        Color::Gray => TerminalColor::Indexed(7),
        Color::DarkGray => TerminalColor::Indexed(8),
        Color::LightRed => TerminalColor::Indexed(9),
        Color::LightGreen => TerminalColor::Indexed(10),
        Color::LightYellow => TerminalColor::Indexed(11),
        Color::LightBlue => TerminalColor::Indexed(12),
        Color::LightMagenta => TerminalColor::Indexed(13),
        Color::LightCyan => TerminalColor::Indexed(14),
        Color::White => TerminalColor::Indexed(15),
    }
}

fn hash_color(report: &mut Report, color: TerminalColor) {
    match color {
        TerminalColor::Default => report.hash(&[0]),
        TerminalColor::Indexed(index) => report.hash(&[1, index]),
        TerminalColor::Rgb { red, green, blue } => report.hash(&[2, red, green, blue]),
    }
}

fn block(title: &'static str) -> Block<'static> {
    Block::default()
        .borders(Borders::ALL)
        .border_set(ASCII_BORDER)
        .title(title)
}

fn dashboard(frame: &mut Frame<'_>, step: usize) {
    let area = frame.area();
    // Repeat each state once: real diff efficiency must include idle frames.
    let tick = step / 2;
    let phase = tick % 12;
    let base = if phase < 4 {
        Style::default().fg(Color::Gray).bg(Color::Indexed(17))
    } else if phase < 8 {
        Style::default()
            .fg(Color::Rgb(190, 215, 240))
            .bg(Color::Rgb(8, 12, 20))
    } else {
        Style::default()
    };
    frame.render_widget(Block::default().style(base), area);
    let title = if phase % 4 == 0 {
        "RATATEST / ACTIVE TASK: a deliberately long title that must disappear"
    } else {
        "RATATEST"
    };
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(title, base.add_modifier(Modifier::REVERSED)),
            Span::styled(format!(" step {tick:04}"), base),
        ])),
        Rect::new(0, 0, area.width, 1),
    );
    let inner = Rect::new(0, 1, area.width, area.height.saturating_sub(2));
    let left_width = if area.width >= 40 {
        area.width / 3
    } else {
        area.width
    };
    let left = Rect::new(inner.x, inner.y, left_width, inner.height);
    let items: Vec<ListItem<'_>> = (0..36)
        .map(|index| {
            let text = if (index + tick) % 3 == 0 {
                format!("task {index:02}: compiling an extended stale suffix")
            } else {
                format!("task {index:02}")
            };
            ListItem::new(text).style(if index % 5 == 0 {
                base.add_modifier(Modifier::UNDERLINED)
            } else {
                base
            })
        })
        .collect();
    let mut state = ListState::default().with_selected(Some((tick * 5) % 36));
    frame.render_stateful_widget(
        List::new(items)
            .block(block(" jobs ").style(base))
            .highlight_symbol("> ")
            .highlight_style(base.add_modifier(Modifier::REVERSED)),
        left,
        &mut state,
    );
    if area.width >= 40 {
        let right = Rect::new(left_width, inner.y, area.width - left_width, inner.height);
        let lines: Vec<Line<'_>> = (0..80)
            .map(|index| {
                Line::from(vec![
                    Span::styled(
                        format!("{:03} ", index + tick),
                        base.fg(Color::Indexed(((index * 13 + tick) % 256) as u8)),
                    ),
                    Span::styled(
                        if index % 4 == phase % 4 {
                            "tool output: a long line grows then vanishes completely"
                        } else {
                            "ok"
                        },
                        if index % 7 == 0 {
                            base.add_modifier(Modifier::UNDERLINED | Modifier::REVERSED)
                        } else {
                            base
                        },
                    ),
                ])
            })
            .collect();
        frame.render_widget(
            Paragraph::new(lines)
                .style(base)
                .block(block(" transcript ").style(base))
                .scroll((((tick * 7) % 80) as u16, 0)),
            right,
        );
    }
    // Explicitly styled blanks alternate with text/default cells. This catches
    // background erasure and modifier-reset errors that glyph checks miss.
    let band_y = area.height.saturating_sub(2);
    frame.render_widget(
        Paragraph::new(if phase % 2 == 0 {
            "                    "
        } else {
            "reset"
        })
        .style(if phase % 2 == 0 {
            base.bg(Color::Indexed(52))
                .add_modifier(Modifier::UNDERLINED | Modifier::REVERSED)
        } else {
            Style::default()
        }),
        Rect::new(0, band_y, area.width, 1),
    );
    frame.render_widget(
        Paragraph::new(if phase % 3 == 0 {
            "> composer: a long editable request with an obsolete trailing suffix"
        } else {
            "> hi"
        })
        .style(base),
        Rect::new(0, area.height - 1, area.width, 1),
    );
    if (3..7).contains(&phase) {
        let popup = Rect::new(
            area.width / 5,
            area.height / 4,
            (area.width * 3 / 5).max(4),
            (area.height / 2).max(3),
        );
        frame.render_widget(Clear, popup);
        frame.render_widget(
            Paragraph::new("Approve tool?\n> Run once\n  Cancel")
                .style(Style::default().fg(Color::Yellow).bg(Color::Blue))
                .block(block(" approval ").style(Style::default().bg(Color::Blue))),
            popup,
        );
    }
}

fn transport_probe() -> io::Result<()> {
    // All specimens are single-cell Unicode scalars. This tests transport
    // continuity only, separately from ASCII backend equivalence above.
    let bytes = "\x1b[2J\x1b[1;1Héλ°\x1b[38;2;12;34;56mZ\x1b[0m\x1b[2;1HOK".as_bytes();
    let mut whole = Screen::new(12, 3);
    whole.feed(bytes);
    if whole.cells()[0].glyph != 'é'
        || whole.cells()[1].glyph != 'λ'
        || whole.cells()[2].glyph != '°'
        || whole.cells()[3].glyph != 'Z'
        || whole.cells()[3].style.foreground
            != (TerminalColor::Rgb {
                red: 12,
                green: 34,
                blue: 56,
            })
        || whole.cells()[12].glyph != 'O'
        || whole.cells()[13].glyph != 'K'
    {
        return Err(io::Error::other("UTF-8/CSI transport specimen failed"));
    }
    for width in [1, 3, 7] {
        let mut chunked = Screen::new(12, 3);
        for chunk in bytes.chunks(width) {
            chunked.feed(chunk);
        }
        if whole.cells() != chunked.cells() || whole.cursor() != chunked.cursor() {
            return Err(io::Error::other(format!(
                "UTF-8/CSI transport split {width} failed"
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn changing_dashboards_match_intent_and_fresh_redraw_across_resizes() {
        crossterm::style::force_color_output(true);
        let report = check(72).unwrap();
        assert_eq!(report.failed_frames, 0, "{:?}", report.failures);
        assert_eq!(report.frames, 288);
        assert_eq!(report.resizes, 3);
        assert!(report.bytes < report.full_bytes);
        assert_eq!(report, check(72).unwrap());
    }

    #[test]
    fn utf8_and_csi_survive_fragmented_transport() {
        transport_probe().unwrap();
    }

    #[test]
    fn stress_frame_limit_is_enforced_before_work_starts() {
        assert!(run(0).is_err());
        assert!(run(MAX_FRAMES + 1).is_err());
    }
}
