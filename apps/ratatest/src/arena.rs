//! Dense, deterministic workloads made from the same widgets as a terminal assistant.
//! The scene is simulated; the observation rail displays measured application counters.
use crate::ui::App;
use ratatui::{
    Frame,
    buffer::Buffer,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    symbols::Marker,
    text::{Line, Span},
    widgets::{
        Axis, BarChart, Block, BorderType, Chart, Clear, Dataset, Gauge, GraphType, List, ListItem,
        ListState, Paragraph, Sparkline, Widget, Wrap,
    },
};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

pub const PAGES: &[(&str, &str, &str)] = &[
    (
        "arena",
        "Mission control",
        "A dense simulated assistant workspace: live transcript, patch, task selection, charts and layered approval. Space pauses, o toggles overlays, i edits, +/- changes load.",
    ),
    (
        "review-lab",
        "Patch review / approval",
        "A responsive review desk: file selection, independent old/new panes, multiline tool results and a floating approval. Look for clean edges and correct repaint when the dialog disappears.",
    ),
    (
        "reflow-lab",
        "Reflow / Unicode laboratory",
        "Styled streaming prose beside a cell-width inspector. Resize, scroll, select and edit; watch wrapping, indentation, wide graphemes, long-to-short replacement and cursor anchors.",
    ),
    (
        "damage-storm",
        "Damage storm",
        "A deterministic high-churn RGB workload with changing bars, cell mosaics, moving overlays and long/short substitutions. Four stable corner sentinels must survive. +/- adjusts load; Space freezes.",
    ),
];

const BG: Color = Color::Rgb(9, 15, 27);
const PANEL: Color = Color::Rgb(14, 23, 39);
const BORDER: Color = Color::Rgb(55, 76, 101);
const FG: Color = Color::Rgb(224, 233, 242);
const MUTED: Color = Color::Rgb(141, 163, 186);
const CYAN: Color = Color::Rgb(91, 225, 237);
const AMBER: Color = Color::Rgb(255, 202, 109);
const VIOLET: Color = Color::Rgb(185, 156, 255);
const GREEN: Color = Color::Rgb(126, 226, 173);
const RED: Color = Color::Rgb(255, 145, 157);
const SPIN: [&str; 8] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧"];

fn ink(color: Color) -> Style {
    Style::new().fg(color).bg(PANEL)
}
fn bold(color: Color) -> Style {
    ink(color).add_modifier(Modifier::BOLD)
}
fn block(title: impl Into<Line<'static>>, color: Color) -> Block<'static> {
    Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(ink(BORDER))
        .style(ink(FG))
        .title(title.into().style(bold(color)))
}
fn tag(label: impl Into<String>, color: Color) -> Span<'static> {
    Span::styled(
        label.into(),
        Style::new().fg(BG).bg(color).add_modifier(Modifier::BOLD),
    )
}
fn phase(app: &App) -> usize {
    ((app.tick / 18) % 5) as usize
}
fn activity(app: &App) -> &'static str {
    [
        "Read workspace",
        "Inspect renderer",
        "Draft patch",
        "Check output",
        "Review result",
    ][phase(app)]
}
fn gauge_value(tick: u64, offset: u64) -> u16 {
    ((tick % 101 + offset) % 101) as u16
}

pub fn render(frame: &mut Frame, area: Rect, page: usize, app: &App) {
    if area.is_empty() {
        return;
    }
    frame.render_widget(Block::new().style(Style::new().fg(FG).bg(BG)), area);
    if area.width < 26 || area.height < 7 {
        let line = format!(
            "{} · tick {} · L{}",
            PAGES[page % PAGES.len()].1,
            app.tick,
            app.load
        );
        frame.render_widget(
            Paragraph::new(line)
                .style(Style::new().fg(CYAN).bg(BG))
                .wrap(Wrap { trim: false }),
            area,
        );
        return;
    }
    let [ribbon, body, expectation] = Layout::vertical([
        Constraint::Length(if area.height >= 19 { 2 } else { 1 }),
        Constraint::Min(0),
        Constraint::Length(1),
    ])
    .areas(area);
    ribbon_view(frame, ribbon, page % PAGES.len(), app);
    match page % PAGES.len() {
        0 => mission(frame, body, app),
        1 => review(frame, body, app),
        2 => reflow(frame, body, app),
        _ => storm(frame, body, app),
    }
    let expected = [
        " EXPECT: clean panel edges · stable composer · overlays erase and restore",
        " EXPECT: old/new panes stay aligned · inverse selection · no stale dialog cells",
        " EXPECT: intact graphemes · preserved spaces · bounded cursor · clean shorter text",
        " EXPECT: NW / NE / SW / SE stay fixed · no residue after moving layers",
    ][page % PAGES.len()];
    frame.render_widget(
        Paragraph::new(expected).style(Style::new().fg(MUTED).bg(BG)),
        expectation,
    );
}

