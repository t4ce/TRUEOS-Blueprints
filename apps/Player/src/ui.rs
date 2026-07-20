use std::{
    cell::Cell as StateCell,
    io,
    path::Path,
    sync::atomic::{AtomicU8, Ordering},
    time::{Duration, Instant},
};

use anyhow::Result;
use crossterm::{
    cursor::{SetCursorStyle, Show},
    event::{
        self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEvent, KeyModifiers,
        MouseButton, MouseEvent, MouseEventKind,
    },
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{
    Frame, Terminal,
    backend::CrosstermBackend,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    symbols::{Marker as GraphMarker, border},
    text::{Line, Span},
    widgets::{
        Block, Borders, Cell, Chart, Clear, Gauge, List, ListItem, Paragraph, Row, Scrollbar,
        ScrollbarOrientation, ScrollbarState, Table, block::Title,
    },
};
use scope::{
    display::{
        Dimension, DisplayMode, GraphConfig, oscilloscope::Oscilloscope,
        spectroscope::Spectroscope, vectorscope::Vectorscope,
    },
    input::Matrix,
};

#[path = "cmd_boxes.rs"]
mod cmd_boxes;

use crate::{control, playback};

const SPINNER: [&str; 8] = ["⢈", "⡈", "⡐", "⡠", "⣀", "⢄", "⢂", "⢁"];
const HOTKEYS_ENABLED: bool = false;
static HOTKEY_STATE: AtomicU8 = AtomicU8::new(0);
const APP_BG: Color = Color::Rgb(255, 255, 255);
const ACCENT: Color = Color::LightCyan;
const COMMAND_FG: Color = Color::Rgb(0, 0, 0);
const FLASH_FG: Color = Color::Rgb(255, 255, 255);
const FLASH_BG: Color = Color::Rgb(0, 0, 0);
const HOTKEY_FINAL_BG: Color = Color::Rgb(255, 255, 255);
const SYSTEM_BG: Color = Color::Rgb(0, 0, 0);
const SYSTEM_FG: Color = Color::Rgb(255, 255, 255);
const BAR_TIME_FG: Color = Color::Indexed(15);
const BAR_FILL: Color = Color::Rgb(0, 130, 150);
const IDLE_CURSOR_DELAY_MS: u128 = 10_000;
const IDLE_CURSOR_SWEEP_MS: u128 = 3_600;
const PROMPT_PREFIX: &str = "┠╴╶╴ ─╴ ╶── ╶───Prompt ───┤";
const SYSTEM_SUFFIX: &str = "├─── System───╴ ──╴ ╶─ ╶╴╶";
const SYSTEM_EDGE: &str = "┨";

/// Action requested when the terminal UI closes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UiExit {
    /// Restore the terminal and hand control back to the mini-VM shell.
    ReturnToShell,
    /// Shut down the current Blueprint after restoring the terminal.
    Terminate,
}

/// Runs the terminal UI with the provided data/configuration.
pub fn run(config: UiConfig) -> Result<UiExit> {
    tokio_worker_probe();
    let mut terminal = setup_terminal()?;
    let result = App::new(config).run(&mut terminal);
    restore_terminal(&mut terminal)?;
    result
}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
fn tokio_worker_probe() {
    let Ok(runtime) = trueos::runtime::current_thread().build() else {
        return;
    };

    runtime.block_on(async {
        let join = trueos::tokio::task::spawn_blocking(|| 0xA11D_10u32);
        let _ = join.await;
    });
}

#[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
fn tokio_worker_probe() {}

/// Initial state and demo data used by the terminal UI.
#[derive(Debug, Clone)]
pub struct UiConfig {
    pub ready_response: String,
    pub default_file_path: String,
    pub next_file_path: String,
    pub default_track: TrackData,
    pub next_track: TrackData,
    pub initial_volume: u16,
    pub initial_gain: i16,
    pub initial_pitch: i16,
    pub initial_progress_secs: u64,
    pub next_progress_secs: u64,
    pub default_duration_secs: u64,
    pub next_duration_secs: u64,
    pub initial_loop_range: Option<(u64, u64)>,
    pub logs: Vec<String>,
    pub playlist_entries: Vec<PlaylistEntryData>,
}

/// Track metadata displayed in the Playback panel.
#[derive(Debug, Clone)]
pub struct TrackData {
    pub file: String,
    pub album: String,
    pub artist: String,
    pub codec: String,
    pub bitrate: String,
    pub sample_rate: String,
    pub channels: String,
    pub size: String,
}

/// One row in the playlist demo table.
#[derive(Debug, Clone)]
pub struct PlaylistEntryData {
    pub icon: String,
    pub name: String,
    pub kind: String,
    pub duration: String,
    pub size: String,
}

fn setup_terminal() -> Result<Terminal<CrosstermBackend<io::Stdout>>> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(
        stdout,
        EnterAlternateScreen,
        EnableMouseCapture,
        Show,
        SetCursorStyle::BlinkingBlock
    )?;
    let backend = CrosstermBackend::new(stdout);
    Ok(Terminal::new(backend)?)
}

fn restore_terminal(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> Result<()> {
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        DisableMouseCapture,
        SetCursorStyle::DefaultUserShape,
        Show,
        LeaveAlternateScreen
    )?;
    terminal.show_cursor()?;
    Ok(())
}

#[derive(Debug, Clone)]
struct App {
    input: String,
    cursor: usize,
    last_response: String,
    file_path: String,
    track: Track,
    playback: playback::PlaybackEngine,
    playing: bool,
    muted: bool,
    volume: u16,
    gain: i16,
    pitch: i16,
    progress_secs: u64,
    duration_secs: u64,
    labels: Vec<Marker>,
    loop_range: Option<(u64, u64)>,
    logs: Vec<String>,
    spinner_idx: usize,
    recording: bool,
    playlist_visible: bool,
    playlist_scroll: usize,
    playlist_entries: Vec<PlaylistEntry>,
    scope_mode: Option<ScopeMode>,
    scope_phase: f64,
    started_at: Instant,
    last_input_at: Instant,
    idle_cursor_sweep_started_at: Option<Instant>,
    idle_cursor_sweep_done: bool,
    last_tick: Instant,
    last_progress_tick: Instant,
    exit: Option<UiExit>,
    transport_hitbox: StateCell<Option<Rect>>,
    default_file_path: String,
    next_file_path: String,
    default_track: Track,
    next_track: Track,
    default_progress_secs: u64,
    next_progress_secs: u64,
    default_duration_secs: u64,
    next_duration_secs: u64,
}

#[derive(Debug, Clone, Copy)]
enum ScopeMode {
    Dual,
    Spectroscope,
}

impl App {
    fn new(config: UiConfig) -> Self {
        let now = Instant::now();
        let default_track: Track = config.default_track.into();
        let next_track: Track = config.next_track.into();
        Self {
            input: String::new(),
            cursor: 0,
            last_response: config.ready_response,
            file_path: config.default_file_path.clone(),
            track: default_track.clone(),
            playback: playback::PlaybackEngine::new(config.initial_volume),
            playing: false,
            muted: false,
            volume: config.initial_volume,
            gain: config.initial_gain,
            pitch: config.initial_pitch,
            progress_secs: config.initial_progress_secs,
            duration_secs: config.default_duration_secs,
            labels: default_labels(config.default_duration_secs),
            loop_range: config.initial_loop_range,
            logs: config.logs,
            spinner_idx: 0,
            recording: false,
            playlist_visible: false,
            playlist_scroll: 0,
            playlist_entries: config
                .playlist_entries
                .into_iter()
                .map(Into::into)
                .collect(),
            scope_mode: None,
            scope_phase: 0.0,
            started_at: now,
            last_input_at: now,
            idle_cursor_sweep_started_at: None,
            idle_cursor_sweep_done: false,
            last_tick: now,
            last_progress_tick: now,
            exit: None,
            transport_hitbox: StateCell::new(None),
            default_file_path: config.default_file_path,
            next_file_path: config.next_file_path,
            default_track,
            next_track,
            default_progress_secs: config.initial_progress_secs,
            next_progress_secs: config.next_progress_secs,
            default_duration_secs: config.default_duration_secs,
            next_duration_secs: config.next_duration_secs,
        }
    }

