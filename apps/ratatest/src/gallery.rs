//! A stock Ratatui gallery. Keep platform-specific terminal probes in the main app.

use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    symbols::{self, Marker},
    text::{Line, Span},
    widgets::{
        Axis, Bar, BarChart, BarGroup, Block, BorderType, Cell, Chart, Clear, Dataset, Fill, Gauge,
        GraphType, LineGauge, List, ListItem, ListState, MascotEyeColor, Padding, Paragraph,
        RatatuiLogo, RatatuiMascot, Row, Scrollbar, ScrollbarOrientation, ScrollbarState,
        Sparkline, Table, TableState, Tabs, Wrap,
        calendar::{CalendarEventStore, Monthly},
        canvas::{
            Canvas, Circle, FilledLine, Line as CanvasLine, Map, MapResolution, Points, Rectangle,
        },
    },
};
use time::{Date, Month};

pub const PAGES: &[(&str, &str)] = &[
    (
        "Blocks & text",
        "Borders, padding, titles, wrapping, scrolling and Tabs",
    ),
    (
        "Lists & tables",
        "Stateful selection, multi-line rows and both scrollbar directions",
    ),
    (
        "Progress & activity",
        "Gauge, LineGauge, Sparkline and animated fractional cells",
    ),
    (
        "Charts",
        "Line, scatter and bar datasets; axes, legends and grouped bars",
    ),
    (
        "Canvas & markers",
        "Every canvas shape and all nine marker types",
    ),
    (
        "Colors & text",
        "ANSI, indexed and RGB colors; every modifier; Unicode widths",
    ),
    (
        "Calendar & layers",
        "Monthly events, surrounding dates and Clear modal layering",
    ),
    (
        "Fill & artwork",
        "Repeated-cell Fill, both RatatuiLogo sizes and RatatuiMascot half blocks",
    ),
];

const ACCENT: Color = Color::Cyan;
const DIM: Color = Color::DarkGray;
const SELECTED: Style = Style::new().fg(Color::Black).bg(Color::Cyan);

pub fn render(frame: &mut Frame, area: Rect, page: usize, tick: u64, selected: usize) {
    if area.is_empty() {
        return;
    }
    match page % PAGES.len() {
        0 => blocks(frame, area, selected),
        1 => collections(frame, area, selected),
        2 => progress(frame, area, tick),
        3 => charts(frame, area, tick, selected),
        4 => canvas(frame, area, tick, selected),
        5 => colors(frame, area, selected),
        6 => calendar(frame, area, selected),
        _ => artwork(frame, area, selected),
    }
}