fn ribbon_view(frame: &mut Frame, area: Rect, page: usize, app: &App) {
    let line = Line::from(vec![
        tag(format!(" {:02} ", page + 1), CYAN),
        Span::styled(
            format!(" {} ", PAGES[page].1),
            Style::new().fg(FG).bg(BG).add_modifier(Modifier::BOLD),
        ),
        Span::styled(" / ", Style::new().fg(BORDER).bg(BG)),
        Span::styled(
            format!("{} {}", SPIN[app.tick as usize % SPIN.len()], activity(app)),
            Style::new().fg(CYAN).bg(BG),
        ),
        Span::styled(
            format!("   SIMULATION  L{}  tick {:05}", app.load, app.tick),
            Style::new().fg(AMBER).bg(BG),
        ),
    ]);
    let mut lines = vec![line];
    if area.height > 1 {
        lines.push(Line::styled(
            " deterministic scene  /  bounded data  /  measurements in the observation rail",
            Style::new().fg(MUTED).bg(BG),
        ));
    }
    frame.render_widget(Paragraph::new(lines), area);
}

fn mission(frame: &mut Frame, area: Rect, app: &App) {
    if area.width >= 100 {
        let [nav, main, rail] = Layout::horizontal([
            Constraint::Length(21),
            Constraint::Min(30),
            Constraint::Length(25),
        ])
        .areas(area);
        tasks(frame, nav, app);
        mission_main(frame, main, app);
        telemetry(frame, rail, app);
    } else if area.width >= 68 {
        let [main, rail] =
            Layout::horizontal([Constraint::Min(36), Constraint::Length(24)]).areas(area);
        mission_main(frame, main, app);
        telemetry(frame, rail, app);
    } else {
        mission_main(frame, area, app);
    }
    if !app.editing && app.popup && app.tick % 90 < 28 && area.width >= 58 && area.height >= 15 {
        approval(
            frame,
            area,
            app,
            "Run the proposed verification?",
            "cargo test -p terminal",
            false,
        );
    } else if !app.editing
        && app.popup
        && app.tick % 90 >= 65
        && area.width >= 38
        && area.height >= 9
    {
        toast(
            frame,
            area,
            "SIMULATED TOOL RESULT",
            "Patch prepared · awaiting review",
            GREEN,
        );
    }
}

fn tasks(frame: &mut Frame, area: Rect, app: &App) {
    let [list, summary] = Layout::vertical([
        Constraint::Min(4),
        Constraint::Length(if area.height >= 20 { 8 } else { 4 }),
    ])
    .areas(area);
    let names = [
        "Workspace",
        "Terminal cells",
        "Styled transcript",
        "Patch review",
        "Input / cursor",
        "Repaint audit",
        "Release notes",
    ];
    let items: Vec<_> = names
        .iter()
        .enumerate()
        .map(|(i, name)| {
            let status = if i < phase(app) {
                "done"
            } else if i == phase(app) {
                "active"
            } else {
                "queued"
            };
            ListItem::new(vec![
                Line::from(vec![
                    Span::styled(format!("{:02} ", i + 1), ink(MUTED)),
                    Span::raw(*name),
                ]),
                Line::styled(
                    format!("   {status}"),
                    ink(if i < phase(app) { GREEN } else { MUTED }),
                ),
            ])
        })
        .collect();
    frame.render_stateful_widget(
        List::new(items)
            .block(block(" WORKSPACE ", CYAN))
            .highlight_symbol("▸ ")
            .highlight_style(Style::new().fg(BG).bg(CYAN).add_modifier(Modifier::BOLD)),
        list,
        &mut ListState::default().with_selected(Some(app.selected % names.len())),
    );
    frame.render_widget(
        Paragraph::new(vec![
            Line::styled("RATATEST / ARENA", bold(VIOLET)),
            Line::from("branch  demo/stress"),
            Line::from("scope   terminal UI"),
            Line::styled("scene   simulated", ink(AMBER)),
            Line::from(""),
            Line::styled("↑ / ↓ selects a task", ink(MUTED)),
        ])
        .block(block(" CONTEXT ", VIOLET)),
        summary,
    );
}

fn mission_main(frame: &mut Frame, area: Rect, app: &App) {
    let composer_height = if area.height >= 12 { 4 } else { 3 };
    let [content, compose] =
        Layout::vertical([Constraint::Min(1), Constraint::Length(composer_height)]).areas(area);
    if content.height >= 19 {
        let [history, patch] =
            Layout::vertical([Constraint::Percentage(57), Constraint::Percentage(43)])
                .areas(content);
        transcript(frame, history, app, false);
        mini_patch(frame, patch, app);
    } else {
        transcript(frame, content, app, false);
    }
    composer(frame, compose, app);
}