    fn run(mut self, terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> Result<UiExit> {
        while self.exit.is_none() {
            terminal.draw(|frame| self.draw(frame))?;

            let timeout = Duration::from_millis(80);
            if event::poll(timeout)? {
                match event::read()? {
                    Event::Key(key) => self.handle_key(key),
                    Event::Mouse(mouse) => self.handle_mouse(mouse),
                    Event::Resize(_, _) => self.transport_hitbox.set(None),
                    _ => {}
                }
            }

            self.tick();
        }

        Ok(self.exit.unwrap_or(UiExit::ReturnToShell))
    }

    fn tick(&mut self) {
        if self.last_tick.elapsed() < Duration::from_millis(90) {
            return;
        }

        self.last_tick = Instant::now();
        self.tick_idle_cursor_sweep();
        if self.scope_mode.is_some() {
            self.scope_phase = (self.scope_phase + 0.18) % std::f64::consts::TAU;
        }
        if self.playing {
            self.spinner_idx = (self.spinner_idx + 1) % SPINNER.len();
            self.tick_playback();
            if self.last_progress_tick.elapsed() < Duration::from_secs(1) {
                return;
            }

            self.last_progress_tick = Instant::now();
            if self.playback.is_loaded() {
                self.progress_secs = self.playback.position_secs().min(self.duration_secs);
            } else {
                self.progress_secs = self.progress_secs.saturating_add(1);
            }

            if let Some((start, end)) = self.loop_range {
                if self.progress_secs >= end {
                    self.progress_secs = start;
                    self.log(format!(
                        "loop: wrapped to {} from {}",
                        fmt_time(start),
                        fmt_time(end)
                    ));
                }
            } else if self.progress_secs >= self.duration_secs {
                self.progress_secs = self.duration_secs;
                self.playing = false;
                self.respond("complete: demo timeline reached the end");
            }
        }
    }

    fn tick_playback(&mut self) {
        match self.playback.feed() {
            Ok(true) => {
                self.playing = false;
                self.progress_secs = self.duration_secs;
                self.respond("complete: playback finished");
            }
            Ok(false) => {}
            Err(err) => {
                self.playing = false;
                self.respond(format!("playback: {err}"));
            }
        }
    }

    fn tick_idle_cursor_sweep(&mut self) {
        if let Some(started_at) = self.idle_cursor_sweep_started_at {
            if started_at.elapsed().as_millis() >= IDLE_CURSOR_SWEEP_MS {
                self.idle_cursor_sweep_started_at = None;
                self.idle_cursor_sweep_done = true;
            }
            return;
        }

        if !self.idle_cursor_sweep_done
            && self.last_input_at.elapsed().as_millis() >= IDLE_CURSOR_DELAY_MS
        {
            self.idle_cursor_sweep_started_at = Some(Instant::now());
        }
    }

    fn record_user_input(&mut self) {
        self.last_input_at = Instant::now();
        self.idle_cursor_sweep_started_at = None;
        self.idle_cursor_sweep_done = false;
    }

    fn handle_key(&mut self, key: KeyEvent) {
        self.record_user_input();
        match key.code {
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.finish(UiExit::ReturnToShell, "quit: ctrl-c")
            }
            KeyCode::Char('q') if HOTKEYS_ENABLED && self.input.is_empty() => {
                self.finish(UiExit::ReturnToShell, "quit: q")
            }
            KeyCode::Esc => {
                self.input.clear();
                self.cursor = 0;
            }
            KeyCode::Enter => self.submit(),
            KeyCode::Backspace => self.backspace(),
            KeyCode::Delete => self.delete(),
            KeyCode::Left => self.cursor = self.cursor.saturating_sub(1),
            KeyCode::Right => self.cursor = (self.cursor + 1).min(self.input.chars().count()),
            KeyCode::Up if self.playlist_visible && self.input.is_empty() => {
                self.scroll_playlist(-1)
            }
            KeyCode::Down if self.playlist_visible && self.input.is_empty() => {
                self.scroll_playlist(1)
            }
            KeyCode::PageUp if self.playlist_visible && self.input.is_empty() => {
                self.scroll_playlist(-12)
            }
            KeyCode::PageDown if self.playlist_visible && self.input.is_empty() => {
                self.scroll_playlist(12)
            }
            KeyCode::Home => self.cursor = 0,
            KeyCode::End => self.cursor = self.input.chars().count(),
            KeyCode::Char(' ') if HOTKEYS_ENABLED && self.input.is_empty() => {
                self.toggle_playback()
            }
            KeyCode::Char('+') if HOTKEYS_ENABLED && self.input.is_empty() => self.adjust_volume(5),
            KeyCode::Char('-') if HOTKEYS_ENABLED && self.input.is_empty() => {
                self.adjust_volume(-5)
            }
            KeyCode::Char('m') if HOTKEYS_ENABLED && self.input.is_empty() => self.toggle_mute(),
            KeyCode::Char('n') if HOTKEYS_ENABLED && self.input.is_empty() => self.next_track(),
            KeyCode::Char('p') if HOTKEYS_ENABLED && self.input.is_empty() => self.prev_track(),
            KeyCode::Char(ch) => self.insert(ch),
            _ => {}
        }
    }

    fn handle_mouse(&mut self, mouse: MouseEvent) {
        if mouse.kind != MouseEventKind::Down(MouseButton::Left) {
            return;
        }

        let Some(hitbox) = self.transport_hitbox.get() else {
            return;
        };
        if !rect_contains(hitbox, mouse.column, mouse.row) {
            return;
        }

        self.record_user_input();
        self.toggle_playback();
    }

    fn submit(&mut self) {
        let command = self.input.trim().to_string();
        if command.is_empty() {
            return;
        }

        self.log(format!("> {command}"));
        self.run_command(&command);
        self.input.clear();
        self.cursor = 0;
    }

    fn run_command(&mut self, command: &str) {
        match control::parse_command(command) {
            Ok(event) => control::dispatch(self, &event),
            Err(control::ParseCommandError::Empty) => {}
            Err(control::ParseCommandError::Unknown(action)) => {
                self.respond(format!("{action}: command logged but not wired"));
            }
            Err(control::ParseCommandError::Ambiguous { input, matches }) => {
                self.respond(format!("{input}: ambiguous, matches {}", matches.join("/")));
            }
        }
    }