fn panel(title: impl Into<Line<'static>>) -> Block<'static> {
    Block::bordered()
        .title(title)
        .border_style(Style::new().fg(DIM))
}

fn blocks(frame: &mut Frame, area: Rect, selected: usize) {
    let rows = Layout::vertical([
        Constraint::Length(3),
        Constraint::Length(if area.height >= 23 { 8 } else { 6 }),
        Constraint::Min(3),
    ])
    .split(area);
    frame.render_widget(
        Tabs::new(["Overview", "Editor", "Review", "Settings"])
            .select(selected % 4)
            .block(panel("Tabs · selection follows ↑ / ↓"))
            .highlight_style(SELECTED)
            .divider(" │ "),
        rows[0],
    );

    let borders = [
        BorderType::Plain,
        BorderType::Rounded,
        BorderType::Double,
        BorderType::Thick,
        BorderType::LightDoubleDashed,
        BorderType::HeavyDoubleDashed,
        BorderType::LightTripleDashed,
        BorderType::HeavyTripleDashed,
        BorderType::LightQuadrupleDashed,
        BorderType::HeavyQuadrupleDashed,
        BorderType::QuadrantInside,
        BorderType::QuadrantOutside,
    ];
    let columns = Layout::horizontal([Constraint::Ratio(1, 3); 3]).split(rows[1]);
    for (i, rect) in columns.iter().enumerate() {
        let border = if i == 2 {
            borders[selected % borders.len()]
        } else {
            borders[i]
        };
        let content = match i {
            0 => "Left aligned\nTop + bottom titles",
            1 => "Centered\nPadded content",
            _ => "↑ / ↓ cycles all\n12 border styles",
        };
        let block = Block::bordered()
            .border_type(border)
            .border_style(Style::new().fg([Color::Cyan, Color::Green, Color::Yellow][i]))
            .title_top(format!("{border}"))
            .title_bottom(Line::from("bottom title").right_aligned())
            .padding(Padding::horizontal(u16::from(i == 1)));
        frame.render_widget(
            Paragraph::new(content)
                .block(block)
                .alignment(if i == 1 {
                    Alignment::Center
                } else {
                    Alignment::Left
                })
                .wrap(Wrap { trim: false }),
            *rect,
        );
    }

    let columns =
        Layout::horizontal([Constraint::Percentage(58), Constraint::Percentage(42)]).split(rows[2]);
    let text = vec![
        Line::from(vec![
            Span::styled(
                "Paragraph: ",
                Style::new().fg(ACCENT).add_modifier(Modifier::BOLD),
            ),
            Span::raw("mixed styles keep their colors while wrapping across the available width."),
        ]),
        Line::from(
            "  Indentation is preserved with trim=false. Resize the terminal to see reflow.",
        ),
        Line::from(vec![
            Span::raw("Links and paths: "),
            Span::styled(
                "src/main.rs:42",
                Style::new()
                    .fg(Color::Blue)
                    .add_modifier(Modifier::UNDERLINED),
            ),
        ]),
        Line::from("Unicode: café  e\u{301}  東京  🦀  →  ───"),
        Line::from("This last line is reachable when the paragraph scrolls."),
    ];
    frame.render_widget(
        Paragraph::new(text)
            .block(panel("Wrapped · vertical scroll ↑ / ↓"))
            .wrap(Wrap { trim: false })
            .scroll(((selected % 5) as u16, 0)),
        columns[0],
    );

    let offset = selected % 36;
    let long = "0000000000111111111122222222223333333333444444444455555555556666666666\n0123456789012345678901234567890123456789012345678901234567890123456789\nA long unwrapped log entry should clip at the panel edge, never overwrite it.";
    frame.render_widget(
        Paragraph::new(long)
            .block(panel("Unwrapped · horizontal scroll"))
            .scroll((0, offset as u16)),
        columns[1],
    );
    let mut scroll = ScrollbarState::new(70)
        .position(offset)
        .viewport_content_length(usize::from(columns[1].width.saturating_sub(2)));
    frame.render_stateful_widget(
        Scrollbar::new(ScrollbarOrientation::HorizontalBottom)
            .begin_symbol(Some("◀"))
            .end_symbol(Some("▶")),
        columns[1].inner(ratatui::layout::Margin::new(1, 0)),
        &mut scroll,
    );
}

fn collections(frame: &mut Frame, area: Rect, selected: usize) {
    let cols =
        Layout::horizontal([Constraint::Percentage(38), Constraint::Percentage(62)]).split(area);
    let names = [
        "Plan changes",
        "Read workspace",
        "Inspect terminal",
        "Build blueprint",
        "Deploy app",
        "Check wrapping",
        "Check wide cells",
        "Check selection",
        "Check scrolling",
        "Review colors",
        "Capture output",
        "Compare frames",
        "Verify resize",
        "Inspect cleanup",
        "Report results",
    ];
    let index = selected % names.len();
    let items: Vec<_> = names
        .iter()
        .enumerate()
        .map(|(i, name)| {
            ListItem::new(vec![
                Line::from(vec![
                    Span::styled(format!("{:02} ", i + 1), Style::new().fg(DIM)),
                    Span::raw(*name),
                ]),
                Line::styled(
                    if i % 3 == 0 {
                        "   queued · multi-line item"
                    } else {
                        "   ready"
                    },
                    Style::new().fg(DIM),
                ),
            ])
        })
        .collect();
    let mut state = ListState::default().with_selected(Some(index));
    frame.render_stateful_widget(
        List::new(items)
            .block(panel("List · ↑ / ↓"))
            .highlight_style(SELECTED)
            .highlight_symbol("▶ ")
            .repeat_highlight_symbol(true),
        cols[0],
        &mut state,
    );
    let mut scroll = ScrollbarState::new(names.len() * 2)
        .position(state.offset() * 2)
        .viewport_content_length(usize::from(cols[0].height.saturating_sub(2)));
    frame.render_stateful_widget(
        Scrollbar::new(ScrollbarOrientation::VerticalRight),
        cols[0].inner(ratatui::layout::Margin::new(0, 1)),
        &mut scroll,
    );

    let rows = (0..24).map(|i| {
        let status = ["ready", "working", "passed", "waiting"][i % 4];
        Row::new(vec![
            Cell::from(format!("task-{:02}", i + 1)),
            Cell::from(status).style(Style::new().fg(if i % 4 == 2 {
                Color::Green
            } else {
                Color::Yellow
            })),
            Cell::from(format!("{} ms\n{} cells", 12 + i * 7, 80 + i * 11)),
        ])
        .height(2)
    });
    let mut state = TableState::default()
        .with_selected(Some(selected % 24))
        .with_selected_column(Some((selected / 24) % 3));
    let header = Row::new(["Task", "Status", "Render work"])
        .style(Style::new().fg(ACCENT).add_modifier(Modifier::BOLD))
        .bottom_margin(1);
    let table = Table::new(
        rows,
        [
            Constraint::Length(10),
            Constraint::Length(10),
            Constraint::Min(8),
        ],
    )
    .block(panel("Table · row / column / cell highlights"))
    .header(header)
    .footer(Row::new(["24 tasks", "4 states", "2-line rows"]).style(Style::new().fg(DIM)))
    .column_spacing(1)
    .row_highlight_style(Style::new().bg(Color::DarkGray))
    .column_highlight_style(Style::new().add_modifier(Modifier::UNDERLINED))
    .cell_highlight_style(SELECTED)
    .highlight_symbol("▶ ");
    frame.render_stateful_widget(table, cols[1], &mut state);
    let mut scroll = ScrollbarState::new(48)
        .position(state.offset() * 2)
        .viewport_content_length(usize::from(cols[1].height.saturating_sub(5)));
    frame.render_stateful_widget(
        Scrollbar::new(ScrollbarOrientation::VerticalRight),
        cols[1].inner(ratatui::layout::Margin::new(0, 1)),
        &mut scroll,
    );
}

fn progress(frame: &mut Frame, area: Rect, tick: u64) {
    let rows = Layout::vertical([
        Constraint::Length(4),
        Constraint::Length(3),
        Constraint::Length(3),
        Constraint::Min(3),
    ])
    .split(area);
    let ratio = (tick % 201) as f64 / 200.0;
    frame.render_widget(
        Gauge::default()
            .block(panel("Gauge · fractional Unicode cells"))
            .ratio(ratio)
            .use_unicode(true)
            .gauge_style(Style::new().fg(Color::Cyan).bg(Color::DarkGray))
            .label(format!("Build artifacts  {:5.1}%", ratio * 100.0)),
        rows[0],
    );
    frame.render_widget(
        LineGauge::default()
            .block(panel("LineGauge · separate label and fill styles"))
            .ratio(ratio)
            .label(format!("{}/200", tick % 201))
            .filled_style(Style::new().fg(Color::Green))
            .unfilled_style(Style::new().fg(DIM))
            .filled_symbol(symbols::line::THICK.horizontal)
            .unfilled_symbol(symbols::line::NORMAL.horizontal),
        rows[1],
    );
    let fixed = Layout::horizontal([Constraint::Ratio(1, 3); 3]).split(rows[2]);
    for (i, p) in [0, 50, 100].into_iter().enumerate() {
        frame.render_widget(
            Gauge::default()
                .block(panel(format!("Boundary {p}%")))
                .percent(p)
                .label(format!("{p}%"))
                .gauge_style(Style::new().fg([Color::Red, Color::Yellow, Color::Green][i])),
            fixed[i],
        );
    }
    let cols = Layout::horizontal([Constraint::Ratio(1, 2); 2]).split(rows[3]);
    let samples: Vec<u64> = (0..160)
        .map(|i| (((i + tick) as f64 / 5.0).sin().abs() * 80.0) as u64 + i % 17)
        .collect();
    frame.render_widget(
        Sparkline::default()
            .block(panel("Sparkline · left to right"))
            .data(&samples)
            .max(100)
            .style(Style::new().fg(Color::Cyan)),
        cols[0],
    );
    frame.render_widget(
        Sparkline::default()
            .block(panel("Sparkline · right to left"))
            .data(&samples)
            .max(100)
            .direction(ratatui::widgets::RenderDirection::RightToLeft)
            .style(Style::new().fg(Color::Magenta)),
        cols[1],
    );
}

fn charts(frame: &mut Frame, area: Rect, tick: u64, selected: usize) {
    let rows =
        Layout::vertical([Constraint::Percentage(60), Constraint::Percentage(40)]).split(area);
    let phase = (tick % 1000) as f64 / 12.0;
    let wave: Vec<_> = (0..81)
        .map(|i| {
            let x = f64::from(i) / 8.0;
            (x, (x + phase).sin() * 0.75)
        })
        .collect();
    let points: Vec<_> = (0..11)
        .map(|i| {
            let x = f64::from(i);
            (x, (x * 0.8 + phase).cos() * 0.55)
        })
        .collect();
    let bars = [
        (1.0, 0.45),
        (3.0, -0.45),
        (5.0, 0.65),
        (7.0, -0.65),
        (9.0, 0.35),
    ];
    let marker = markers()[selected % markers().len()].1;
    let filled: Vec<_> = wave.iter().map(|(x, y)| (*x, y * 0.2 - 0.7)).collect();
    let datasets = vec![
        Dataset::default()
            .name("area")
            .data(&filled)
            .graph_type(GraphType::Area)
            .fill_to_y(-1.0)
            .marker(Marker::HalfBlock)
            .style(Style::new().fg(Color::Blue)),
        Dataset::default()
            .name("line")
            .data(&wave)
            .graph_type(GraphType::Line)
            .marker(marker)
            .style(Style::new().fg(Color::Cyan)),
        Dataset::default()
            .name("scatter")
            .data(&points)
            .graph_type(GraphType::Scatter)
            .marker(Marker::Dot)
            .style(Style::new().fg(Color::Yellow)),
        Dataset::default()
            .name("bars")
            .data(&bars)
            .graph_type(GraphType::Bar)
            .marker(Marker::HalfBlock)
            .style(Style::new().fg(Color::Magenta)),
    ];
    frame.render_widget(
        Chart::new(datasets)
            .block(panel(format!("Chart · {marker} · ↑ / ↓ changes marker")))
            .x_axis(
                Axis::default()
                    .title("time / s")
                    .bounds([0.0, 10.0])
                    .labels(["0", "5", "10"])
                    .style(Style::new().fg(DIM)),
            )
            .y_axis(
                Axis::default()
                    .title("signal")
                    .bounds([-1.0, 1.0])
                    .labels(["−1", "0", "+1"])
                    .style(Style::new().fg(DIM)),
            )
            .hidden_legend_constraints((Constraint::Percentage(60), Constraint::Percentage(60))),
        rows[0],
    );
    let cols =
        Layout::horizontal([Constraint::Percentage(55), Constraint::Percentage(45)]).split(rows[1]);
    let group = |name: &'static str, color, shift: u64| {
        BarGroup::default().label(Line::from(name)).bars(&[
            Bar::with_label("CPU", 35 + shift).style(Style::new().fg(color)),
            Bar::with_label("I/O", 65 - shift).style(Style::new().fg(color)),
        ])
    };
    frame.render_widget(
        BarChart::default()
            .block(panel("BarChart · groups"))
            .data(group("before", Color::Blue, 0))
            .data(group("after", Color::Cyan, 15))
            .bar_width(3)
            .bar_gap(1)
            .group_gap(3)
            .max(100)
            .value_style(Style::new().fg(Color::Black)),
        cols[0],
    );
    frame.render_widget(
        BarChart::default()
            .block(panel("BarChart · horizontal"))
            .data(&[("read", 78), ("write", 46), ("render", 92)])
            .direction(Direction::Horizontal)
            .bar_width(1)
            .bar_gap(1)
            .max(100)
            .bar_style(Style::new().fg(Color::Green))
            .value_style(Style::new().fg(Color::Black)),
        cols[1],
    );
}