fn transcript(frame: &mut Frame, area: Rect, app: &App, extensive: bool) {
    let tokens = [
        "Review",
        "Review the",
        "Review the terminal",
        "Review the terminal renderer",
        "Review the terminal renderer and explain",
        "Review the terminal renderer and explain the redraw path.",
    ];
    let streamed = tokens[(app.tick as usize / 3) % tokens.len()];
    let mut lines = vec![
        Line::from(vec![
            tag(" YOU ", VIOLET),
            Span::raw("  exercise the terminal at realistic density"),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled("● ", ink(CYAN)),
            Span::styled("Assistant", bold(CYAN)),
            Span::styled("  simulated session", ink(MUTED)),
        ]),
        Line::from(vec![Span::styled("▌ ", ink(CYAN)), Span::raw(streamed)]),
        Line::from("The transcript, input and review stay active together."),
        Line::from(""),
        Line::from(vec![
            Span::styled("  ├─ ", ink(BORDER)),
            Span::styled("Read ", ink(VIOLET)),
            Span::raw("crates/trueos-terminal/src/lib.rs"),
        ]),
        Line::from(vec![
            Span::styled("  ├─ ", ink(BORDER)),
            Span::styled("Inspect ", ink(VIOLET)),
            Span::raw("cell widths, inverse style and cursor state"),
        ]),
        Line::from(vec![
            Span::styled("  └─ ", ink(BORDER)),
            Span::styled("Prepare ", ink(GREEN)),
            Span::raw("a focused renderer specimen"),
        ]),
        Line::from(""),
        Line::styled(
            "    $ cargo test -p terminal  [illustrative output]",
            ink(AMBER),
        ),
        Line::from("    running 3 specimen checks"),
        Line::styled("    inverse style      ... ok", ink(GREEN)),
        Line::styled("    overlay cleanup    ... ok", ink(GREEN)),
        Line::styled("    cursor anchor      ... ok", ink(GREEN)),
        Line::from(""),
        Line::from(vec![
            Span::styled(
                "   selected text",
                ink(CYAN).add_modifier(Modifier::REVERSED),
            ),
            Span::raw(" should remain legible while this line wraps."),
        ]),
    ];
    if extensive {
        lines.extend([
            Line::from(""),
            Line::styled("## Rendering notes", bold(VIOLET)),
            Line::from("A terminal assistant mixes narrative text with command output, inline code, file paths and highlighted choices. Each span keeps its own style after wrapping."),
            Line::from("  Indentation is deliberate.  Two spaces between clauses stay visible."),
            Line::from("    nested line: keep four leading spaces even at a narrow width."),
            Line::from("Unicode: café · e\u{301} · 東京 · 🦀 · 👩‍💻 · ♥️ · 🇩🇪"),
            Line::from("A_path_without_breaks/src/very_long_module_name/renderer/selected_cells.rs:128:9"),
            Line::from(vec![Span::raw("Link specimen: "), Span::styled("ratatui.rs", ink(CYAN).add_modifier(Modifier::UNDERLINED)), Span::raw("  (visual underline)")]),
            Line::from(""),
            Line::from("End of bounded sample. PgUp / PgDn changes the visible viewport."),
        ]);
    }
    for text in app.submissions.iter().rev().take(3).rev() {
        lines.push(Line::from(vec![
            Span::styled("YOU › ", ink(VIOLET)),
            Span::raw(text.clone()),
        ]));
    }
    frame.render_widget(
        Paragraph::new(lines)
            .block(block(
                if extensive {
                    " TRANSCRIPT / REFLOW "
                } else {
                    " LIVE TRANSCRIPT "
                },
                CYAN,
            ))
            .wrap(Wrap { trim: false })
            .scroll((app.scroll, 0)),
        area,
    );
}

fn mini_patch(frame: &mut Frame, area: Rect, app: &App) {
    let lines = vec![
        Line::from(vec![
            Span::styled("src/renderer.rs", bold(VIOLET)),
            Span::styled("   +3 −2  ·  simulated", ink(MUTED)),
        ]),
        Line::styled(
            "  42 − let selected = Style::default();",
            ink(RED).bg(Color::Rgb(44, 24, 37)),
        ),
        Line::styled(
            "  42 + let selected = Style::default()",
            ink(GREEN).bg(Color::Rgb(18, 42, 38)),
        ),
        Line::styled(
            "  43 +     .fg(Color::Cyan)",
            ink(GREEN).bg(Color::Rgb(18, 42, 38)),
        ),
        Line::styled(
            "  44 +     .add_modifier(Modifier::REVERSED);",
            ink(GREEN).bg(Color::Rgb(18, 42, 38)),
        ),
        Line::styled(
            if app.tick % 12 < 6 {
                "  output: checking terminal/cursor/selection/overlay cleanup"
            } else {
                "  output: ok"
            },
            ink(AMBER),
        ),
    ];
    frame.render_widget(
        Paragraph::new(lines).block(block(" PATCH PREVIEW ", VIOLET)),
        area,
    );
}

fn composer(frame: &mut Frame, area: Rect, app: &App) {
    let panel = block(
        if app.editing {
            " COMPOSER / EDITING "
        } else {
            " COMPOSER / i to edit "
        },
        if app.editing { CYAN } else { MUTED },
    );
    let inner = panel.inner(area);
    frame.render_widget(panel, area);
    let prompt = if app.input.is_empty() && !app.editing {
        "Ask about redraws, @renderer or /review…"
    } else {
        &app.input
    };
    let room = usize::from(inner.width.saturating_sub(3));
    let (tail, width) = visible_tail(prompt, room);
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(vec![
                Span::styled("› ", bold(CYAN)),
                Span::styled(tail, ink(if app.editing { FG } else { MUTED })),
            ]),
            Line::styled(
                "Enter submit   Esc finish   o overlay   Space pause",
                ink(MUTED),
            ),
        ]),
        inner,
    );
    if app.editing && inner.width >= 3 && inner.height > 0 {
        frame.set_cursor_position((inner.x + 2 + width as u16, inner.y));
    }
}