    fn draw(&self, frame: &mut Frame) {
        self.transport_hitbox.set(None);
        HOTKEY_STATE.store(self.hotkey_state(), Ordering::Relaxed);
        let area = frame.area();
        frame.render_widget(Clear, area);
        frame.render_widget(Paragraph::new("").style(Style::default().bg(APP_BG)), area);

        let loaded_file = Path::new(&self.file_path)
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or(self.file_path.as_str());
        let outer = Block::default()
            .title(Title::from(Line::from(vec![
                Span::styled("━", Style::default().fg(Color::DarkGray)),
                icon_span("🎞"),
                Span::styled(
                    " Player ",
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled("╺  ╺ ╺╸ ━╺━━", Style::default().fg(Color::DarkGray)),
            ])))
            .title(Title::from(
                Line::from(vec![
                    Span::styled("━━╋  ", Style::default().fg(Color::DarkGray)),
                    Span::styled(
                        loaded_file.to_string(),
                        Style::default().fg(COMMAND_FG).add_modifier(Modifier::BOLD),
                    ),
                    Span::styled("  ╋━━", Style::default().fg(Color::DarkGray)),
                ])
                .centered(),
            ))
            .borders(Borders::ALL)
            .border_set(border::THICK)
            .border_style(Style::default().fg(Color::DarkGray));
        let inner = outer.inner(area);
        frame.render_widget(outer, area);

        let compact = inner.width < 112 || inner.height < 28;
        let labels_visible = self.labels_visible();
        if compact {
            if labels_visible {
                let root = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([
                        Constraint::Length(7),
                        Constraint::Length(2),
                        Constraint::Length(3),
                        Constraint::Min(10),
                    ])
                    .split(inner);

                cmd_boxes::draw_compact(frame, root[0]);
                self.draw_prompt(frame, root[1]);
                self.draw_labels(frame, root[2]);
                self.draw_main(frame, root[3]);
            } else {
                let root = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([
                        Constraint::Length(7),
                        Constraint::Length(2),
                        Constraint::Min(10),
                    ])
                    .split(inner);

                cmd_boxes::draw_compact(frame, root[0]);
                self.draw_prompt(frame, root[1]);
                self.draw_main(frame, root[2]);
            }
        } else {
            if labels_visible {
                let root = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([
                        Constraint::Length(6),
                        Constraint::Length(1),
                        Constraint::Length(3),
                        Constraint::Min(13),
                    ])
                    .split(inner);

                cmd_boxes::draw(frame, root[0]);
                self.draw_prompt(frame, root[1]);
                self.draw_labels(frame, root[2]);
                self.draw_main(frame, root[3]);
            } else {
                let root = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([
                        Constraint::Length(6),
                        Constraint::Length(1),
                        Constraint::Min(13),
                    ])
                    .split(inner);

                cmd_boxes::draw(frame, root[0]);
                self.draw_prompt(frame, root[1]);
                self.draw_main(frame, root[2]);
            }
        }
    }

    fn draw_prompt(&self, frame: &mut Frame, area: Rect) {
        let prompt_bg = APP_BG;
        let prompt_input_bg = Color::Rgb(238, 238, 238);
        let system_bg = SYSTEM_BG;
        let prompt_label = Style::default()
            .fg(Color::DarkGray)
            .bg(prompt_bg)
            .add_modifier(Modifier::BOLD);
        let prompt_input = Style::default().fg(COMMAND_FG).bg(prompt_input_bg);
        let system_label = Style::default()
            .fg(COMMAND_FG)
            .bg(APP_BG)
            .add_modifier(Modifier::BOLD);
        let system_edge = Style::default()
            .fg(Color::DarkGray)
            .bg(APP_BG)
            .add_modifier(Modifier::BOLD);
        let system_text = Style::default().fg(SYSTEM_FG).bg(system_bg);

        if area.height <= 1 {
            let columns = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
                .split(area);
            let prompt_area = extend_left(columns[0], 1);

            frame.render_widget(
                Paragraph::new("").style(Style::default().bg(prompt_bg)),
                columns[0],
            );
            frame.render_widget(
                Paragraph::new("").style(Style::default().bg(system_bg)),
                columns[1],
            );

            let sweep_col = self.idle_cursor_sweep_col(prompt_area.width);
            let prompt = self.prompt_line(prompt_label, prompt_input);
            self.draw_prompt_input_bg(frame, prompt_area, prompt_input_bg);
            frame.render_widget(Paragraph::new(prompt), prompt_area);
            self.place_prompt_cursor(frame, prompt_area, sweep_col);

            let response = Line::from(vec![
                Span::styled(&self.last_response, system_text),
                Span::styled(SYSTEM_SUFFIX, system_label),
                Span::styled(SYSTEM_EDGE, system_edge),
            ])
            .right_aligned();
            frame.render_widget(Paragraph::new(response), extend_right(columns[1], 1));

            return;
        }

        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(1), Constraint::Length(1)])
            .split(area);
        let prompt_area = extend_left(rows[0], 1);

        frame.render_widget(
            Paragraph::new("").style(Style::default().bg(prompt_bg)),
            rows[0],
        );
        frame.render_widget(
            Paragraph::new("").style(Style::default().bg(system_bg)),
            rows[1],
        );

        let sweep_col = self.idle_cursor_sweep_col(prompt_area.width);
        let prompt = self.prompt_line(prompt_label, prompt_input);
        self.draw_prompt_input_bg(frame, prompt_area, prompt_input_bg);
        frame.render_widget(Paragraph::new(prompt), prompt_area);
        self.place_prompt_cursor(frame, prompt_area, sweep_col);

