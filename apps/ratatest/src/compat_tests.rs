//! Replay the vendored backend's actual escape bytes through TRUEOS's parser.
use std::{
    cell::RefCell,
    io::{self, Write},
    rc::Rc,
};

use ratatui::{
    Terminal, TerminalOptions, Viewport,
    backend::CrosstermBackend,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
};
use trueos_terminal::{CellStyle, Terminal as Screen, TerminalColor};

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
    fn replay_into(&self, screen: &mut Screen) -> Vec<u8> {
        let bytes = std::mem::take(&mut *self.0.borrow_mut());
        // Transport chunks need not end on either a CSI or UTF-8 boundary.
        for chunk in bytes.chunks(3) {
            screen.feed(chunk);
        }
        bytes
    }
}

fn terminal(cols: u16, rows: u16) -> (Terminal<CrosstermBackend<Capture>>, Capture, Screen) {
    // CI and agent hosts may set NO_COLOR; these tests need actual color bytes.
    crossterm::style::force_color_output(true);
    let capture = Capture::default();
    let terminal = Terminal::with_options(
        CrosstermBackend::new(capture.clone()),
        TerminalOptions {
            viewport: Viewport::Fixed(Rect::new(0, 0, cols, rows)),
        },
    )
    .unwrap();
    (terminal, capture, Screen::new(cols as usize, rows as usize))
}

#[test]
fn backend_diff_erases_a_long_ascii_suffix_when_content_shrinks() {
    let (mut terminal, capture, mut screen) = terminal(40, 3);
    terminal
        .draw(|frame| {
            frame.render_widget(
                Paragraph::new("composer: an obsolete long suffix"),
                frame.area(),
            );
        })
        .unwrap();
    capture.replay_into(&mut screen);
    assert_eq!(
        screen.render_rows()[0],
        format!("composer: an obsolete long suffix{}", " ".repeat(7))
    );

    terminal
        .draw(|frame| {
            frame.render_widget(Paragraph::new("composer: hi"), frame.area());
        })
        .unwrap();
    let diff = capture.replay_into(&mut screen);
    assert!(!diff.is_empty());
    assert_eq!(
        screen.render_rows()[0],
        format!("composer: hi{}", " ".repeat(28))
    );
    assert!(
        screen.render_rows()[1..]
            .iter()
            .all(|row| row.trim().is_empty())
    );
}

#[test]
fn raw_character_editing_specimens_match_their_printed_expectations() {
    let mut screen = Screen::new(80, 24);
    for chunk in crate::ansi::specimen(0, 80, 24).as_bytes().chunks(3) {
        screen.feed(chunk);
    }
    let rows = screen.render_rows();
    assert_eq!(rows[9].trim_end(), "ABE");
    assert_eq!(rows[11].trim_end(), "ABXYCDE");
    assert_eq!(rows[13].trim_end(), "AB  E");
    assert_eq!(rows[15].trim_end(), "Saved cursor: OK");
}

#[test]
fn backend_selection_emits_inverse_reset_and_preserves_cell_colors() {
    let (mut terminal, capture, mut screen) = terminal(16, 2);
    let normal = Style::default()
        .fg(Color::Rgb(10, 20, 30))
        .bg(Color::Indexed(52));
    terminal
        .draw(|frame| {
            let line = Line::from(vec![
                Span::styled("P", normal),
                Span::styled(
                    "SEL",
                    normal.add_modifier(Modifier::REVERSED | Modifier::UNDERLINED),
                ),
                Span::styled("Q", normal),
                Span::raw("Z"),
            ]);
            frame.render_widget(Paragraph::new(line), frame.area());
        })
        .unwrap();
    let bytes = capture.replay_into(&mut screen);
    assert!(bytes.windows(4).any(|window| window == b"\x1b[7m"));
    assert!(bytes.windows(5).any(|window| window == b"\x1b[27m"));
    assert_eq!(&screen.render_rows()[0][..6], "PSELQZ");
    let cells = screen.cells();
    for cell in &cells[1..4] {
        assert!(cell.style.reversed);
        assert!(cell.style.underline);
        assert_eq!(
            cell.style.foreground,
            TerminalColor::Rgb {
                red: 10,
                green: 20,
                blue: 30
            }
        );
        assert_eq!(cell.style.background, TerminalColor::Indexed(52));
    }
    assert!(!cells[0].style.reversed);
    assert!(!cells[4].style.reversed);
    assert!(!cells[4].style.underline);
    assert_eq!(cells[5].style, CellStyle::default());
}

#[test]
fn raw_history_scroll_keeps_the_header_and_composer_fixed() {
    let mut screen = Screen::new(80, 24);
    let specimen = crate::ansi::specimen(1, 80, 24);
    for chunk in specimen.as_bytes().chunks(5) {
        screen.feed(chunk);
    }
    let rows = screen.render_rows();
    assert_eq!(rows[2].trim_end(), "FIXED HEADER - must not scroll");
    assert_eq!(rows[20].trim_end(), "FIXED COMPOSER > keep this text");
    assert_eq!(rows[17].trim_end(), "history line 24");
    assert!(
        !rows[5..19]
            .iter()
            .any(|row| row.contains("history line 01"))
    );
    assert!(rows[22].starts_with("Esc returns / 20 second timeout"));
    // The specimen restores margins before handing the terminal back.
    screen.feed(b"\x1b[24;1H\n");
    assert_eq!(
        screen.render_rows()[1].trim_end(),
        "FIXED HEADER - must not scroll"
    );
}

#[test]
fn raw_capability_queries_preserve_the_saved_cursor_anchor() {
    let mut screen = Screen::new(80, 24);
    let specimen = crate::ansi::specimen(2, 80, 24);
    // Include arbitrary splits through the prefixed keyboard-control queries.
    for chunk in specimen.as_bytes().chunks(2) {
        screen.feed(chunk);
    }
    let rows = screen.render_rows();
    assert_eq!(rows[4].trim_end(), "saved position:");
    assert_eq!(rows[6].trim_end(), "ANCHOR:OK");
    let responses = screen.take_responses();
    assert!(responses.ends_with(b"\x1b[?1;2c"));
    assert_eq!(responses.iter().filter(|&&byte| byte == b'R').count(), 1);
    assert_eq!(responses.iter().filter(|&&byte| byte == b'c').count(), 1);
    assert_eq!(responses.iter().filter(|&&byte| byte == b'\x1b').count(), 2);
}
