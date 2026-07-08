use std::sync::atomic::Ordering;

use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    symbols::border,
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
};

use super::{COMMAND_FG, FLASH_BG, FLASH_FG, HOTKEY_FINAL_BG, HOTKEY_STATE};

const COMPACT_CELL_WIDTH: usize = 6;
const ARG_FG: Color = Color::Rgb(105, 105, 105);

pub fn draw_compact(frame: &mut Frame, area: Rect) {
    let lines = vec![
        compact_row(
            "File",
            &[("l", "⏏"), ("pl", "▶"), ("pa", "⏸"), ("ma", "🪟")],
        ),
        compact_row(
            "Seek",
            &[
                ("n", "⏭"),
                ("pr", "⏮"),
                ("go", "🔽"),
                ("la", "⏱"),
                ("lo", "🔁"),
            ],
        ),
        compact_row("Vol", &[("-", "🔉"), ("+", "🔊"), ("m", "🔇"), ("u", "🔈")]),
        compact_row(
            "Edit",
            &[
                ("ga", "🎚"),
                ("pi", "🎛"),
                ("c", "✂"),
                ("re", "⏺"),
                ("mi", "🎙"),
                ("s", "💾"),
            ],
        ),
        compact_row(
            "Misc",
            &[
                ("pl", "📋"),
                ("w", "📻"),
                ("sc", "〰"),
                ("sp", "▥"),
                ("tr", "📄"),
            ],
        ),
    ];

    let block = Block::default()
        .title("─cmds ─")
        .title_style(plain_style().fg(COMMAND_FG))
        .borders(Borders::ALL)
        .border_set(border::ROUNDED)
        .border_style(Style::default().fg(Color::DarkGray));
    frame.render_widget(Paragraph::new(lines).block(block), area);
}