fn markers() -> [(&'static str, Marker); 9] {
    [
        ("Dot", Marker::Dot),
        ("Block", Marker::Block),
        ("Bar", Marker::Bar),
        ("Braille", Marker::Braille),
        ("HalfBlock", Marker::HalfBlock),
        ("Quadrant", Marker::Quadrant),
        ("Sextant", Marker::Sextant),
        ("Octant", Marker::Octant),
        ("Custom +", Marker::Custom('+')),
    ]
}

fn canvas(frame: &mut Frame, area: Rect, tick: u64, selected: usize) {
    let rows = Layout::vertical([
        Constraint::Min(5),
        Constraint::Length(if area.height >= 26 { 12 } else { 9 }),
    ])
    .split(area);
    let cols = Layout::horizontal([Constraint::Ratio(1, 2); 2]).split(rows[0]);
    let marker = markers()[selected % 9].1;
    let position = (tick % 80) as f64 / 10.0;
    frame.render_widget(
        Canvas::default()
            .block(panel(format!("Shapes · {marker}")))
            .marker(marker)
            .x_bounds([0.0, 10.0])
            .y_bounds([0.0, 10.0])
            .paint(|ctx| {
                ctx.draw(&FilledLine::new(0.0, 2.0, 4.0, 4.0, 0.0, Color::Blue));
                ctx.layer();
                ctx.draw(&Rectangle {
                    x: 0.7,
                    y: 1.0,
                    width: 8.0,
                    height: 7.8,
                    color: Color::Cyan,
                });
                ctx.draw(&Circle {
                    x: 5.0,
                    y: 5.0,
                    radius: 2.3,
                    color: Color::Yellow,
                });
                ctx.draw(&CanvasLine {
                    x1: 1.0,
                    y1: 1.0,
                    x2: 9.0,
                    y2: 9.0,
                    color: Color::Green,
                });
                ctx.draw(&Points {
                    coords: &[(1.0 + position, 7.5), (8.2, 1.8)],
                    color: Color::Red,
                });
                ctx.print(
                    1.0,
                    9.8,
                    Line::styled("layered shapes", Style::new().fg(Color::White)),
                );
            }),
        cols[0],
    );
    let resolution = if selected % 2 == 0 {
        MapResolution::High
    } else {
        MapResolution::Low
    };
    frame.render_widget(
        Canvas::default()
            .block(panel(format!("Map · {resolution:?}")))
            .marker(Marker::Braille)
            .x_bounds([-180.0, 180.0])
            .y_bounds([-90.0, 90.0])
            .paint(|ctx| {
                ctx.draw(&Map {
                    color: Color::Green,
                    resolution,
                });
                ctx.layer();
                ctx.draw(&Points {
                    coords: &[(13.4, 52.5), (-122.4, 37.8), (139.7, 35.7)],
                    color: Color::Yellow,
                });
            }),
        cols[1],
    );
    let mini_rows = Layout::vertical([Constraint::Ratio(1, 3); 3]).split(rows[1]);
    for (i, (name, marker)) in markers().into_iter().enumerate() {
        let mini_cols = Layout::horizontal([Constraint::Ratio(1, 3); 3]).split(mini_rows[i / 3]);
        frame.render_widget(
            Canvas::default()
                .block(panel(name))
                .marker(marker)
                .x_bounds([0.0, 10.0])
                .y_bounds([0.0, 4.0])
                .paint(|ctx| {
                    ctx.draw(&CanvasLine {
                        x1: 0.0,
                        y1: 0.0,
                        x2: 10.0,
                        y2: 4.0,
                        color: Color::Cyan,
                    });
                    ctx.draw(&Circle {
                        x: 5.0,
                        y: 2.0,
                        radius: 1.8,
                        color: Color::Yellow,
                    });
                }),
            mini_cols[i % 3],
        );
    }
}

fn colors(frame: &mut Frame, area: Rect, selected: usize) {
    let rows = Layout::vertical([Constraint::Min(5), Constraint::Length(6)]).split(area);
    let cols =
        Layout::horizontal([Constraint::Percentage(60), Constraint::Percentage(40)]).split(rows[0]);
    let block = panel("Color modes · ANSI / 256 indexed / RGB");
    let inner = block.inner(cols[0]);
    frame.render_widget(block, cols[0]);
    let ansi = [
        Color::Black,
        Color::Red,
        Color::Green,
        Color::Yellow,
        Color::Blue,
        Color::Magenta,
        Color::Cyan,
        Color::Gray,
        Color::DarkGray,
        Color::LightRed,
        Color::LightGreen,
        Color::LightYellow,
        Color::LightBlue,
        Color::LightMagenta,
        Color::LightCyan,
        Color::White,
    ];
    let mut lines = vec![
        Line::from("ANSI 0–15"),
        Line::from(
            ansi.into_iter()
                .map(|color| Span::styled("  ", Style::new().bg(color)))
                .collect::<Vec<_>>(),
        ),
    ];
    let count = usize::from(inner.width / 2).clamp(1, 32);
    let available = usize::from(inner.height.saturating_sub(5));
    let total_rows = 256_usize.div_ceil(count);
    let start = if available < total_rows {
        (selected % total_rows) * count
    } else {
        0
    };
    lines.push(Line::from(format!(
        "Indexed {start}–{} · ↑ / ↓ scroll",
        (start + available * count).min(256).saturating_sub(1)
    )));
    for row in 0..available.min(total_rows) {
        let spans = (0..count)
            .filter_map(|column| {
                let index = start + row * count + column;
                (index < 256)
                    .then(|| Span::styled("  ", Style::new().bg(Color::Indexed(index as u8))))
            })
            .collect::<Vec<_>>();
        lines.push(Line::from(spans));
    }
    lines.push(Line::from("Truecolor RGB gradient"));
    lines.push(Line::from(
        (0..count)
            .map(|i| {
                let x = (i * 255 / count.max(2).saturating_sub(1)) as u8;
                Span::styled("  ", Style::new().bg(Color::Rgb(x, 180, 255 - x)))
            })
            .collect::<Vec<_>>(),
    ));
    frame.render_widget(Paragraph::new(lines), inner);

    let samples = [
        ("bold", Modifier::BOLD),
        ("dim", Modifier::DIM),
        ("italic", Modifier::ITALIC),
        ("underlined", Modifier::UNDERLINED),
        ("slow blink", Modifier::SLOW_BLINK),
        ("rapid blink", Modifier::RAPID_BLINK),
        ("reversed", Modifier::REVERSED),
        ("hidden", Modifier::HIDDEN),
        ("crossed out", Modifier::CROSSED_OUT),
    ];
    let mut modifiers: Vec<_> = samples
        .into_iter()
        .map(|(label, modifier)| {
            Line::from(vec![
                Span::raw(format!("{label:12} ")),
                Span::styled("Aa09", Style::new().fg(ACCENT).add_modifier(modifier)),
            ])
        })
        .collect();
    modifiers.push(Line::from(vec![
        Span::raw("combination  "),
        Span::styled(
            "Aa09",
            Style::new()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD | Modifier::ITALIC | Modifier::UNDERLINED),
        ),
    ]));
    modifiers.push(Line::from(vec![
        Span::raw("underline RGB "),
        Span::styled(
            "Aa09",
            Style::new()
                .add_modifier(Modifier::UNDERLINED)
                .underline_color(Color::Rgb(255, 100, 150)),
        ),
    ]));
    modifiers.push(Line::from(vec![
        Span::raw("reset        "),
        Span::styled("default", Style::reset()),
    ]));
    let scroll = if cols[1].height.saturating_sub(2) < modifiers.len() as u16 {
        selected % modifiers.len()
    } else {
        0
    };
    frame.render_widget(
        Paragraph::new(modifiers)
            .block(panel("9 modifiers · ↑ / ↓"))
            .scroll((scroll as u16, 0)),
        cols[1],
    );
    frame.render_widget(
        Paragraph::new(vec![
            Line::from("ASCII | café | combining e\u{301} a\u{308} | CJK 東京 中文 | Arabic مرحبا"),
            Line::from("emoji 🦀 🚀 ✅ | variation ♥ ♥️ | ZWJ 👩‍💻 | flags 🇩🇪 | keycap 1️⃣"),
            Line::from("cells │a│界│e\u{301}│🦀│ end · widths must keep these separators aligned"),
            Line::from("Box ┌─┬─┐ └─┴─┘  blocks ▁▂▃▄▅▆▇█  braille ⠁⠃⠇⠏⠟⠿⣿"),
        ])
        .block(panel("Unicode · graphemes, wide cells and shaping"))
        .wrap(Wrap { trim: false }),
        rows[1],
    );
}