fn visible_tail(text: &str, max_width: usize) -> (&str, usize) {
    let mut width = 0;
    let mut start = text.len();
    for (i, grapheme) in text.grapheme_indices(true).rev() {
        let next = UnicodeWidthStr::width(grapheme);
        if width + next > max_width {
            break;
        }
        width += next;
        start = i;
    }
    (&text[start..], width)
}

fn telemetry(frame: &mut Frame, area: Rect, app: &App) {
    if area.height < 13 {
        observations(frame, area, app);
        return;
    }
    let [metrics, progress, graph] = Layout::vertical([
        Constraint::Length(if area.height >= 24 { 10 } else { 7 }),
        Constraint::Length(4),
        Constraint::Min(2),
    ])
    .areas(area);
    observations(frame, metrics, app);
    frame.render_widget(
        Gauge::default()
            .block(block(" SCENE / simulated ", AMBER))
            .percent(gauge_value(app.tick, 17))
            .gauge_style(ink(CYAN).bg(Color::Rgb(26, 45, 62)))
            .use_unicode(true)
            .label(format!("phase {} / 5", phase(app) + 1)),
        progress,
    );
    let samples: Vec<u64> = (0..64)
        .map(|i| ((i * 17 + (app.tick % 997) * 11) % 73) + 15)
        .collect();
    if graph.height < 12 {
        frame.render_widget(
            Sparkline::default()
                .block(block(" ACTIVITY / simulated ", VIOLET))
                .data(&samples)
                .max(100)
                .style(ink(VIOLET)),
            graph,
        );
    } else {
        let [spark, chart] =
            Layout::vertical([Constraint::Length(5), Constraint::Min(3)]).areas(graph);
        frame.render_widget(
            Sparkline::default()
                .block(block(" ACTIVITY / simulated ", VIOLET))
                .data(&samples)
                .max(100)
                .style(ink(VIOLET)),
            spark,
        );
        let wave: Vec<_> = (0..48)
            .map(|i| {
                let x = f64::from(i);
                (x, ((x + (app.tick % 1000) as f64) * 0.2).sin() * 0.7)
            })
            .collect();
        frame.render_widget(
            Chart::new(vec![
                Dataset::default()
                    .data(&wave)
                    .graph_type(GraphType::Line)
                    .marker(Marker::Braille)
                    .style(ink(CYAN)),
            ])
            .block(block(" SIGNAL / simulated ", CYAN))
            .x_axis(Axis::default().bounds([0.0, 47.0]).style(ink(BORDER)))
            .y_axis(Axis::default().bounds([-1.0, 1.0]).style(ink(BORDER))),
            chart,
        );
    }
}

fn observations(frame: &mut Frame, area: Rect, app: &App) {
    let m = &app.metrics;
    let lines = vec![
        Line::from(vec![
            Span::styled("frames ", ink(MUTED)),
            Span::styled(format!("{:>12}", m.frames), bold(CYAN)),
        ]),
        Line::from(vec![
            Span::styled("last B ", ink(MUTED)),
            Span::styled(format!("{:>12}", m.last_frame_bytes), ink(FG)),
        ]),
        Line::from(vec![
            Span::styled("total B", ink(MUTED)),
            Span::styled(format!("{:>12}", m.bytes), ink(FG)),
        ]),
        Line::from(vec![
            Span::styled("draw μs", ink(MUTED)),
            Span::styled(format!("{:>12}", m.draw_micros), ink(AMBER)),
        ]),
        Line::from(vec![
            Span::styled("peak μs", ink(MUTED)),
            Span::styled(format!("{:>12}", m.peak_draw_micros), ink(AMBER)),
        ]),
        Line::from(vec![
            Span::styled("slow   ", ink(MUTED)),
            Span::styled(format!("{:>12}", m.slow_frames), ink(VIOLET)),
        ]),
        Line::styled(
            format!(
                "observed {:.1}s · load {}",
                m.elapsed_ms as f64 / 1000.0,
                app.load
            ),
            ink(MUTED),
        ),
        Line::styled("previous completed draw", ink(MUTED)),
    ];
    frame.render_widget(
        Paragraph::new(lines).block(block(" OBSERVED / actual ", GREEN)),
        area,
    );
}

