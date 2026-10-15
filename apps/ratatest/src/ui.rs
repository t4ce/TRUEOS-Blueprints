use crate::{arena, codex, gallery};
use crossterm::event::Event;
use ratatui::{
    Frame, Terminal,
    backend::TestBackend,
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Tabs, Wrap},
};
use std::{collections::VecDeque, io};

pub const GROUPS: [&str; 5] = [
    "1 Scenarios",
    "2 Widgets",
    "3 Codex patterns",
    "4 Terminal / ANSI",
    "5 Arena",
];
#[derive(Clone, Copy)]
pub enum Kind {
    Scenario(usize),
    Gallery(usize),
    Codex(usize),
    Ansi(usize),
    Arena(usize),
}
pub struct Case {
    pub id: String,
    pub title: &'static str,
    pub detail: &'static str,
    pub group: usize,
    pub kind: Kind,
}
pub fn cases() -> Vec<Case> {
    let mut cases = Vec::new();
    for (index, (id, title, detail)) in [
        ("session", "Assistant session", "Combined transcript, live status, composer, chooser and footer. Press i to type, Enter to submit, o for the popup."),
        ("dashboard", "Dashboard", "Default dashboard: live gauges, charts, borders and status. Space freezes the animation."),
        ("browser", "File / command chooser", "Default selection scenario: highlighted rows, descriptions, scrolling and popup layering. Up/Down changes selection."),
        ("help", "Help / dialog", "Default overlay scenario: Clear erases the covered area; closing the dialog must repaint the original content. Press o."),
    ].into_iter().enumerate() { cases.push(Case { id: id.into(), title, detail, group: 0, kind: Kind::Scenario(index) }); }
    for (index, (title, detail)) in gallery::PAGES.iter().enumerate() {
        cases.push(Case {
            id: format!("widgets-{index}"),
            title,
            detail,
            group: 1,
            kind: Kind::Gallery(index),
        });
    }
    for (index, (id, title, detail)) in [
        ("prose", "Styled prose / code / links", "Codex Paragraph + Wrap(trim:false), Line/Span, code fences, text tables, inline code and OSC8-capable hyperlink cells."),
        ("diff", "Tool output / diff", "Independent tool transcript: additions, deletions, line numbers, multiline output and styled trailing blanks."),
        ("composer", "Composer / cursor", "Press i to edit; Enter submits; Esc leaves editing. Paste is one Event::Paste. Cursor position accounts for grapheme display width."),
        ("chooser", "Approval / search popup", "Clear + Block overlay, selected row with REVERSED, scrolling descriptions. Up/Down selects; o opens/closes the popup."),
        ("status", "Spinner / status / footer", "Codex-style Braille spinner, changing status text, elapsed activity and key hints; Space freezes animation."),
        ("stream", "Streaming / wrapping / selection", "Incremental transcript, preserved indentation, long words, scrolling and inverse selection. PgUp/PgDn scroll; Space freezes."),
        ("repaint", "Differential redraw / Unicode", "Alternating long/short text, colored trailing spaces, CJK/emoji/combining glyphs, custom Buffer widget and cursor anchors. Watch for stale cells or drift."),
        ("combined", "Combined Codex-style session", "All component patterns together. Deterministic simulated content; no model, account or network needed. i edit, o overlay, Up/Down select."),
    ].into_iter().enumerate() { cases.push(Case { id: id.into(), title, detail, group: 2, kind: Kind::Codex(index) }); }
    for (index, (id, title, detail)) in [
        ("ansi-cursor", "Cursor / erase / SGR", "Press a: cursor positioning/save/restore, erase/insert/delete characters, truecolor, indexed colors and inverse reset. Expected output is printed beside each specimen."),
        ("ansi-history", "History / scroll region", "Press a: DECSTBM history insertion. Only the boxed history rows should scroll; the composer below must remain fixed. Region is restored on return."),
        ("ansi-probes", "Capability / default-color probes", "Press a: CPR, DA1/DA2, OSC10/11 and CSI ?u probes. Unsupported capability queries must not move the cursor or leak bytes into text."),
        ("ansi-modes", "Modes / enhanced keyboard / mouse", "Press a: synchronized output, bracketed paste, focus, SGR mouse, Kitty keyboard and modifyOtherKeys. Event log shows delivered input; Esc returns and restores modes."),
        ("input", "Input event inspector", "All key press/repeat/release, modifier, resize, paste, mouse and focus events are shown below. Press a for enhanced input modes. i starts a small text field."),
    ].into_iter().enumerate() { cases.push(Case { id: id.into(), title, detail, group: 3, kind: Kind::Ansi(index) }); }
    for (index, (id, title, detail)) in arena::PAGES.iter().enumerate() {
        cases.push(Case {
            id: (*id).into(),
            title,
            detail,
            group: 4,
            kind: Kind::Arena(index),
        });
    }
    cases
}