        let response = Line::from(vec![
            Span::styled(&self.last_response, system_text),
            Span::styled(SYSTEM_SUFFIX, system_label),
            Span::styled(SYSTEM_EDGE, system_edge),
        ])
        .right_aligned();
        frame.render_widget(Paragraph::new(response), extend_right(rows[1], 1));
    }

    fn prompt_line<'a>(&'a self, prompt_label: Style, prompt_input: Style) -> Line<'a> {
        let cursor_byte = self.byte_index(self.cursor);
        let (before, after) = self.input.split_at(cursor_byte);

        Line::from(vec![
            Span::styled(PROMPT_PREFIX, prompt_label),
            Span::styled(before, prompt_input),
            Span::styled(after, prompt_input),
        ])
    }

    fn draw_prompt_input_bg(&self, frame: &mut Frame, area: Rect, input_bg: Color) {
        let start = self.prompt_cursor_col().saturating_sub(self.cursor as u16);
        if start >= area.width {
            return;
        }

        let input_area = Rect {
            x: area.x + start,
            width: area.width.saturating_sub(start),
            ..area
        };
        frame.render_widget(
            Paragraph::new("").style(Style::default().bg(input_bg)),
            input_area,
        );
    }

    fn place_prompt_cursor(&self, frame: &mut Frame, area: Rect, sweep_col: Option<u16>) {
        if area.width == 0 {
            return;
        }

        let col = match sweep_col {
            Some(col) => col,
            None => self.prompt_cursor_col(),
        };

        frame.set_cursor_position((area.x + col.min(area.width.saturating_sub(1)), area.y));
    }

    fn draw_labels(&self, frame: &mut Frame, area: Rect) {
        let compact = area.width < 90;
        let mut line = Vec::new();
        for (idx, marker) in self.labels.iter().enumerate() {
            if idx > 0 {
                line.push(Span::styled(
                    if compact { "  " } else { "  |  " },
                    Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
                ));
            }

            if !compact {
                line.push(icon_span("⏱"));
                line.push(accent_span(" "));
            }
            line.push(accent_span_owned(marker.name.clone()));
            line.push(Span::styled(
                format!(" {}", fmt_time(marker.seconds)),
                plain_style().fg(ACCENT),
            ));
        }

        let block = Block::default()
            .title("─Labels ─")
            .title_style(block_title_style())
            .borders(Borders::ALL)
            .border_set(border::ROUNDED)
            .border_style(Style::default().fg(Color::DarkGray));
        frame.render_widget(Paragraph::new(Line::from(line)).block(block), area);
    }

    fn draw_main(&self, frame: &mut Frame, area: Rect) {
        if let Some(mode) = self.scope_mode {
            self.draw_scope(frame, area, mode);
            return;
        }

        if self.playlist_visible {
            self.draw_playlist(frame, area);
            return;
        }

        let columns = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Length(19), Constraint::Min(40)])
            .split(area);

        self.draw_track(frame, columns[0]);
        self.draw_status(frame, columns[1]);
    }

    fn draw_scope(&self, frame: &mut Frame, area: Rect, mode: ScopeMode) {
        let cfg = GraphConfig {
            samples: 256,
            sampling_rate: 48_000,
            scale: 1.2,
            width: area.width as u32,
            scatter: false,
            references: true,
            show_ui: false,
            marker_type: GraphMarker::Braille,
            palette: vec![Color::LightCyan, Color::LightMagenta],
            labels_color: Color::Yellow,
            axis_color: Color::DarkGray,
            ..GraphConfig::default()
        };
        let data = self.scope_data(cfg.samples as usize);

        match mode {
            ScopeMode::Dual => {
                let panes = Layout::default()
                    .direction(Direction::Horizontal)
                    .constraints([Constraint::Percentage(58), Constraint::Percentage(42)])
                    .split(area);
                let mut oscillo = Oscilloscope::default();
                self.draw_scope_chart(
                    frame,
                    panes[0],
                    &cfg,
                    &data,
                    &mut oscillo,
                    "Scope",
                    Some("time"),
                );
                let mut scope = Vectorscope::default();
                self.draw_scope_chart(frame, panes[1], &cfg, &data, &mut scope, "Vector", None);
            }
            ScopeMode::Spectroscope => {
                let mut scope = Spectroscope {
                    sampling_rate: cfg.sampling_rate,
                    buffer_size: cfg.samples,
                    average: 1,
                    buf: Vec::new(),
                    window: true,
                    log_y: true,
                    phase_diff: false,
                };
                self.draw_scope_chart(frame, area, &cfg, &data, &mut scope, "Spectro", None);
            }
        }
    }

    fn draw_scope_chart<M: DisplayMode>(
        &self,
        frame: &mut Frame,
        area: Rect,
        cfg: &GraphConfig,
        data: &Matrix<f64>,
        mode: &mut M,
        title: &str,
        x_label: Option<&str>,
    ) {
        let mut series = mode.references(cfg);
        series.extend(mode.process(cfg, data));
        let datasets = series.iter().map(Into::into).collect::<Vec<_>>();

        let block = Block::default()
            .title(format!("─{title} ─"))
            .title_style(block_title_style())
            .borders(Borders::ALL)
            .border_set(border::ROUNDED)
            .border_style(Style::default().fg(Color::DarkGray));

        let chart = Chart::new(datasets)
            .block(block)
            .x_axis(mode.axis(cfg, Dimension::X))
            .y_axis(mode.axis(cfg, Dimension::Y));
        frame.render_widget(chart, area);

        if let Some(label) = x_label {
            let label_area = Rect {
                x: area.x.saturating_add(1),
                y: area.bottom().saturating_sub(2),
                width: area.width.saturating_sub(2),
                height: 1,
            };
            frame.render_widget(
                Paragraph::new(label)
                    .alignment(Alignment::Center)
                    .style(Style::default().fg(COMMAND_FG)),
                label_area,
            );
        }
    }

    fn draw_playlist(&self, frame: &mut Frame, area: Rect) {
        let visible_rows = area.height.saturating_sub(3) as usize;
        let max_scroll = self
            .playlist_entries
            .len()
            .saturating_sub(visible_rows.max(1));
        let scroll = self.playlist_scroll.min(max_scroll);
        let end = (scroll + visible_rows).min(self.playlist_entries.len());
        let rows = self.playlist_entries[scroll..end]
            .iter()
            .enumerate()
            .map(|(idx, entry)| {
                let absolute = scroll + idx + 1;
                Row::new(vec![
                    Cell::from(format!("{absolute:03}")),
                    Cell::from(entry.icon.as_str()),
                    Cell::from(entry.name.as_str()),
                    Cell::from(entry.kind.as_str()),
                    Cell::from(entry.duration.as_str()),
                    Cell::from(entry.size.as_str()),
                ])
            })
            .collect::<Vec<_>>();

        let block = Block::default()
            .title(format!(
                "─Playlist  /apps/scope/tui/folder  {} entries ─",
                self.playlist_entries.len()
            ))
            .title_style(block_title_style())
            .borders(Borders::ALL)
            .border_set(border::ROUNDED)
            .border_style(Style::default().fg(Color::DarkGray));
        let inner = inset_x(block.inner(area), 1);

        let table = Table::new(
            rows,
            [
                Constraint::Length(5),
                Constraint::Length(3),
                Constraint::Min(22),
                Constraint::Length(10),
                Constraint::Length(9),
                Constraint::Length(9),
            ],
        )
        .header(
            Row::new(vec!["#", "", "name", "kind", "duration", "size"]).style(
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
        )
        .block(block)
        .style(Style::default().fg(Color::Gray))
        .column_spacing(1);
        frame.render_widget(table, area);

        let mut scrollbar_state = ScrollbarState::new(self.playlist_entries.len()).position(scroll);
        let scrollbar = Scrollbar::default()
            .orientation(ScrollbarOrientation::VerticalRight)
            .thumb_style(Style::default().fg(Color::Yellow))
            .track_style(Style::default().fg(Color::DarkGray));
        frame.render_stateful_widget(scrollbar, inner, &mut scrollbar_state);
    }

    fn draw_track(&self, frame: &mut Frame, area: Rect) {
        let fields = [
            ("file", self.track.file.clone()),
            ("album", self.track.album.clone()),
            ("artist", self.track.artist.clone()),
            ("codec", self.track.codec.clone()),
            ("bitrate", self.track.bitrate.clone()),
            ("sample", self.track.sample_rate.clone()),
            ("channels", self.track.channels.clone()),
            ("size", self.track.size.clone()),
            ("gain", format!("{} dB", signed(self.gain))),
            ("pitch", format!("{} st", signed(self.pitch))),
        ];
        let block = Block::default()
            .title("─Playback ─")
            .title_style(block_title_style())
            .borders(Borders::ALL)
            .border_set(border::ROUNDED)
            .border_style(Style::default().fg(Color::DarkGray));
        let inner = inset_x(block.inner(area), 1);

        let mut lines = Vec::new();
        for (label, value) in fields {
            lines.push(Line::from(Span::styled(
                label,
                plain_style().fg(Color::Gray),
            )));
            lines.extend(wrapped_track_value_lines(&value, inner.width));
        }

        frame.render_widget(block, area);
        frame.render_widget(Paragraph::new(lines), inner);
    }

    fn draw_status(&self, frame: &mut Frame, area: Rect) {
        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(6),
                Constraint::Length(4),
                Constraint::Min(2),
            ])
            .split(area);

        let status_columns = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Length(12), Constraint::Min(12)])
            .split(rows[0]);

        self.draw_transport_slot(frame, status_columns[0]);
        self.draw_status_panel(frame, status_columns[1]);
        self.draw_meters(frame, rows[1]);
        self.draw_log(frame, rows[2]);
    }

    fn draw_transport_slot(&self, frame: &mut Frame, area: Rect) {
        let symbol = if self.playing { "▶" } else { "⏸" };
        let (top_title, bottom_title) = transport_titles(
            symbol,
            area.width.saturating_sub(2) as usize,
            (self.started_at.elapsed().as_millis() / 180) as usize,
        );
        let block = Block::default()
            .title_top(top_title)
            .title_bottom(bottom_title)
            .borders(Borders::ALL)
            .border_set(border::ROUNDED)
            .border_style(Style::default().fg(Color::DarkGray));
        let inner = block.inner(area);
        self.transport_hitbox.set(Some(inner));
        frame.render_widget(block, area);

        let hot = Style::default().fg(SYSTEM_BG).add_modifier(Modifier::BOLD);
        let lines = if self.playing {
            vec![
                Line::from(Span::styled("   █▄     ", hot)),
                Line::from(Span::styled("   ████▄  ", hot)),
                Line::from(Span::styled("   ████▀  ", hot)),
                Line::from(Span::styled("   █▀     ", hot)),
            ]
        } else {
            vec![
                Line::from(Span::styled(" ▐██  ██▌ ", hot)),
                Line::from(Span::styled(" ▐██  ██▌ ", hot)),
                Line::from(Span::styled(" ▐██  ██▌ ", hot)),
                Line::from(Span::styled(" ▝▀▀  ▀▀▘ ", hot)),
            ]
        };

        frame.render_widget(Paragraph::new(lines).alignment(Alignment::Center), inner);
    }

    fn draw_status_panel(&self, frame: &mut Frame, area: Rect) {
        let block = Block::default()
            .title("─Status ─")
            .title_style(block_title_style())
            .borders(Borders::ALL)
            .border_set(border::ROUNDED)
            .border_style(Style::default().fg(Color::DarkGray));
        let inner = inset_x(block.inner(area), 1);
        frame.render_widget(block, area);

        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1),
                Constraint::Length(1),
                Constraint::Length(2),
            ])
            .split(inner);

        self.draw_status_header(frame, rows[0]);
        self.draw_progress(frame, rows[1]);
        self.draw_marker_row(frame, rows[2]);
    }

    fn draw_status_header(&self, frame: &mut Frame, area: Rect) {
        let spinner = if self.playing {
            SPINNER[self.spinner_idx]
        } else {
            "·"
        };
        let samples_total = 10_833_502_u64;
        let samples_now =
            samples_total.saturating_mul(self.progress_secs) / self.duration_secs.max(1);
        let record = if self.recording { "  REC armed" } else { "" };

        let content = if area.width < 60 {
            let left = vec![Span::styled(
                format!("{spinner} "),
                Style::default()
                    .fg(Color::LightGreen)
                    .add_modifier(Modifier::BOLD),
            )];
            let right = vec![
                Span::styled(
                    samples_now.to_string(),
                    Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
                ),
                Span::raw("  "),
                Span::styled(
                    format!("{}%", self.progress_percent()),
                    Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    record,
                    Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                ),
                Span::raw(" "),
            ];
            justified_line(left, right, area.width)
        } else {
            let left = vec![Span::styled(
                format!("{spinner} "),
                Style::default()
                    .fg(Color::LightGreen)
                    .add_modifier(Modifier::BOLD),
            )];
            let right = vec![
                Span::styled(
                    format!("{samples_now}/{samples_total}"),
                    Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
                ),
                Span::raw("  "),
                Span::styled(
                    format!("{}%", self.progress_percent()),
                    Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    record,
                    Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                ),
                Span::raw(" "),
            ];
            justified_line(left, right, area.width)
        };

        frame.render_widget(Paragraph::new(content), area);
    }

    fn draw_progress(&self, frame: &mut Frame, area: Rect) {
        let area = Rect {
            x: area.x.saturating_add(1),
            width: area.width.saturating_sub(2),
            ..area
        };
        let width = usize::from(area.width);
        if width == 0 {
            return;
        }
        if width == 1 {
            frame.render_widget(
                Paragraph::new(Span::styled("█", Style::default().fg(BAR_FILL))),
                area,
            );
            return;
        }

        let ratio = self.progress_secs as f64 / self.duration_secs.max(1) as f64;
        let filled = ((width.saturating_sub(2) as f64) * ratio).round() as usize;
        let empty = width.saturating_sub(2).saturating_sub(filled);
        let right_cap = if ratio >= 1.0 {
            BAR_FILL
        } else {
            Color::DarkGray
        };

        let mut cells = Vec::with_capacity(width);
        cells.push(BarCell::new('▌', BAR_FILL));
        cells.extend((0..filled).map(|_| BarCell::new('█', BAR_FILL)));
        cells.extend((0..empty).map(|_| BarCell::new('█', Color::DarkGray)));
        cells.push(BarCell::new('▐', right_cap));

        let elapsed = fmt_time(self.progress_secs);
        let duration = fmt_time(self.duration_secs);
        let duration_start = width.saturating_sub(1 + duration.chars().count());
        place_bar_text(&mut cells, duration_start, &duration, Color::DarkGray);

        let elapsed_start = if width > 2 {
            let elapsed_width = elapsed.chars().count();
            let pos = 1 + filled.saturating_sub(elapsed_width);
            let max_start = duration_start.saturating_sub(elapsed.chars().count() + 1);
            pos.min(max_start).max(1)
        } else {
            0
        };
        place_bar_text(&mut cells, elapsed_start, &elapsed, BAR_FILL);

        let line = Line::from(
            cells
                .into_iter()
                .map(|cell| Span::styled(cell.text, cell.style))
                .collect::<Vec<_>>(),
        );
        frame.render_widget(Paragraph::new(line), area);
    }

    fn draw_marker_row(&self, frame: &mut Frame, area: Rect) {
        let area = Rect {
            x: area.x.saturating_add(1),
            width: area.width.saturating_sub(2),
            ..area
        };
        let width = area.width.max(1) as usize;
        let mut arrows = vec![' '; width];
        let mut names = vec![' '; width];

        for marker in &self.labels {
            let pos = marker_position(marker.seconds, self.duration_secs, width);
            if pos < arrows.len() {
                arrows[pos] = '⬆';
                place_centered(&mut names, pos, &marker.name);
            }
        }

        let content = vec![
            Line::from(Span::styled(
                arrows.iter().collect::<String>(),
                Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
            )),
            Line::from(Span::styled(
                names.iter().collect::<String>(),
                Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
            )),
        ];
        frame.render_widget(Paragraph::new(content).alignment(Alignment::Left), area);
    }

    fn draw_meters(&self, frame: &mut Frame, area: Rect) {
        let (output_icon, output_text, output_color) = if self.muted {
            ("🔇", "muted", Color::Red)
        } else {
            ("🔈", "live", Color::LightGreen)
        };
        let output_title = Title::from(
            Line::from(vec![
                Span::styled("──┤", Style::default().fg(Color::DarkGray)),
                icon_span(output_icon),
                Span::styled(
                    output_text,
                    Style::default()
                        .fg(output_color)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled("├───", Style::default().fg(Color::DarkGray)),
            ])
            .centered(),
        );
        let block = Block::default()
            .title("─Volume ─")
            .title(output_title)
            .title_style(block_title_style())
            .borders(Borders::ALL)
            .border_set(border::ROUNDED)
            .border_style(Style::default().fg(Color::DarkGray));
        let inner = inset_x(block.inner(area), 1);
        frame.render_widget(block, area);

        let l = if self.muted { 0 } else { self.volume as u64 };
        let r = if self.muted {
            0
        } else {
            self.volume.saturating_sub(3) as u64
        };
        let channel_rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(1), Constraint::Length(1)])
            .split(inner);
        self.draw_channel_meter(frame, channel_rows[0], "L", l);
        self.draw_channel_meter(frame, channel_rows[1], "R", r);
    }

    fn draw_channel_meter(&self, frame: &mut Frame, area: Rect, label: &str, value: u64) {
        let columns = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Length(3), Constraint::Min(8)])
            .split(area);

        frame.render_widget(
            Paragraph::new(label).style(Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)),
            columns[0],
        );
        let gauge = Gauge::default()
            .gauge_style(Style::default().fg(BAR_FILL).bg(Color::DarkGray))
            .ratio(value as f64 / 100.0)
            .label("");
        frame.render_widget(gauge, columns[1]);

        let percent = format!("{}%", value);
        let percent_area = Rect {
            x: columns[1].x.saturating_add(1),
            width: columns[1].width.saturating_sub(1),
            ..columns[1]
        };
        frame.render_widget(
            Paragraph::new(Span::styled(
                percent,
                Style::reset().fg(BAR_TIME_FG).bg(BAR_FILL),
            )),
            percent_area,
        );

        let db = if self.muted {
            "-inf dB".to_string()
        } else {
            format!("{:.1} dB", value as f64 * 0.48 - 30.0)
        };
        let db_width = db.chars().count() as u16;
        let db_area = Rect {
            x: columns[1].right().saturating_sub(db_width + 1),
            width: db_width,
            ..columns[1]
        };
        frame.render_widget(
            Paragraph::new(Span::styled(
                db,
                Style::reset().fg(BAR_TIME_FG).bg(Color::DarkGray),
            )),
            db_area,
        );
    }

    fn draw_log(&self, frame: &mut Frame, area: Rect) {
        let block = Block::default()
            .title("─Log ─")
            .title_style(block_title_style())
            .borders(Borders::ALL)
            .border_set(border::ROUNDED)
            .border_style(Style::default().fg(Color::DarkGray));
        let inner = inset_x(block.inner(area), 1);
        if inner.height == 0 {
            return;
        }

        frame.render_widget(block, area);

        let visible = inner.height as usize;
        let start = self.logs.len().saturating_sub(visible);
        let items = self.logs[start..]
            .iter()
            .map(|item| ListItem::new(item.as_str()))
            .collect::<Vec<_>>();

        let list = List::new(items).style(Style::default().fg(Color::Gray));
        frame.render_widget(list, inner);
    }

    fn insert(&mut self, ch: char) {
        let byte = self.byte_index(self.cursor);
        self.input.insert(byte, ch);
        self.cursor += 1;
    }

    fn backspace(&mut self) {
        if self.cursor == 0 {
            return;
        }

        let byte = self.byte_index(self.cursor);
        let prev = self.byte_index(self.cursor - 1);
        self.input.replace_range(prev..byte, "");
        self.cursor -= 1;
    }

    fn delete(&mut self) {
        if self.cursor >= self.input.chars().count() {
            return;
        }

        let start = self.byte_index(self.cursor);
        let end = self.byte_index(self.cursor + 1);
        self.input.replace_range(start..end, "");
    }

    fn byte_index(&self, char_index: usize) -> usize {
        self.input
            .char_indices()
            .nth(char_index)
            .map(|(index, _)| index)
            .unwrap_or(self.input.len())
    }

    fn toggle_playback(&mut self) {
        if self.playing {
            self.pause_playback();
        } else {
            self.play_loaded();
        }
    }

    fn toggle_mute(&mut self) {
        self.muted = !self.muted;
        if self.muted {
            self.respond("mute: output marked silent");
        } else {
            self.respond("unmute: output restored");
        }
    }

    fn adjust_volume(&mut self, delta: i16) {
        self.set_volume(self.volume as i16 + delta);
    }

    fn set_volume(&mut self, volume: i16) {
        self.volume = volume.clamp(0, 100) as u16;
        self.respond(format!("volume: {}%", self.volume));
    }

    fn show_mainview(&mut self) {
        self.playlist_visible = false;
        self.scope_mode = None;
        self.respond("mainview: playback view restored");
    }

    fn scroll_playlist(&mut self, delta: isize) {
        let max_scroll = self.playlist_entries.len().saturating_sub(1);
        self.playlist_scroll = self
            .playlist_scroll
            .saturating_add_signed(delta)
            .min(max_scroll);
    }

    fn scope_data(&self, samples: usize) -> Matrix<f64> {
        let mut left = Vec::with_capacity(samples);
        let mut right = Vec::with_capacity(samples);

        for index in 0..samples {
            let t = index as f64 / samples.max(1) as f64;
            let sweep = self.scope_phase + (t * std::f64::consts::TAU);
            let l = (sweep * 3.0).sin() * 0.70 + (sweep * 11.0).sin() * 0.18;
            let r = (sweep * 2.0 + 0.8).sin() * 0.55 + (sweep * 7.0).cos() * 0.24;
            left.push(l);
            right.push(r);
        }

        vec![left, right]
    }

    fn seek(&mut self, seconds: u64) {
        self.progress_secs = seconds.min(self.duration_secs);
        self.respond(format!("goto: {}", fmt_time(self.progress_secs)));
    }

    fn next_track(&mut self) {
        self.playback.clear();
        self.track = self.next_track.clone();
        self.file_path.clone_from(&self.next_file_path);
        self.progress_secs = self.next_progress_secs;
        self.duration_secs = self.next_duration_secs;
        self.labels = default_labels(self.next_duration_secs);
        self.respond("next: loaded demo metadata only");
    }

    fn prev_track(&mut self) {
        self.playback.clear();
        self.track = self.default_track.clone();
        self.file_path.clone_from(&self.default_file_path);
        self.progress_secs = self.default_progress_secs;
        self.duration_secs = self.default_duration_secs;
        self.labels = default_labels(self.default_duration_secs);
        self.respond("prev: restored first demo metadata");
    }

    fn load_path(&mut self, path: String) -> std::result::Result<(), String> {
        let loaded = self
            .playback
            .load_path(&path)
            .map_err(|err| format!("load: {err}"))?;

        self.file_path = loaded.path;
        self.track.file = loaded.file_name;
        self.track.codec = loaded.codec;
        self.track.sample_rate = "48 kHz".into();
        self.track.channels = "stereo".into();
        self.track.size = loaded.size_label;
        self.duration_secs = loaded.duration_secs.max(1);
        self.progress_secs = 0;
        self.playing = false;
        self.respond(format!("load: {} frames ready", loaded.frames));
        Ok(())
    }

    fn play_loaded(&mut self) {
        if !self.playback.is_loaded() {
            if let Err(err) = self.load_path(self.file_path.clone()) {
                self.respond(err);
                return;
            }
        }

        match self.playback.play() {
            Ok(()) => {
                self.playing = true;
                self.last_progress_tick = Instant::now();
                self.respond("play: playback started");
            }
            Err(err) => {
                self.playing = false;
                self.respond(format!("play: {err}"));
            }
        }
    }

    fn pause_playback(&mut self) {
        match self.playback.pause() {
            Ok(()) => {
                self.playing = false;
                self.respond("pause: playback paused");
            }
            Err(err) => {
                self.playing = false;
                self.respond(format!("pause: {err}"));
            }
        }
    }

    fn finish(&mut self, exit: UiExit, message: impl Into<String>) {
        self.playback.close_stream();
        self.respond(message);
        self.exit = Some(exit);
    }

    fn respond(&mut self, message: impl Into<String>) {
        let message = message.into();
        self.last_response = compact_response(&message);
        self.log(message);
    }

    fn log(&mut self, message: impl Into<String>) {
        self.logs.push(message.into());
        if self.logs.len() > 200 {
            self.logs.remove(0);
        }
    }

    fn progress_percent(&self) -> u64 {
        self.progress_secs.saturating_mul(100) / self.duration_secs.max(1)
    }

    fn labels_visible(&self) -> bool {
        self.scope_mode.is_none()
            && !self.playlist_visible
            && self.labels.iter().any(|marker| {
                let is_start = marker.name == "S" && marker.seconds == 0;
                let is_end = marker.name == "E" && marker.seconds == self.duration_secs;
                !(is_start || is_end)
            })
    }

    fn hotkey_state(&self) -> u8 {
        let elapsed = self.started_at.elapsed();
        let elapsed_ms = elapsed.as_millis();

        if elapsed_ms < 1500 {
            return 0;
        }

        let blink_ms = elapsed_ms - 1500;
        if blink_ms < 900 {
            return if (blink_ms / 150) % 2 == 0 { 1 } else { 0 };
        }

        2
    }

    fn idle_cursor_sweep_col(&self, width: u16) -> Option<u16> {
        let started_at = self.idle_cursor_sweep_started_at?;
        if width == 0 {
            return None;
        }

        let elapsed = started_at.elapsed().as_millis();
        if elapsed >= IDLE_CURSOR_SWEEP_MS {
            return None;
        }

        let start_col = self.prompt_cursor_col().min(width.saturating_sub(1));
        let max_col = width.saturating_sub(1);
        let travel = u128::from(max_col.saturating_sub(start_col));
        let half = IDLE_CURSOR_SWEEP_MS / 2;
        let offset = if elapsed <= half {
            elapsed.saturating_mul(travel) / half
        } else {
            IDLE_CURSOR_SWEEP_MS
                .saturating_sub(elapsed)
                .saturating_mul(travel)
                / half
        };

        Some(start_col.saturating_add(offset as u16))
    }

    fn prompt_cursor_col(&self) -> u16 {
        PROMPT_PREFIX
            .chars()
            .count()
            .try_into()
            .unwrap_or(u16::MAX)
            .saturating_add(self.cursor as u16)
    }
}