fn calendar(frame: &mut Frame, area: Rect, selected: usize) {
    let cols = Layout::horizontal([Constraint::Length(25), Constraint::Min(5)]).split(area);
    let date = Date::from_calendar_date(2026, Month::October, 1).expect("valid gallery date");
    let mut events = CalendarEventStore::default();
    for (day, color) in [(3, Color::Blue), (15, Color::Green), (27, Color::Magenta)] {
        events.add(
            Date::from_calendar_date(2026, Month::October, day).expect("valid event day"),
            Style::new().bg(color).fg(Color::Black),
        );
    }
    events.add(
        Date::from_calendar_date(2026, Month::October, (selected % 31 + 1) as u8)
            .expect("valid selected day"),
        SELECTED,
    );
    frame.render_widget(
        Monthly::new(date, events)
            .block(panel("Monthly · ↑ / ↓ day"))
            .show_month_header(Style::new().fg(ACCENT).add_modifier(Modifier::BOLD))
            .show_weekdays_header(Style::new().fg(Color::Yellow))
            .show_surrounding(Style::new().fg(DIM)),
        cols[0],
    );
    let text: Vec<_> = (0..usize::from(area.height))
        .map(|i| {
            Line::from(format!(
                "Background {:02} · pending job · styled log content behind the overlay",
                i + 1
            ))
        })
        .collect();
    frame.render_widget(
        Paragraph::new(text)
            .block(panel("Layering · odd selection opens modal"))
            .style(Style::new().fg(Color::Blue))
            .wrap(Wrap { trim: false }),
        cols[1],
    );
    if selected % 2 == 1 {
        let width = cols[1].width.saturating_sub(4).min(56);
        let height = cols[1].height.saturating_sub(2).min(8);
        let popup = Rect::new(
            cols[1].x + cols[1].width.saturating_sub(width) / 2,
            cols[1].y + cols[1].height.saturating_sub(height) / 2,
            width,
            height,
        );
        frame.render_widget(Clear, popup);
        frame.render_widget(Paragraph::new("Clear erases the background cells and styles before this modal is drawn.\n\n↑ / ↓ opens and closes the overlay.")
            .block(Block::bordered().border_type(BorderType::Double).title(" Confirm action ")
                .border_style(Style::new().fg(Color::Yellow)).padding(Padding::horizontal(1)))
            .alignment(Alignment::Center).wrap(Wrap { trim: true }), popup);
    }
}