#[derive(Default)]
pub struct FrameMetrics {
    pub frames: u64,
    pub bytes: u64,
    pub last_frame_bytes: u64,
    pub draw_micros: u64,
    pub peak_draw_micros: u64,
    pub elapsed_ms: u64,
    pub slow_frames: u64,
}

#[derive(Default)]
pub struct App {
    pub case: usize,
    pub selected: usize,
    pub scroll: u16,
    pub tick: u64,
    pub home: bool,
    pub paused: bool,
    pub tour: bool,
    pub editing: bool,
    pub popup: bool,
    pub input: String,
    pub submissions: Vec<String>,
    pub events: VecDeque<String>,
    pub probe_runs: u64,
    pub load: u8,
    pub stress_tour: bool,
    pub metrics: FrameMetrics,
}
impl App {
    pub fn new() -> Self {
        Self {
            case: cases()
                .iter()
                .position(|c| matches!(c.kind, Kind::Arena(0)))
                .unwrap_or(0),
            home: true,
            popup: false,
            load: 1,
            ..Self::default()
        }
    }
    pub fn next(&mut self, delta: isize) {
        self.case = (self.case as isize + delta).rem_euclid(cases().len() as isize) as usize;
        self.selected = 0;
        self.scroll = 0;
        self.editing = false;
        self.popup = matches!(
            cases()[self.case].kind,
            Kind::Codex(3) | Kind::Scenario(2) | Kind::Scenario(3)
        );
    }
    pub fn select_group(&mut self, group: usize) {
        self.case = cases().iter().position(|c| c.group == group).unwrap_or(0);
        self.home = true;
        self.selected = 0;
        self.scroll = 0;
        self.stress_tour = false;
        self.popup = matches!(
            cases()[self.case].kind,
            Kind::Codex(3) | Kind::Scenario(2) | Kind::Scenario(3)
        );
    }
    pub fn is_arena(&self) -> bool {
        matches!(cases()[self.case].kind, Kind::Arena(_))
    }
    pub fn start_stress(&mut self) {
        self.case = cases()
            .iter()
            .position(|c| matches!(c.kind, Kind::Arena(0)))
            .unwrap();
        self.home = false;
        self.paused = false;
        self.editing = false;
        self.popup = true;
        self.load = 2;
        self.tick = 0;
        self.selected = 0;
        self.scroll = 0;
        self.tour = false;
        self.stress_tour = true;
    }
    pub fn advance_stress(&mut self) {
        let all = cases();
        let first = all
            .iter()
            .position(|c| matches!(c.kind, Kind::Arena(0)))
            .unwrap();
        self.case = first + (self.case.saturating_sub(first) + 1) % arena::PAGES.len();
        self.selected = 0;
        self.scroll = 0;
    }
    pub fn tick_millis(&self) -> u64 {
        if self.is_arena() && !self.home {
            match self.load {
                1 => 100,
                2 => 50,
                _ => 25,
            }
        } else {
            120
        }
    }
    pub fn record_event(&mut self, event: &Event) {
        if self.events.len() == 8 {
            self.events.pop_front();
        }
        self.events
            .push_back(format!("{event:?}").chars().take(4096).collect());
    }
}

pub fn render(frame: &mut Frame, app: &App) {
    let all = cases();
    let case = &all[app.case % all.len()];
    let [header, groups, detail, body, footer] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(2),
        Constraint::Length(if frame.area().height > 18 { 3 } else { 2 }),
        Constraint::Min(0),
        Constraint::Length(2),
    ])
    .areas(frame.area());
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                " RATATEST ",
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(format!(
                " Ratatui 0.30.2  {}  {}",
                case.id,
                if app.paused {
                    "PAUSED"
                } else if app.stress_tour {
                    "STRESS"
                } else if app.tour {
                    "TOUR"
                } else {
                    "LIVE"
                }
            )),
        ])),
        header,
    );
    frame.render_widget(
        Tabs::new(GROUPS.to_vec())
            .select(case.group)
            .highlight_style(Style::default().fg(Color::Cyan))
            .divider(" | "),
        groups,
    );
    frame.render_widget(
        Paragraph::new(case.detail)
            .wrap(Wrap { trim: false })
            .style(Style::default().fg(Color::Gray)),
        detail,
    );
    if app.home {
        home(frame, body, app, &all);
    } else {
        match case.kind {
            Kind::Gallery(page) => gallery::render(frame, body, page, app.tick, app.selected),
            Kind::Codex(page) => codex::render(frame, body, page, app),
            Kind::Scenario(0) => codex::render(frame, body, 7, app),
            Kind::Scenario(1) => {
                let [activity, charts] =
                    Layout::vertical([Constraint::Percentage(40), Constraint::Percentage(60)])
                        .areas(body);
                gallery::render(frame, activity, 2, app.tick, app.selected);
                gallery::render(frame, charts, 3, app.tick, app.selected);
            }
            Kind::Scenario(2) => codex::render(frame, body, 3, app),
            Kind::Scenario(_) => {
                codex::render(frame, body, 7, app);
                if app.popup {
                    codex::help(frame, body);
                }
            }
            Kind::Ansi(index) => crate::ansi::render(frame, body, index, app),
            Kind::Arena(index) => arena::render(frame, body, index, app),
        }
    }
    frame.render_widget(Paragraph::new(if app.editing { " EDITING: type / paste   Enter submit   Esc finish editing   Ctrl-Q quit" }
        else { " 1..5 group  n/p case  Enter open  h menu  s stress  +/- load  Space pause\n Up/Down select  PgUp/PgDn scroll  i edit  o popup  t tour  a probe  F10 shell  q quit" }).style(Style::default().fg(Color::Cyan)), footer);
}