fn review(frame: &mut Frame, area: Rect, app: &App) {
    if area.width >= 100 {
        let [files, main, rail] = Layout::horizontal([
            Constraint::Length(21),
            Constraint::Min(35),
            Constraint::Length(25),
        ])
        .areas(area);
        review_files(frame, files, app);
        review_main(frame, main, app);
        telemetry(frame, rail, app);
    } else if area.width >= 70 {
        let [files, main] =
            Layout::horizontal([Constraint::Length(20), Constraint::Min(30)]).areas(area);
        review_files(frame, files, app);
        review_main(frame, main, app);
    } else {
        review_main(frame, area, app);
    }
    if app.popup && (!app.stress_tour || app.tick % 64 < 32) && area.width >= 48 && area.height >= 13 {
        approval(
            frame,
            area,
            app,
            "Apply this illustrative patch?",
            "src/renderer.rs  +3 −2",
            true,
        );
    }
}

fn review_files(frame: &mut Frame, area: Rect, app: &App) {
    let items = [
        ("M", "renderer.rs", "+3 −2"),
        ("M", "cells.rs", "+8 −4"),
        ("A", "repaint.rs", "+24"),
        ("M", "composer.rs", "+6 −1"),
        ("M", "README.md", "+12"),
    ]
    .into_iter()
    .map(|(kind, name, delta)| {
        ListItem::new(vec![
            Line::from(vec![
                Span::styled(
                    format!("{kind} "),
                    ink(if kind == "A" { GREEN } else { AMBER }),
                ),
                Span::raw(name),
            ]),
            Line::styled(format!("  {delta} · simulated"), ink(MUTED)),
        ])
    })
    .collect::<Vec<_>>();
    frame.render_stateful_widget(
        List::new(items)
            .block(block(" CHANGED FILES ", VIOLET))
            .highlight_symbol("▸ ")
            .highlight_style(Style::new().fg(BG).bg(VIOLET)),
        area,
        &mut ListState::default().with_selected(Some(app.selected % 5)),
    );
}

fn review_main(frame: &mut Frame, area: Rect, app: &App) {
    let [diff, output] = Layout::vertical([
        Constraint::Min(4),
        Constraint::Length(if area.height >= 17 { 7 } else { 4 }),
    ])
    .areas(area);
    if diff.width >= 66 {
        let [old, new] = Layout::horizontal([Constraint::Ratio(1, 2); 2]).areas(diff);
        code_side(frame, old, false, app);
        code_side(frame, new, true, app);
    } else {
        mini_patch(frame, diff, app);
    }
    let long = app.tick % 16 < 8;
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(vec![
                Span::styled(
                    format!("{} ", SPIN[app.tick as usize % SPIN.len()]),
                    ink(AMBER),
                ),
                Span::styled("Tool transcript", bold(AMBER)),
                Span::styled(" · illustrative", ink(MUTED)),
            ]),
            Line::styled(
                if long {
                    "$ cargo test -p terminal --features=wide-cells,overlays"
                } else {
                    "$ cargo test"
                },
                ink(CYAN),
            ),
            Line::from("  ├─ style state / reverse video / blank fill"),
            Line::from("  └─ overlay teardown / cursor restore / resize"),
            Line::styled("This scene does not execute commands.", ink(MUTED)),
        ])
        .block(block(" VERIFICATION OUTPUT ", AMBER))
        .wrap(Wrap { trim: false }),
        output,
    );
}

fn code_side(frame: &mut Frame, area: Rect, new: bool, app: &App) {
    let source = if new {
        [
            "fn paint_selection() {",
            "    let selected = Style::new()",
            "        .fg(Color::Cyan)",
            "        .add_modifier(REVERSED);",
            "    draw_row(selected);",
            "}",
            "",
            "fn restore_overlay() {",
            "    frame.render_widget(Clear, area);",
            "    paint_content(frame, area);",
            "}",
        ]
    } else {
        [
            "fn paint_selection() {",
            "    let selected = Style::new();",
            "",
            "",
            "    draw_row(selected);",
            "}",
            "",
            "fn restore_overlay() {",
            "    // underlying cells may persist",
            "    paint_content(frame, area);",
            "}",
        ]
    };
    let lines: Vec<_> = source
        .into_iter()
        .enumerate()
        .map(|(i, text)| {
            let changed = matches!(i, 1..=3 | 8);
            let color = if changed {
                if new { GREEN } else { RED }
            } else {
                FG
            };
            let row_bg = if changed {
                if new {
                    Color::Rgb(18, 42, 38)
                } else {
                    Color::Rgb(44, 24, 37)
                }
            } else {
                PANEL
            };
            Line::from(vec![
                Span::styled(
                    format!(
                        "{:>3} {} ",
                        i + 40,
                        if changed {
                            if new { "+" } else { "−" }
                        } else {
                            " "
                        }
                    ),
                    ink(MUTED).bg(row_bg),
                ),
                Span::styled(text, ink(color).bg(row_bg)),
            ])
        })
        .collect();
    frame.render_widget(
        Paragraph::new(lines)
            .block(block(
                if new {
                    " AFTER / proposed "
                } else {
                    " BEFORE / baseline "
                },
                if new { GREEN } else { RED },
            ))
            .scroll(((app.scroll / 2).min(10), 0)),
        area,
    );
}

