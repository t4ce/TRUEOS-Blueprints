//! Small independent specimens of patterns used by codex-rs/tui. No Codex services.
use crate::ui::App;
use ratatui::{
    Frame,
    buffer::{Buffer, CellDiffOption},
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{
        Block, BorderType, Clear, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState,
        Widget, Wrap,
    },
};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

const SPINNER: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
pub fn render(frame: &mut Frame, area: Rect, page: usize, app: &App) {
    match page {
        0 => prose(frame, area, app),
        1 => diff(frame, area),
        2 => composer(frame, area, app),
        3 => {
            transcript(frame, area, app);
            if app.popup {
                chooser(frame, area, app);
            }
        }
        4 => status(frame, area, app),
        5 => transcript(frame, area, app),
        6 => repaint(frame, area, app),
        _ => combined(frame, area, app),
    }
}

fn prose(frame: &mut Frame, area: Rect, app: &App) {
    let text = vec![
        Line::styled(
            "# Rendering a terminal assistant",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Line::from(""),
        Line::from(vec![
            Span::raw("Paragraphs wrap while "),
            Span::styled("inline code", Style::default().fg(Color::Yellow)),
            Span::raw(" and "),
            Span::styled("emphasis", Style::default().add_modifier(Modifier::ITALIC)),
            Span::raw(" retain their own styles."),
        ]),
        Line::from("  Indented text keeps two leading spaces (Wrap trim: false)."),
        Line::from(vec![
            Span::styled(
                "selected words",
                Style::default().add_modifier(Modifier::REVERSED),
            ),
            Span::raw(" should invert their foreground and background."),
        ]),
        Line::from(""),
        Line::styled("```rust", Style::default().fg(Color::DarkGray)),
        Line::from(vec![
            Span::styled("fn ", Style::default().fg(Color::Magenta)),
            Span::styled("main", Style::default().fg(Color::Blue)),
            Span::raw("() {"),
        ]),
        Line::from(vec![
            Span::raw("    println!("),
            Span::styled("\"Hello, TRUEOS!\"", Style::default().fg(Color::Green)),
            Span::raw(");"),
        ]),
        Line::from("}"),
        Line::styled("```", Style::default().fg(Color::DarkGray)),
        Line::from(""),
        Line::from("Pattern      │ Expected"),
        Line::from("─────────────┼─────────────────────"),
        Line::from("Soft wrap    │ no clipped words"),
        Line::from("Line styles  │ no style leakage"),
        Line::from(vec![
            Span::styled(
                "https://ratatui.rs",
                Style::default()
                    .fg(Color::Blue)
                    .add_modifier(Modifier::UNDERLINED),
            ),
            Span::raw("  (visual link; OSC8 lives in ANSI probes)"),
        ]),
        Line::from("Unicode: café  e\u{301}  日本語  🙂  👩‍💻  →  ✓"),
    ];
    frame.render_widget(
        Paragraph::new(text)
            .wrap(Wrap { trim: false })
            .scroll((app.scroll, 0))
            .block(Block::bordered().title(" Styled transcript ")),
        area,
    );
}

fn diff(frame: &mut Frame, area: Rect) {
    let lines = vec![
        Line::styled(
            "• Ran cargo test -p terminal",
            Style::default().add_modifier(Modifier::BOLD),
        ),
        Line::from("  └ 22 passed; 0 failed"),
        Line::from(""),
        Line::styled(
            "• Updated src/terminal.rs (+2 -1)",
            Style::default().fg(Color::Cyan),
        ),
        Line::styled(
            "  14 - let selected = Style::default();",
            Style::default().fg(Color::Red).bg(Color::Rgb(45, 12, 18)),
        ),
        Line::styled(
            "  14 + let selected = Style::default()",
            Style::default().fg(Color::Green).bg(Color::Rgb(10, 38, 25)),
        ),
        Line::styled(
            "  15 +     .add_modifier(Modifier::REVERSED);",
            Style::default().fg(Color::Green).bg(Color::Rgb(10, 38, 25)),
        ),
        Line::from(""),
        Line::from(vec![
            Span::raw("deleted "),
            Span::styled(
                "old value",
                Style::default()
                    .fg(Color::Red)
                    .add_modifier(Modifier::CROSSED_OUT),
            ),
            Span::raw("  added "),
            Span::styled(
                "new value",
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::UNDERLINED),
            ),
        ]),
        Line::from(vec![
            Span::raw("Styled blanks between brackets: ["),
            Span::styled("        ", Style::default().bg(Color::Blue)),
            Span::raw("]"),
        ]),
        Line::from(""),
        Line::styled(
            "This is simulated tool output; no command is executed.",
            Style::default().add_modifier(Modifier::DIM),
        ),
    ];
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .block(Block::bordered().title(" Tool transcript / diff ")),
        area,
    );
}