fn home(frame: &mut Frame, area: Rect, app: &App, all: &[Case]) {
    let group = all[app.case].group;
    let entries: Vec<_> = all.iter().filter(|c| c.group == group).collect();
    let position = entries
        .iter()
        .position(|c| c.id == all[app.case].id)
        .unwrap_or(0);
    let [menu, info] =
        Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)]).areas(area);
    let items: Vec<_> = entries
        .iter()
        .map(|c| {
            ListItem::new(vec![
                Line::from(c.title),
                Line::from(Span::styled(
                    c.id.clone(),
                    Style::default().fg(Color::DarkGray),
                )),
            ])
        })
        .collect();
    frame.render_stateful_widget(
        List::new(items)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" Choose a showcase "),
            )
            .highlight_symbol(" > ")
            .highlight_style(Style::default().add_modifier(Modifier::REVERSED)),
        menu,
        &mut ListState::default().with_selected(Some(position)),
    );
    let text = format!(
        "{}\n\n{}\n\nEnter opens the selected case.\nn/p move through every case; 1..5 changes the section.\n\ns starts the Arena stress tour. +/- changes load: 10, 20 or 40 target frames/s. Space freezes animation. The counters measure backend writes and draw time.\n\nt tours all {} cases. ANSI probes still require a.\n\nCompare native UI4 with SSH: the native terminal parses ANSI into cells; SSH forwards app bytes. Unicode width and several text/protocol features remain explicit specimens; see README.",
        all[app.case].title,
        all[app.case].detail,
        all.len()
    );
    frame.render_widget(
        Paragraph::new(text)
            .wrap(Wrap { trim: false })
            .block(Block::bordered().title(" Purpose / expected behavior ")),
        info,
    );
}

pub fn self_check() -> io::Result<()> {
    let all = cases();
    let mut draws = 0;
    for (width, height) in [(1, 1), (20, 5), (40, 12), (80, 24), (120, 40)] {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        let mut app = App::new();
        for index in 0..all.len() {
            app.case = index;
            for home in [true, false] {
                app.home = home;
                for tick in [0, 9, 80] {
                    app.tick = tick;
                    app.selected = tick as usize;
                    app.popup = tick != 9;
                    terminal.draw(|frame| render(frame, &app)).unwrap();
                    draws += 1;
                }
            }
        }
    }
    for case in &all {
        println!(
            "ratatest: case {} [{}] {}",
            case.id,
            case.group + 1,
            case.title
        );
    }
    println!("ratatest: PASS cases={} sizes=5 frames={draws}", all.len());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::buffer::{Buffer, CellWidth};

    fn visible_cells(buffer: &Buffer) -> Buffer {
        let mut visible = buffer.clone();
        for row in visible.content.chunks_mut(usize::from(visible.area.width)) {
            let mut hidden = 0;
            for cell in row {
                if hidden > 0 {
                    // TestBackend stores emitted cells without simulating the terminal's
                    // wide-glyph painting. Hidden placeholders may retain older contents.
                    cell.reset();
                    hidden -= 1;
                } else {
                    hidden = cell.cell_width().saturating_sub(1);
                }
            }
        }
        visible
    }

    #[test]
    fn every_showcase_renders_at_small_and_normal_sizes() {
        self_check().unwrap();
    }
    #[test]
    fn closing_overlays_and_shrinking_content_leaves_no_stale_cells() {
        for index in 0..cases().len() {
            let mut changed = Terminal::new(TestBackend::new(80, 30)).unwrap();
            let mut fresh = Terminal::new(TestBackend::new(80, 30)).unwrap();
            let mut app = App::new();
            app.home = false;
            app.case = index;
            app.tick = 80;
            changed.draw(|f| render(f, &app)).unwrap();
            app.tick = 9;
            app.popup = false;
            changed.draw(|f| render(f, &app)).unwrap();
            fresh.clear().unwrap();
            fresh.draw(|f| render(f, &app)).unwrap();
            assert_eq!(
                visible_cells(changed.backend().buffer()),
                visible_cells(fresh.backend().buffer()),
                "case {}",
                cases()[index].id
            );
        }
    }
}
