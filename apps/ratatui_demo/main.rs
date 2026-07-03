use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Gauge, Paragraph, Widget},
};
use trueos::logl::{self, level};

const WIDTH: u16 = 48;
const HEIGHT: u16 = 12;

fn main() {
    logl::log(level::INFO, format_args!("ratatui_demo: start"));

    let mut buffer = Buffer::empty(Rect::new(0, 0, WIDTH, HEIGHT));
    render_shell_model(&mut buffer);
    log_buffer(&buffer);

    match validate_render(&buffer) {
        Ok(()) => logl::log(level::INFO, format_args!("ratatui_demo: done")),
        Err(stage) => logl::log(
            level::ERROR,
            format_args!("ratatui_demo: failed stage={}", stage),
        ),
    }
}

fn render_shell_model(buffer: &mut Buffer) {
    let area = buffer.area;
    let chunks = Layout::vertical([
        Constraint::Length(3),
        Constraint::Length(5),
        Constraint::Length(3),
    ])
    .split(area);

    Paragraph::new(Line::from(vec![
        Span::styled(
            "TRUEOS",
            Style::new()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" kernel shell model"),
    ]))
    .block(Block::new().title("ratatui").borders(Borders::ALL))
    .render(chunks[0], buffer);

    Paragraph::new(vec![
        Line::from("prompt  / > apps"),
        Line::from("select  ratatui_demo"),
        Line::from("render  buffer-only tui probe"),
    ])
    .block(Block::new().title("shell").borders(Borders::ALL))
    .render(chunks[1], buffer);

    Gauge::default()
        .block(Block::new().title("model fit").borders(Borders::ALL))
        .gauge_style(Style::new().fg(Color::Green))
        .ratio(0.72)
        .label("ratatui vendored")
        .render(chunks[2], buffer);
}

fn log_buffer(buffer: &Buffer) {
    for y in 0..buffer.area.height {
        let mut row = String::new();
        for x in 0..buffer.area.width {
            row.push_str(buffer[(x, y)].symbol());
        }
        logl::log(level::INFO, format_args!("ratatui_demo: |{}|", row));
    }
}

fn validate_render(buffer: &Buffer) -> Result<(), &'static str> {
    let rendered = buffer_text(buffer);
    if !rendered.contains("TRUEOS kernel shell model") {
        return Err("title.text");
    }
    if !rendered.contains("buffer-only tui probe") {
        return Err("body.text");
    }
    if !rendered.contains("ratatui vendored") {
        return Err("gauge.label");
    }
    Ok(())
}

fn buffer_text(buffer: &Buffer) -> String {
    let mut rendered = String::new();
    for y in 0..buffer.area.height {
        for x in 0..buffer.area.width {
            rendered.push_str(buffer[(x, y)].symbol());
        }
        rendered.push('\n');
    }
    rendered
}