pub fn composer(frame: &mut Frame, area: Rect, app: &App) {
    let [field, submitted] =
        Layout::vertical([Constraint::Length(5), Constraint::Min(0)]).areas(area);
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(if app.editing {
            Color::Cyan
        } else {
            Color::Gray
        }))
        .title(" Composer (i to edit) ");
    let inner = block.inner(field);
    frame.render_widget(block, field);
    let display = if app.input.is_empty() && !app.editing {
        "Ask for a change, @ a file, or / a command…"
    } else {
        &app.input
    };
    let (tail, width) = visible_tail(display, inner.width.saturating_sub(3) as usize);
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("› ", Style::default().fg(Color::Cyan)),
            Span::raw(tail),
        ])),
        inner,
    );
    if app.editing && inner.width > 2 && inner.height > 0 {
        frame.set_cursor_position((inner.x + 2 + width as u16, inner.y));
    }
    let lines: Vec<_> = app
        .submissions
        .iter()
        .rev()
        .take(submitted.height as usize)
        .map(|line| Line::from(format!("You: {line}")))
        .collect();
    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), submitted);
}

/// Keep the cursor inside the field without splitting a grapheme or a wide glyph.
fn visible_tail(text: &str, max_width: usize) -> (&str, usize) {
    let mut width = 0;
    let mut start = text.len();
    for (index, grapheme) in text.grapheme_indices(true).rev() {
        let next = UnicodeWidthStr::width(grapheme);
        if width + next > max_width {
            break;
        }
        width += next;
        start = index;
    }
    (&text[start..], width)
}

fn status(frame: &mut Frame, area: Rect, app: &App) {
    let phases = [
        "Inspecting workspace",
        "Reading terminal code",
        "Writing a focused patch",
        "Running tests",
    ];
    let label = phases[(app.tick / 12) as usize % phases.len()];
    let lines = vec![
        Line::from(vec![
            Span::styled(
                format!("{} ", SPINNER[app.tick as usize % SPINNER.len()]),
                Style::default().fg(Color::Magenta),
            ),
            Span::styled(label, Style::default().add_modifier(Modifier::BOLD)),
            Span::styled(
                format!("  {}s", app.tick / 8),
                Style::default().add_modifier(Modifier::DIM),
            ),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled("esc", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(" interrupt   "),
            Span::styled("ctrl+t", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(" transcript   "),
            Span::styled("?", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(" shortcuts"),
        ]),
        Line::from(""),
        Line::styled(
            "/home/t4ce/Repos/TRUEOS  •  local demo  •  82% context left",
            Style::default().fg(Color::DarkGray),
        ),
        Line::from(format!(
            "Variable-width status: {}",
            if app.tick % 16 < 8 {
                "Executing a much longer operation label"
            } else {
                "Done"
            }
        )),
        Line::from("The short label must erase all old trailing characters."),
    ];
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .block(Block::bordered().title(" Animated status / footer ")),
        area,
    );
}

fn transcript(frame: &mut Frame, area: Rect, app: &App) {
    let answer = "I found the terminal rendering path. The parser handles colors and underlines. Inverse selections must swap resolved colors, and unsupported keyboard capability queries must leave the cursor alone. We can test these patterns independently before combining them with a streaming transcript and popup.";
    let count = (app.tick as usize * 3) % (answer.len() + 80);
    let stream = &answer[..count.min(answer.len())];
    let mut lines = vec![
        Line::styled(
            "› Inspect terminal compatibility",
            Style::default().fg(Color::Cyan),
        ),
        Line::from(""),
        Line::styled("• Working", Style::default().add_modifier(Modifier::DIM)),
        Line::from("  ├ Read trueos-terminal/src/lib.rs"),
        Line::from("  └ Read tui/src/custom_terminal.rs"),
        Line::from(""),
        Line::from(stream),
        Line::from(""),
        Line::from(vec![
            Span::raw("Selection: "),
            Span::styled(
                "foreground / background",
                Style::default().add_modifier(Modifier::REVERSED),
            ),
            Span::raw(" normal again"),
        ]),
        Line::from("  preserved indentation"),
        Line::from(
            "long_token_without_spaces_abcdefghijklmnopqrstuvwxyz_0123456789_abcdefghijklmnopqrstuvwxyz",
        ),
        Line::from("Widths: [日本語] [🙂] [e\u{301}] [👩‍💻] right edge →"),
    ];
    for text in app.submissions.iter().rev().take(6) {
        lines.push(Line::from(format!("› {text}")));
    }
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .scroll((app.scroll, 0))
            .block(Block::bordered().title(" Streaming transcript / PgUp PgDn ")),
        area,
    );
    if area.width > 2 && area.height > 2 {
        let mut scroll = ScrollbarState::new(30).position(app.scroll as usize);
        frame.render_stateful_widget(
            Scrollbar::new(ScrollbarOrientation::VerticalRight),
            area,
            &mut scroll,
        );
    }
}

fn combined(frame: &mut Frame, area: Rect, app: &App) {
    let [history, state, input] = Layout::vertical([
        Constraint::Min(0),
        Constraint::Length(3),
        Constraint::Length(5),
    ])
    .areas(area);
    transcript(frame, history, app);
    status(frame, state, app);
    composer(frame, input, app);
    if app.popup {
        chooser(frame, area, app);
    }
}

fn chooser(frame: &mut Frame, area: Rect, app: &App) {
    let popup = centered(area, 64, 13);
    frame.render_widget(Clear, popup);
    let block = Block::bordered()
        .title(" Approval / command chooser ")
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(Color::Cyan));
    let inner = block.inner(popup);
    frame.render_widget(block, popup);
    let choices = [
        "Allow this action once",
        "Allow this command for the session",
        "Reject and give feedback",
        "Open src/terminal.rs",
        "Search transcript",
        "Choose a model",
        "Attach @README.md",
        "Review the diff",
    ];
    let selected = app.selected % choices.len();
    let visible = inner.height.saturating_sub(2) as usize;
    let offset = selected.saturating_sub(visible.saturating_sub(1));
    let mut lines = vec![
        Line::styled(
            "↑/↓ choose a simulated action",
            Style::default().add_modifier(Modifier::DIM),
        ),
        Line::from(""),
    ];
    for (index, choice) in choices.iter().enumerate().skip(offset).take(visible) {
        lines.push(Line::styled(
            format!("{} {}", if index == selected { "›" } else { " " }, choice),
            if index == selected {
                Style::default().add_modifier(Modifier::REVERSED)
            } else {
                Style::default()
            },
        ));
    }
    frame.render_widget(Paragraph::new(lines), inner);
}