fn approval(frame: &mut Frame, area: Rect, app: &App, title: &str, detail: &str, review: bool) {
    let width = area.width.saturating_sub(6).min(54);
    let height = area.height.saturating_sub(2).min(10);
    let popup = Rect::new(
        area.x + area.width.saturating_sub(width) / 2,
        area.y + area.height.saturating_sub(height) / 2,
        width,
        height,
    );
    let shadow = Rect::new(
        popup.x.saturating_add(1),
        popup.y.saturating_add(1),
        popup.width,
        popup.height,
    )
    .intersection(area);
    frame.render_widget(
        Block::new().style(Style::new().bg(Color::Rgb(3, 7, 14))),
        shadow,
    );
    frame.render_widget(Clear, popup);
    let mut lines = vec![
        Line::styled(title.to_string(), bold(FG)),
        Line::styled(detail.to_string(), ink(CYAN)),
        Line::styled("Illustrative choice; no action is executed.", ink(MUTED)),
        Line::from(""),
    ];
    let options = if review {
        ["Apply once", "Inspect first", "Keep baseline"]
    } else {
        ["Allow once", "Inspect command", "Decline"]
    };
    for (i, option) in options.into_iter().enumerate() {
        lines.push(Line::styled(
            format!(
                " {} {option}",
                if i == app.selected % 3 { "▸" } else { " " }
            ),
            if i == app.selected % 3 {
                ink(AMBER).add_modifier(Modifier::REVERSED)
            } else {
                ink(FG)
            },
        ));
    }
    frame.render_widget(
        Paragraph::new(lines)
            .block(block(" APPROVAL / o closes ", AMBER))
            .wrap(Wrap { trim: false }),
        popup,
    );
}

fn toast(frame: &mut Frame, area: Rect, title: &str, body: &str, color: Color) {
    let width = area.width.saturating_sub(2).min(43);
    let height = area.height.min(4);
    let popup = Rect::new(
        area.right().saturating_sub(width + 1),
        area.bottom().saturating_sub(height + 1),
        width,
        height,
    );
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(body.to_string())
            .block(block(format!(" {title} "), color))
            .wrap(Wrap { trim: false }),
        popup,
    );
}

fn reflow(frame: &mut Frame, area: Rect, app: &App) {
    let (main, rail) = if area.width >= 100 {
        let [nav, main, rail] = Layout::horizontal([
            Constraint::Length(21),
            Constraint::Min(30),
            Constraint::Length(30),
        ])
        .areas(area);
        reflow_nav(frame, nav, app);
        (main, Some(rail))
    } else if area.width >= 68 {
        let [main, rail] =
            Layout::horizontal([Constraint::Min(33), Constraint::Length(29)]).areas(area);
        (main, Some(rail))
    } else {
        (area, None)
    };
    let [prose, compose] = Layout::vertical([
        Constraint::Min(2),
        Constraint::Length(if main.height >= 12 { 4 } else { 3 }),
    ])
    .areas(main);
    transcript(frame, prose, app, true);
    composer(frame, compose, app);
    if let Some(rail) = rail {
        let [cells, metrics] = Layout::vertical([
            Constraint::Min(5),
            Constraint::Length(if rail.height >= 24 { 10 } else { 7 }),
        ])
        .areas(rail);
        cell_inspector(frame, cells, app);
        observations(frame, metrics, app);
    }
}

fn reflow_nav(frame: &mut Frame, area: Rect, app: &App) {
    let names = [
        "Styled prose",
        "Indentation",
        "Long token",
        "Wide grapheme",
        "Combining mark",
        "Inverse span",
        "Cursor anchor",
        "Shorter repaint",
    ];
    let items: Vec<_> = names
        .into_iter()
        .enumerate()
        .map(|(i, name)| {
            ListItem::new(vec![
                Line::from(format!("{:02} {name}", i + 1)),
                Line::styled("   resize to inspect", ink(MUTED)),
            ])
        })
        .collect();
    frame.render_stateful_widget(
        List::new(items)
            .block(block(" SPECIMENS ", VIOLET))
            .highlight_symbol("▸ ")
            .highlight_style(Style::new().fg(BG).bg(VIOLET)),
        area,
        &mut ListState::default().with_selected(Some(app.selected % names.len())),
    );
}

fn cell_inspector(frame: &mut Frame, area: Rect, app: &App) {
    let specimen = ["ASCII", "東京", "e\u{301}", "🦀", "👩‍💻", "♥️", "🇩🇪"][app.selected % 7];
    let long = app.tick % 18 < 9;
    let lines = vec![
        Line::styled("GRAPHEME / CELL WIDTH", bold(VIOLET)),
        Line::from(""),
        Line::from(vec![
            Span::raw("sample   "),
            Span::styled(specimen, bold(CYAN)),
        ]),
        Line::from(format!("graphemes  {}", specimen.graphemes(true).count())),
        Line::from(format!("cells      {}", UnicodeWidthStr::width(specimen))),
        Line::from(""),
        Line::styled("│a│界│e\u{301}│🦀│end", ink(CYAN)),
        Line::styled("01234567890123456789012345", ink(MUTED)),
        Line::from(""),
        Line::styled("LONG → SHORT REPLACEMENT", bold(AMBER)),
        Line::styled(
            if long {
                "request/render/selection/okay"
            } else {
                "ok"
            },
            ink(AMBER),
        ),
        Line::from(""),
        Line::from(vec![
            Span::raw("blanks ["),
            Span::styled("       ", ink(FG).bg(VIOLET)),
            Span::raw("]"),
        ]),
        Line::from(vec![
            Span::raw("inverse "),
            Span::styled(" focus ", ink(CYAN).add_modifier(Modifier::REVERSED)),
        ]),
        Line::from(""),
        Line::styled("Widths above are Ratatui's", ink(MUTED)),
        Line::styled("expected terminal cell model.", ink(MUTED)),
        Line::styled("Compare native and SSH views.", ink(MUTED)),
    ];
    frame.render_widget(
        Paragraph::new(lines).block(block(" CELL INSPECTOR ", VIOLET)),
        area,
    );
}