pub fn draw(frame: &mut Frame, area: Rect) {
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(21),
            Constraint::Percentage(26),
            Constraint::Length(12),
            Constraint::Percentage(24),
            Constraint::Percentage(29),
        ])
        .split(area);

    let file_width = cols[0].width.saturating_sub(2);
    let seek_width = cols[1].width.saturating_sub(2);
    let vol_width = cols[2].width.saturating_sub(2);
    let edit_width = cols[3].width.saturating_sub(2);
    let misc_width = cols[4].width.saturating_sub(2);

    let groups = [
        (
            "File",
            vec![
                icon_column_line(
                    [keyed_command("load"), vec![arg_span(" <path>")]].concat(),
                    "⏏",
                    first_icon_column_width(file_width),
                ),
                dual_icon_line(
                    keyed_command("play"),
                    "▶",
                    keyed_command("pause"),
                    "⏸",
                    file_width,
                ),
                dual_icon_line(
                    keyed_command("quit"),
                    "✖",
                    keyed_command("terminate"),
                    "⏹",
                    file_width,
                ),
                icon_column_line(
                    keyed_command("mainview"),
                    "🪟",
                    first_icon_column_width(file_width),
                ),
            ],
        ),
        (
            "Seek",
            vec![
                dual_icon_line(
                    keyed_command("next"),
                    "⏭",
                    keyed_command("prev"),
                    "⏮",
                    seek_width,
                ),
                icon_column_line(
                    [
                        keyed_command("goto"),
                        vec![arg_span(" < "), icon_span("⏱"), arg_span(" >")],
                    ]
                    .concat(),
                    "🔽",
                    first_icon_column_width(seek_width),
                ),
                icon_column_line(
                    [keyed_command("label"), vec![arg_span(" <mm:ss>")]].concat(),
                    "⏱",
                    first_icon_column_width(seek_width),
                ),
                icon_column_line(
                    [
                        keyed_command("loop"),
                        vec![
                            arg_span(" < "),
                            icon_span("⏱"),
                            arg_span(", "),
                            icon_span("⏱"),
                            arg_span(" >"),
                        ],
                    ]
                    .concat(),
                    "🔁",
                    first_icon_column_width(seek_width),
                ),
            ],
        ),
        (
            "Vol",
            vec![
                right_icon_line(vec![hotkey_span("-")], "🔉", vol_width),
                right_icon_line(vec![hotkey_span("+")], "🔊", vol_width),
                right_icon_line(keyed_command("mute"), "🔇", vol_width),
                right_icon_line(keyed_command("unmute"), "🔈", vol_width),
            ],
        ),
        (
            "Edit",
            vec![
                dual_icon_line(
                    [keyed_command("gain"), vec![arg_span(" <db>")]].concat(),
                    "🎚",
                    keyed_command("pitch"),
                    "🎛",
                    edit_width,
                ),
                icon_column_line(
                    [
                        keyed_command("cut"),
                        vec![
                            arg_span(" < "),
                            icon_span("⏱"),
                            arg_span(", "),
                            icon_span("⏱"),
                            arg_span(" >"),
                        ],
                    ]
                    .concat(),
                    "✂",
                    first_icon_column_width(edit_width),
                ),
                dual_icon_line(
                    keyed_command("rec"),
                    "⏺",
                    keyed_command("mic"),
                    "🎙",
                    edit_width,
                ),
                icon_column_line(
                    [keyed_command("save"), vec![arg_span(" <path>")]].concat(),
                    "💾",
                    first_icon_column_width(edit_width),
                ),
            ],
        ),
        (
            "Misc",
            vec![
                icon_column_line(
                    keyed_command("playlist"),
                    "📋",
                    first_icon_column_width(misc_width),
                ),
                icon_column_line(
                    keyed_command("webradio"),
                    "📻",
                    first_icon_column_width(misc_width),
                ),
                dual_icon_line(
                    keyed_command("scope"),
                    "〰",
                    keyed_command("spectro"),
                    "▥",
                    misc_width,
                ),
                icon_column_line(
                    keyed_command("transcribe"),
                    "📄",
                    first_icon_column_width(misc_width),
                ),
            ],
        ),
    ];

    for (idx, (title, lines)) in groups.iter().enumerate() {
        let title = if idx >= 3 {
            Line::from(format!("─ {title}─")).right_aligned()
        } else {
            Line::from(format!("─{title} ─"))
        };
        let block = Block::default()
            .title(title)
            .title_style(block_title_style())
            .borders(Borders::ALL)
            .border_set(border::ROUNDED)
            .border_style(Style::default().fg(Color::DarkGray));
        let paragraph = Paragraph::new(lines.clone())
            .block(block)
            .style(Style::default().fg(Color::Gray));
        frame.render_widget(paragraph, cols[idx]);
    }
}