impl control::ControlEventHandler for App {
    fn on_load(&mut self, event: &control::ParsedCommand) {
        let path = event.rest();
        if path.is_empty() {
            self.respond("load: missing path");
        } else if let Err(err) = self.load_path(path) {
            self.respond(err);
        }
    }

    fn on_play(&mut self, _event: &control::ParsedCommand) {
        self.play_loaded();
    }

    fn on_pause(&mut self, _event: &control::ParsedCommand) {
        self.pause_playback();
    }

    fn on_quit(&mut self, _event: &control::ParsedCommand) {
        self.finish(UiExit::ReturnToShell, "quit: returning to mini-VM shell");
    }

    fn on_terminate(&mut self, _event: &control::ParsedCommand) {
        self.finish(UiExit::Terminate, "terminate: shutting down Blueprint");
    }

    fn on_mainview(&mut self, _event: &control::ParsedCommand) {
        self.show_mainview();
    }

    fn on_next(&mut self, _event: &control::ParsedCommand) {
        self.next_track();
    }

    fn on_prev(&mut self, _event: &control::ParsedCommand) {
        self.prev_track();
    }

    fn on_goto(&mut self, event: &control::ParsedCommand) {
        match event.arg(0).and_then(parse_time) {
            Some(seconds) => self.seek(seconds),
            None => self.respond("goto: use mm:ss or seconds"),
        }
    }