fn artwork(frame: &mut Frame, area: Rect, selected: usize) {
    let cols =
        Layout::horizontal([Constraint::Percentage(45), Constraint::Percentage(55)]).split(area);
    let rows = Layout::vertical([
        Constraint::Min(3),
        Constraint::Length(4),
        Constraint::Length(4),
    ])
    .split(cols[0]);
    let block = panel("Fill · ↑ / ↓ changes symbol");
    let inner = block.inner(rows[0]);
    frame.render_widget(block, rows[0]);
    frame.render_widget(
        Fill::new(["·", "░", "▒", "▓", "█", "╳"][selected % 6])
            .style(Style::new().fg(Color::Cyan).bg(Color::Blue)),
        inner,
    );
    let tiny = panel("RatatuiLogo · tiny (15 × 2)");
    let small = panel("RatatuiLogo · small (27 × 2)");
    let tiny_area = tiny.inner(rows[1]);
    let small_area = small.inner(rows[2]);
    frame.render_widget(tiny, rows[1]);
    frame.render_widget(small, rows[2]);
    frame.render_widget(RatatuiLogo::tiny(), tiny_area);
    frame.render_widget(RatatuiLogo::small(), small_area);
    let block = panel("RatatuiMascot · 32 × 16 · eye ↑ / ↓");
    let inner = block.inner(cols[1]);
    frame.render_widget(block, cols[1]);
    frame.render_widget(
        RatatuiMascot::new().set_eye(if selected % 2 == 0 {
            MascotEyeColor::Default
        } else {
            MascotEyeColor::Red
        }),
        inner,
    );
}