pub fn help(frame: &mut Frame, area: Rect) {
    let popup = centered(area, 60, 12);
    frame.render_widget(Clear, popup);
    frame.render_widget(Paragraph::new("Ratatest help\n\n1..4: choose section\nTab: next case; Shift-Tab: previous\ni: edit; Enter: submit\no: toggle this overlay\nSpace: freeze animation\nF10: park and return to Shell3\nq: quit\n\nClosing this dialog must restore covered text.").wrap(Wrap { trim: false }).block(Block::bordered().title(" Keyboard help ")), popup);
}

pub fn centered(area: Rect, width: u16, height: u16) -> Rect {
    let width = width.min(area.width);
    let height = height.min(area.height);
    Rect::new(
        area.x + (area.width - width) / 2,
        area.y + (area.height - height) / 2,
        width,
        height,
    )
}

fn repaint(frame: &mut Frame, area: Rect, app: &App) {
    let [specimens, cells] =
        Layout::vertical([Constraint::Min(0), Constraint::Length(4)]).areas(area);
    let long = app.tick % 16 < 8;
    let text = if long {
        "A long line which must disappear after the short replacement"
    } else {
        "short"
    };
    let wide = if long { "日本語🙂" } else { "ascii" };
    let lines = vec![
        Line::from(text),
        Line::from(wide),
        Line::from(vec![
            Span::raw("background ["),
            Span::styled(
                if long { "                " } else { "   " },
                Style::default().bg(Color::Blue),
            ),
            Span::raw("] reset"),
        ]),
        Line::from("Combining: e\u{301}  a\u{308}  👩‍💻  🇩🇪"),
        Line::from("0123456789012345678901234567890123456789"),
        Line::from("Wide: [日本語] narrow: [ABCDEF] both occupy six cells"),
        Line::from("SGR reset after inverse: "),
        Line::from(vec![
            Span::styled(
                " inverted ",
                Style::default()
                    .fg(Color::Yellow)
                    .bg(Color::Blue)
                    .add_modifier(Modifier::REVERSED),
            ),
            Span::raw(" plain "),
            Span::styled(
                " underlined ",
                Style::default().add_modifier(Modifier::UNDERLINED),
            ),
        ]),
    ];
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .block(Block::bordered().title(" Replace long ↔ short / wide ↔ narrow ")),
        specimens,
    );
    frame.render_widget(BufferProbe { tick: app.tick }, cells);
}

struct BufferProbe {
    tick: u64,
}
impl Widget for BufferProbe {
    fn render(self, area: Rect, buffer: &mut Buffer) {
        let labels = [
            "Custom Widget / direct Buffer",
            "AlwaysUpdate / ForcedWidth / Skip specimen",
        ];
        for (row, text) in labels.into_iter().enumerate().take(area.height as usize) {
            buffer.set_stringn(
                area.x,
                area.y + row as u16,
                text,
                area.width as usize,
                Style::default().fg(Color::Magenta),
            );
        }
        if area.height > 2 && area.width > 12 {
            let cell = &mut buffer[(area.x, area.y + 2)];
            cell.set_symbol(if self.tick % 2 == 0 { "#" } else { "+" });
            cell.diff_option = CellDiffOption::AlwaysUpdate;
            buffer[(area.x + 3, area.y + 2)]
                .set_symbol("界")
                .diff_option = CellDiffOption::ForcedWidth(std::num::NonZeroU16::new(2).unwrap());
            buffer[(area.x + 4, area.y + 2)].diff_option = CellDiffOption::Skip;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn composer_tail_respects_graphemes_and_display_width() {
        assert_eq!(visible_tail("ab日本", 4), ("日本", 4));
        assert_eq!(visible_tail("abe\u{301}", 1), ("e\u{301}", 1));
        assert_eq!(visible_tail("hello", 0), ("", 0));
    }
}