    fn on_label(&mut self, event: &control::ParsedCommand) {
        let (name, seconds) = match (event.arg(0), event.arg(1)) {
            (None, _) => ("L", self.progress_secs),
            (Some(raw_time), None) if raw_time.starts_with('-') => {
                self.respond(format!(
                    "label: time must be between 00:00 and {}",
                    fmt_time(self.duration_secs)
                ));
                return;
            }
            (Some(raw_time), None) if raw_time.contains(':') || raw_time.parse::<u64>().is_ok() => {
                let Some(seconds) = parse_time(raw_time) else {
                    self.respond(format!(
                        "label: time must be between 00:00 and {}",
                        fmt_time(self.duration_secs)
                    ));
                    return;
                };
                ("L", seconds)
            }
            (Some(name), None) => (name, self.progress_secs),
            (Some(name), Some(raw_time)) => {
                let Some(seconds) = parse_time(raw_time) else {
                    self.respond(format!(
                        "label: time must be between 00:00 and {}",
                        fmt_time(self.duration_secs)
                    ));
                    return;
                };
                (name, seconds)
            }
        };

        if seconds > self.duration_secs {
            self.respond(format!(
                "label: time must be between 00:00 and {}",
                fmt_time(self.duration_secs)
            ));
            return;
        }

        self.labels.push(Marker::new(name, seconds));
        self.respond(format!("label: {name} at {}", fmt_time(seconds)));
    }