fn storm(frame: &mut Frame, area: Rect, app: &App) {
    if area.width >= 100 {
        let [nav, main, rail] = Layout::horizontal([
            Constraint::Length(21),
            Constraint::Min(30),
            Constraint::Length(25),
        ])
        .areas(area);
        storm_legend(frame, nav, app);
        storm_main(frame, main, app);
        telemetry(frame, rail, app);
    } else if area.width >= 68 {
        let [main, rail] =
            Layout::horizontal([Constraint::Min(33), Constraint::Length(24)]).areas(area);
        storm_main(frame, main, app);
        telemetry(frame, rail, app);
    } else {
        storm_main(frame, area, app);
    }
}

fn storm_legend(frame: &mut Frame, area: Rect, app: &App) {
    let lines = vec![
        Line::styled("DAMAGE WORKLOAD", bold(AMBER)),
        Line::from(""),
        Line::styled(format!("LOAD {} / 3", app.load), bold(CYAN)),
        Line::from(match app.load {
            1 => "Sparse moving bands",
            2 => "Dense colored bands",
            _ => "Every cell changes",
        }),
        Line::from(""),
        Line::from("1. RGB backgrounds"),
        Line::from("2. Foreground reset"),
        Line::from("3. Changing bar sizes"),
        Line::from("4. Long / short text"),
        Line::from("5. Moving Clear layer"),
        Line::from("6. Corner sentinels"),
        Line::from(""),
        Line::styled("+/- changes load", ink(CYAN)),
        Line::styled("o toggles layers", ink(CYAN)),
        Line::styled("Space freezes scene", ink(CYAN)),
        Line::from(""),
        Line::styled("No synthetic speed", ink(MUTED)),
        Line::styled("or pass/fail claims.", ink(MUTED)),
    ];
    frame.render_widget(
        Paragraph::new(lines)
            .block(block(" STORM CONTROL ", AMBER))
            .wrap(Wrap { trim: false }),
        area,
    );
}