fn compact_row(caption: &'static str, items: &[(&'static str, &'static str)]) -> Line<'static> {
    let mut spans = vec![compact_caption_span(caption), Span::raw("  ")];

    for (idx, (alias, icon)) in items.iter().enumerate() {
        if idx > 0 {
            spans.push(split_separator_span());
        }
        spans.extend(compact_item_spans(alias, icon));
    }

    Line::from(spans)
}

fn compact_caption_span(text: &'static str) -> Span<'static> {
    Span::styled(format!("{text:<4}"), plain_style().fg(COMMAND_FG))
}

fn compact_item_spans(alias: &'static str, icon: &'static str) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    let alias_width = str_target_width(alias);
    if alias_width < 2 {
        spans.push(Span::raw(" ".repeat(2 - alias_width)));
    }
    spans.push(hotkey_span(alias));
    spans.push(Span::raw(" "));
    spans.push(icon_span(icon));

    let width = spans_target_width(&spans);
    if width < COMPACT_CELL_WIDTH {
        spans.push(Span::raw(" ".repeat(COMPACT_CELL_WIDTH - width)));
    }

    spans
}

fn hotkey_span(text: &'static str) -> Span<'static> {
    let style = match HOTKEY_STATE.load(Ordering::Relaxed) {
        1 => plain_style()
            .fg(FLASH_FG)
            .bg(FLASH_BG)
            .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
        2 => plain_style()
            .fg(COMMAND_FG)
            .bg(HOTKEY_FINAL_BG)
            .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
        _ => plain_style()
            .fg(COMMAND_FG)
            .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
    };

    Span::styled(text, style)
}

fn icon_span(text: &'static str) -> Span<'static> {
    Span::styled(text, plain_style().fg(COMMAND_FG))
}

fn split_separator_span() -> Span<'static> {
    Span::styled("┊", plain_style().fg(Color::DarkGray))
}

fn command_span(text: &'static str) -> Span<'static> {
    Span::styled(text, plain_style().fg(COMMAND_FG))
}

fn keyed_command(command: &'static str) -> Vec<Span<'static>> {
    let hotkey_len = command_hotkey_len(command);
    let split = command
        .char_indices()
        .nth(hotkey_len)
        .map(|(idx, _)| idx)
        .unwrap_or(command.len());
    let (hotkey, rest) = command.split_at(split);

    vec![hotkey_span(hotkey), command_span(rest)]
}

fn command_hotkey_len(command: &str) -> usize {
    let word_len = command
        .chars()
        .take_while(|ch| ch.is_alphanumeric())
        .count();
    let preferred = match command.trim_end() {
        "play" | "pause" | "label" | "loop" | "scope" | "spectro" => 2,
        _ => 1,
    };

    preferred.min(word_len).max(1)
}

fn block_title_style() -> Style {
    Style::default().fg(COMMAND_FG).add_modifier(Modifier::BOLD)
}

fn plain_style() -> Style {
    Style::default().remove_modifier(Modifier::BOLD | Modifier::UNDERLINED | Modifier::REVERSED)
}

fn span_target_width(span: &Span<'_>) -> usize {
    span.content.chars().count()
}

fn spans_target_width(spans: &[Span<'_>]) -> usize {
    spans.iter().map(span_target_width).sum()
}

fn str_target_width(text: &str) -> usize {
    text.chars().count()
}

fn right_icon_line(
    mut content: Vec<Span<'static>>,
    icon: &'static str,
    inner_width: u16,
) -> Line<'static> {
    let content_width = spans_target_width(&content);
    let icon = icon_span(icon);
    let icon_width = span_target_width(&icon);
    let gap = usize::from(inner_width).saturating_sub(content_width + icon_width);

    content.push(Span::raw(" ".repeat(gap)));
    content.push(icon);
    Line::from(content)
}

fn icon_column_line(
    mut content: Vec<Span<'static>>,
    icon: &'static str,
    column_width: u16,
) -> Line<'static> {
    let content_width = spans_target_width(&content);
    let icon = icon_span(icon);
    let icon_width = span_target_width(&icon);
    let gap = usize::from(column_width)
        .saturating_sub(content_width + icon_width)
        .max(1);

    content.push(Span::raw(" ".repeat(gap)));
    content.push(icon);
    content.push(split_separator_span());
    Line::from(content)
}

fn first_icon_column_width(inner_width: u16) -> u16 {
    inner_width.saturating_sub(1) / 2
}

fn dual_icon_line(
    left: Vec<Span<'static>>,
    left_icon: &'static str,
    right: Vec<Span<'static>>,
    right_icon: &'static str,
    inner_width: u16,
) -> Line<'static> {
    let available = inner_width.saturating_sub(1);
    let left_width = available / 2;
    dual_icon_line_split(left, left_icon, right, right_icon, left_width, inner_width)
}

fn dual_icon_line_split(
    left: Vec<Span<'static>>,
    left_icon: &'static str,
    right: Vec<Span<'static>>,
    right_icon: &'static str,
    left_width: u16,
    inner_width: u16,
) -> Line<'static> {
    let available = inner_width.saturating_sub(1);
    let left_width = left_width.min(available);
    let right_width = available.saturating_sub(left_width);
    let mut spans = right_icon_line(left, left_icon, left_width).spans;
    spans.push(split_separator_span());
    spans.extend(right_icon_line(right, right_icon, right_width).spans);
    Line::from(spans)
}

fn arg_span(text: &'static str) -> Span<'static> {
    Span::styled(text, plain_style().fg(ARG_FG))
}