    fn on_loop(&mut self, event: &control::ParsedCommand) {
        let start = event.arg(0).and_then(parse_time);
        let end = event.arg(1).and_then(parse_time);

        match (start, end) {
            (Some(start), Some(end)) if start < end => {
                self.loop_range = Some((start, end.min(self.duration_secs)));
                self.respond(format!("loop: {} to {}", fmt_time(start), fmt_time(end)));
            }
            _ => self.respond("loop: use loop <start> <end>, for example loop 0:12 1:05"),
        }
    }

    fn on_clear_loop(&mut self, _event: &control::ParsedCommand) {
        self.loop_range = None;
        self.respond("loop: cleared");
    }

    fn on_volume(&mut self, event: &control::ParsedCommand) {
        let Some(value) = event.arg(0) else {
            self.respond(format!("volume: {}%", self.volume));
            return;
        };

        match value {
            "+" | "up" => self.adjust_volume(5),
            "-" | "down" => self.adjust_volume(-5),
            raw => match raw.parse::<i16>() {
                Ok(value) => self.set_volume(value),
                Err(_) => self.respond("volume: use vol +, vol -, or vol <0-100>"),
            },
        }
    }

    fn on_volume_up(&mut self, _event: &control::ParsedCommand) {
        self.adjust_volume(5);
    }

    fn on_volume_down(&mut self, _event: &control::ParsedCommand) {
        self.adjust_volume(-5);
    }

    fn on_mute(&mut self, _event: &control::ParsedCommand) {
        self.muted = true;
        self.respond("mute: output marked silent");
    }

    fn on_unmute(&mut self, _event: &control::ParsedCommand) {
        self.muted = false;
        self.respond("unmute: output restored");
    }

    fn on_gain(&mut self, event: &control::ParsedCommand) {
        match event.arg(0).and_then(|value| value.parse::<i16>().ok()) {
            Some(value) => {
                self.gain = value.clamp(-24, 24);
                self.respond(format!("gain: {} dB", signed(self.gain)));
            }
            None => self.respond("gain: use gain <-24..24>"),
        }
    }

    fn on_pitch(&mut self, event: &control::ParsedCommand) {
        match event.arg(0).and_then(|value| value.parse::<i16>().ok()) {
            Some(value) => {
                self.pitch = value.clamp(-12, 12);
                self.respond(format!("pitch: {} semitones", signed(self.pitch)));
            }
            None => self.respond("pitch: use pitch <-12..12>"),
        }
    }

    fn on_cut(&mut self, event: &control::ParsedCommand) {
        let start = event.arg(0).and_then(parse_time);
        let end = event.arg(1).and_then(parse_time);

        match (start, end) {
            (Some(start), Some(end)) if start < end => {
                self.respond(format!(
                    "cut: staged {}..{}; no file has been changed",
                    fmt_time(start),
                    fmt_time(end)
                ));
            }
            _ => self.respond("cut: use cut <start> <end>"),
        }
    }

    fn on_rec(&mut self, _event: &control::ParsedCommand) {
        self.recording = !self.recording;
        if self.recording {
            self.respond("record: armed visually; microphone is not connected");
        } else {
            self.respond("record: disarmed");
        }
    }

    fn on_mic(&mut self, _event: &control::ParsedCommand) {
        self.respond("mic: demo input meter only; no capture device opened");
    }

    fn on_save(&mut self, event: &control::ParsedCommand) {
        let path = event.rest();
        if path.is_empty() {
            self.respond("save: missing path");
        } else {
            self.respond(format!("save: staged export to {path}; no bytes written"));
        }
    }

    fn on_playlist(&mut self, _event: &control::ParsedCommand) {
        self.playlist_visible = true;
        self.scope_mode = None;
        self.respond(format!(
            "playlist: showing folder demo with {} entries",
            self.playlist_entries.len()
        ));
    }

    fn on_playlist_top(&mut self, _event: &control::ParsedCommand) {
        self.playlist_scroll = 0;
        self.respond("playlist: top");
    }

    fn on_playlist_bottom(&mut self, _event: &control::ParsedCommand) {
        self.playlist_scroll = self.playlist_entries.len().saturating_sub(1);
        self.respond("playlist: bottom");
    }

    fn on_playlist_up(&mut self, _event: &control::ParsedCommand) {
        self.scroll_playlist(-1);
        self.respond(format!("playlist: row {}", self.playlist_scroll + 1));
    }

    fn on_playlist_down(&mut self, _event: &control::ParsedCommand) {
        self.scroll_playlist(1);
        self.respond(format!("playlist: row {}", self.playlist_scroll + 1));
    }

    fn on_webradio(&mut self, _event: &control::ParsedCommand) {
        self.respond("webradio: stream list is not wired yet");
    }

    fn on_scope(&mut self, _event: &control::ParsedCommand) {
        self.scope_mode = Some(ScopeMode::Dual);
        self.playlist_visible = false;
        self.respond("scope: showing scope-tui oscillo/vector demo");
    }

    fn on_spectro(&mut self, _event: &control::ParsedCommand) {
        self.scope_mode = Some(ScopeMode::Spectroscope);
        self.playlist_visible = false;
        self.respond("scope: showing scope-tui spectroscope demo");
    }

    fn on_transcribe(&mut self, _event: &control::ParsedCommand) {
        self.respond("transcribe: would listen and append words to log/file");
    }

    fn on_help(&mut self, _event: &control::ParsedCommand) {
        self.respond(format!("commands: {}", control::command_names()));
    }
}

#[derive(Debug, Clone)]
struct Track {
    file: String,
    album: String,
    artist: String,
    codec: String,
    bitrate: String,
    sample_rate: String,
    channels: String,
    size: String,
}

impl From<TrackData> for Track {
    fn from(track: TrackData) -> Self {
        Self {
            file: track.file,
            album: track.album,
            artist: track.artist,
            codec: track.codec,
            bitrate: track.bitrate,
            sample_rate: track.sample_rate,
            channels: track.channels,
            size: track.size,
        }
    }
}

#[derive(Debug, Clone)]
struct Marker {
    name: String,
    seconds: u64,
}

impl Marker {
    fn new(name: impl Into<String>, seconds: u64) -> Self {
        Self {
            name: name.into(),
            seconds,
        }
    }
}