fn storm_main(frame: &mut Frame, area: Rect, app: &App) {
    let [mosaic, bars] = Layout::vertical([
        Constraint::Min(4),
        Constraint::Length(if area.height >= 19 { 7 } else { 4 }),
    ])
    .areas(area);
    let panel = block(" RGB DAMAGE FIELD ", AMBER);
    let inner = panel.inner(mosaic);
    frame.render_widget(panel, mosaic);
    frame.render_widget(
        DamageField {
            tick: app.tick,
            load: app.load,
        },
        inner,
    );
    if inner.width >= 12 && inner.height >= 3 {
        let [top, middle, bottom] = Layout::vertical([
            Constraint::Length(1),
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .areas(inner);
        corner_line(frame, top, "NW", "NE");
        corner_line(frame, bottom, "SW", "SE");
        let substitution = if app.tick % 12 < 6 {
            "LONG: abcdefghijklmnopqrstuvwxyz / stale cells must clear"
        } else {
            "SHORT: ok"
        };
        let label_area = Rect::new(middle.x, middle.y, middle.width, 1.min(middle.height));
        frame.render_widget(
            Paragraph::new(substitution).style(Style::new().fg(FG).bg(BG)),
            label_area,
        );
        if app.popup && middle.width >= 26 && middle.height >= 5 {
            let width = middle.width.saturating_sub(4).min(36);
            let height = 5.min(middle.height);
            let travel_x = middle.width.saturating_sub(width);
            let travel_y = middle.height.saturating_sub(height);
            let popup = Rect::new(
                middle.x + (app.tick % (u64::from(travel_x) + 1)) as u16,
                middle.y + ((app.tick / 3) % (u64::from(travel_y) + 1)) as u16,
                width,
                height,
            );
            if app.tick % 24 < 17 {
                frame.render_widget(Clear, popup);
                frame.render_widget(
                    Paragraph::new(vec![
                        Line::styled(format!("layer {:04}", app.tick), bold(AMBER)),
                        Line::styled(
                            if app.tick % 8 < 4 {
                                "moving / overlapping / repainting"
                            } else {
                                "ok"
                            },
                            ink(FG),
                        ),
                        Line::styled("corners must stay fixed", ink(MUTED)),
                    ])
                    .block(block(" CLEAR OVERLAY ", VIOLET)),
                    popup,
                );
                if app.tick % 24 < 8 && middle.width >= 36 && middle.height >= 7 {
                    let toast_area = Rect::new(
                        middle.right().saturating_sub(24),
                        middle.bottom().saturating_sub(3),
                        24,
                        3,
                    );
                    frame.render_widget(Clear, toast_area);
                    frame.render_widget(
                        Paragraph::new("second layer").block(block(" TOAST ", CYAN)),
                        toast_area,
                    );
                }
            }
        }
    }
    let values = [
        ("paint", 8 + app.tick % 89),
        ("erase", 8 + (app.tick.wrapping_mul(7) % 89)),
        ("layer", 8 + (app.tick.wrapping_mul(13) % 89)),
    ];
    frame.render_widget(
        BarChart::default()
            .block(block(" CHANGING EXTENTS / simulated ", VIOLET))
            .data(&values)
            .direction(Direction::Horizontal)
            .bar_width(1)
            .bar_gap(1)
            .max(100)
            .bar_style(ink(VIOLET))
            .value_style(Style::new().fg(BG).bg(VIOLET))
            .label_style(ink(MUTED)),
        bars,
    );
}

fn corner_line(frame: &mut Frame, area: Rect, left: &str, right: &str) {
    frame.render_widget(
        Paragraph::new(format!("[{left}]")).style(Style::new().fg(CYAN).bg(BG)),
        area,
    );
    frame.render_widget(
        Paragraph::new(format!("[{right}]"))
            .alignment(Alignment::Right)
            .style(Style::new().fg(CYAN).bg(BG)),
        area,
    );
}

/// Write only inside the supplied rectangle. Spaces deliberately carry RGB backgrounds.
struct DamageField {
    tick: u64,
    load: u8,
}
impl Widget for DamageField {
    fn render(self, area: Rect, buffer: &mut Buffer) {
        let tick = self.tick % 4096;
        for y in area.y..area.bottom() {
            for x in area.x..area.right() {
                let px = u64::from(x - area.x);
                let py = u64::from(y - area.y);
                let active = match self.load {
                    1 => (px + py + tick) % 9 < 2,
                    2 => (px + py + tick) % 4 < 3,
                    _ => true,
                };
                let (symbol, fg, bg) = if active {
                    let r = (18 + (px * 5 + tick * 11) % 68) as u8;
                    let g = (30 + (py * 13 + tick * 7) % 80) as u8;
                    let b = (50 + (px * 3 + py * 7 + tick * 5) % 100) as u8;
                    (
                        [" ", "░", "·", " "][(px + py + tick) as usize % 4],
                        Color::Rgb(
                            r.saturating_add(95),
                            g.saturating_add(95),
                            b.saturating_add(80),
                        ),
                        Color::Rgb(r, g, b),
                    )
                } else {
                    (" ", MUTED, PANEL)
                };
                buffer[(x, y)]
                    .set_symbol(symbol)
                    .set_style(Style::new().fg(fg).bg(bg));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{Terminal, backend::TestBackend};

    #[test]
    fn mosaic_never_changes_cells_outside_its_rectangle() {
        let bounds = Rect::new(0, 0, 18, 10);
        let inset = Rect::new(3, 2, 9, 5);
        let initial = Buffer::filled(bounds, ratatui::buffer::Cell::new("X"));
        let mut painted = initial.clone();
        DamageField {
            tick: u64::MAX,
            load: 3,
        }
        .render(inset, &mut painted);
        for y in bounds.y..bounds.bottom() {
            for x in bounds.x..bounds.right() {
                if !inset.contains((x, y).into()) {
                    assert_eq!(painted[(x, y)], initial[(x, y)]);
                }
            }
        }
    }

    #[test]
    fn storm_corners_survive_each_overlay_phase() {
        let mut terminal = Terminal::new(TestBackend::new(80, 30)).unwrap();
        let mut app = App::new();
        app.load = 3;
        app.popup = true;
        for tick in [0, 7, 8, 16, 17, 23, 24, 90, u64::MAX] {
            app.tick = tick;
            terminal
                .draw(|frame| storm_main(frame, frame.area(), &app))
                .unwrap();
            let buffer = terminal.backend().buffer();
            for (x, y, label) in [
                (1, 1, "[NW]"),
                (75, 1, "[NE]"),
                (1, 21, "[SW]"),
                (75, 21, "[SE]"),
            ] {
                for (offset, ch) in label.chars().enumerate() {
                    assert_eq!(
                        buffer[(x + offset as u16, y)].symbol(),
                        ch.to_string(),
                        "tick={tick} at {x},{y}"
                    );
                }
            }
        }
    }

    #[test]
    fn visible_tail_preserves_whole_graphemes_and_cursor_room() {
        assert_eq!(visible_tail("abc東京", 3), ("京", 2));
        assert_eq!(visible_tail("abc e\u{301}", 1), ("e\u{301}", 1));
        assert_eq!(visible_tail("🦀", 1), ("", 0));
    }
}