fn default_labels(duration_secs: u64) -> Vec<Marker> {
    vec![Marker::new("S", 0), Marker::new("E", duration_secs)]
}

struct BarCell {
    text: String,
    style: Style,
}

impl BarCell {
    fn new(ch: char, color: Color) -> Self {
        Self {
            text: ch.to_string(),
            style: Style::default().fg(color),
        }
    }

    fn set_text(&mut self, ch: char, bg: Color) {
        self.text = ch.to_string();
        self.style = Style::reset().fg(BAR_TIME_FG).bg(bg);
    }
}

fn place_bar_text(cells: &mut [BarCell], start: usize, text: &str, bg: Color) {
    for (idx, ch) in text.chars().enumerate() {
        if let Some(cell) = cells.get_mut(start + idx) {
            cell.set_text(ch, bg);
        }
    }
}

fn compact_response(message: &str) -> String {
    let summary = message
        .split_once(':')
        .map(|(head, _)| head)
        .unwrap_or(message)
        .trim();
    let words = summary.split_whitespace().take(5).collect::<Vec<_>>();

    if words.is_empty() {
        "ready".into()
    } else {
        words.join(" ")
    }
}

fn wrapped_track_value_lines(value: &str, width: u16) -> Vec<Line<'static>> {
    const INDENT: usize = 2;
    const MAX_LINES: usize = 3;

    let text_width = usize::from(width).saturating_sub(INDENT).max(1);
    let mut chunks = wrap_text(value, text_width);
    let overflow = chunks.len() > MAX_LINES;
    chunks.truncate(MAX_LINES);

    if chunks.is_empty() {
        chunks.push(String::new());
    }
    if overflow {
        if let Some(last) = chunks.last_mut() {
            *last = ellipsize(last, text_width);
        }
    }

    chunks
        .into_iter()
        .map(|line| {
            Line::from(Span::styled(
                format!("  {line}"),
                plain_style().fg(Color::Gray).add_modifier(Modifier::ITALIC),
            ))
        })
        .collect()
}

fn wrap_text(text: &str, width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let mut current = String::new();

    for word in text.split_whitespace() {
        let word_len = word.chars().count();
        if word_len > width {
            if !current.is_empty() {
                lines.push(current);
                current = String::new();
            }
            lines.extend(chunk_word(word, width));
            continue;
        }

        let next_len = if current.is_empty() {
            word_len
        } else {
            current.chars().count() + 1 + word_len
        };

        if next_len <= width {
            if !current.is_empty() {
                current.push(' ');
            }
            current.push_str(word);
        } else {
            lines.push(current);
            current = word.to_string();
        }
    }

    if !current.is_empty() {
        lines.push(current);
    }

    lines
}

fn chunk_word(word: &str, width: usize) -> Vec<String> {
    let chars = word.chars().collect::<Vec<_>>();
    chars
        .chunks(width.max(1))
        .map(|chunk| chunk.iter().collect())
        .collect()
}

fn ellipsize(text: &str, width: usize) -> String {
    if width <= 3 {
        return ".".repeat(width);
    }

    let prefix = text.chars().take(width - 3).collect::<String>();
    format!("{prefix}...")
}

#[derive(Debug, Clone)]
struct PlaylistEntry {
    icon: String,
    name: String,
    kind: String,
    duration: String,
    size: String,
}

impl From<PlaylistEntryData> for PlaylistEntry {
    fn from(entry: PlaylistEntryData) -> Self {
        Self {
            icon: entry.icon,
            name: entry.name,
            kind: entry.kind,
            duration: entry.duration,
            size: entry.size,
        }
    }
}

fn icon_span(text: &'static str) -> Span<'static> {
    Span::styled(text, plain_style())
}

fn justified_line(
    mut left: Vec<Span<'static>>,
    right: Vec<Span<'static>>,
    width: u16,
) -> Line<'static> {
    let left_width = Line::from(left.clone()).width();
    let right_width = Line::from(right.clone()).width();
    let gap = usize::from(width).saturating_sub(left_width + right_width);

    left.push(Span::raw(" ".repeat(gap)));
    left.extend(right);
    Line::from(left)
}

fn block_title_style() -> Style {
    Style::default().fg(COMMAND_FG).add_modifier(Modifier::BOLD)
}

fn rect_contains(area: Rect, column: u16, row: u16) -> bool {
    column >= area.x
        && column < area.x.saturating_add(area.width)
        && row >= area.y
        && row < area.y.saturating_add(area.height)
}

fn inset_x(area: Rect, margin: u16) -> Rect {
    let margin = margin.min(area.width / 2);
    Rect {
        x: area.x + margin,
        width: area.width.saturating_sub(margin * 2),
        ..area
    }
}

fn extend_right(area: Rect, amount: u16) -> Rect {
    Rect {
        width: area.width.saturating_add(amount),
        ..area
    }
}

fn extend_left(area: Rect, amount: u16) -> Rect {
    let amount = amount.min(area.x);
    Rect {
        x: area.x.saturating_sub(amount),
        width: area.width.saturating_add(amount),
        ..area
    }
}

fn transport_titles(symbol: &str, rail_len: usize, phase: usize) -> (String, String) {
    if rail_len < 3 {
        return (symbol.to_string(), symbol.to_string());
    }

    let max_pos = rail_len.saturating_sub(2);
    let top_pos = 1 + pingpong_phase(phase, max_pos);
    let bottom_pos = rail_len.saturating_sub(top_pos + 2);

    (
        transport_title(symbol, rail_len, top_pos),
        transport_title(symbol, rail_len, bottom_pos),
    )
}

fn pingpong_phase(phase: usize, len: usize) -> usize {
    if len <= 1 {
        return 0;
    }

    let cycle = len * 2 - 2;
    let step = phase % cycle;
    if step < len { step } else { cycle - step }
}

fn transport_title(symbol: &str, rail_len: usize, pos: usize) -> String {
    let mut cells = vec!["─".to_string(); rail_len];
    if let Some(cell) = cells.get_mut(pos) {
        *cell = symbol.to_string();
    }
    if let Some(cell) = cells.get_mut(pos + 1) {
        *cell = " ".to_string();
    }

    cells.concat()
}

fn plain_style() -> Style {
    Style::default().remove_modifier(Modifier::BOLD | Modifier::UNDERLINED | Modifier::REVERSED)
}

fn accent_span(text: &'static str) -> Span<'static> {
    Span::styled(
        text,
        Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
    )
}

fn accent_span_owned(text: String) -> Span<'static> {
    Span::styled(
        text,
        Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
    )
}

fn parse_time(input: &str) -> Option<u64> {
    let parts = input.split(':').collect::<Vec<_>>();
    match parts.as_slice() {
        [seconds] => seconds.parse().ok(),
        [minutes, seconds] => {
            let minutes = minutes.parse::<u64>().ok()?;
            let seconds = seconds.parse::<u64>().ok()?;
            Some(minutes * 60 + seconds)
        }
        [hours, minutes, seconds] => {
            let hours = hours.parse::<u64>().ok()?;
            let minutes = minutes.parse::<u64>().ok()?;
            let seconds = seconds.parse::<u64>().ok()?;
            Some(hours * 3600 + minutes * 60 + seconds)
        }
        _ => None,
    }
}

fn marker_position(seconds: u64, duration: u64, width: usize) -> usize {
    ((seconds as f64 / duration.max(1) as f64) * width.saturating_sub(1) as f64).round() as usize
}

fn place_centered(row: &mut [char], center: usize, text: &str) {
    let chars = text.chars().collect::<Vec<_>>();
    let start = center.saturating_sub(chars.len() / 2);

    for (offset, ch) in chars.into_iter().enumerate() {
        let index = start + offset;
        if index < row.len() {
            row[index] = ch;
        }
    }
}

fn fmt_time(seconds: u64) -> String {
    let minutes = seconds / 60;
    let seconds = seconds % 60;
    format!("{minutes:02}:{seconds:02}")
}

fn signed(value: i16) -> String {
    if value > 0 {
        format!("+{value}")
    } else {
        value.to_string()
    }
}
