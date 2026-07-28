use std::{
    io,
    time::{Duration, Instant},
};

use anyhow::Result;
use crossterm::{
    cursor::{SetCursorStyle, Show},
    event::{self, Event, KeyCode, KeyEvent, KeyModifiers},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{
    Frame, Terminal,
    backend::CrosstermBackend,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Cell, Gauge, Paragraph, Row, Table, Wrap, block::Title},
};

use crate::weather::WeatherSnapshot;

const BG: Color = Color::Rgb(9, 13, 18);
const PANEL: Color = Color::Rgb(18, 24, 31);
const PANEL_2: Color = Color::Rgb(24, 31, 40);
const TEXT: Color = Color::Rgb(232, 239, 246);
const DIM: Color = Color::Rgb(137, 151, 165);
const ACCENT: Color = Color::Rgb(114, 210, 174);
const BLUE: Color = Color::Rgb(111, 169, 242);
const WARN: Color = Color::Rgb(255, 193, 109);
const REFRESH_SECS: u64 = 600;

pub trait WeatherVisual {
    fn publish_snapshot(&mut self, snapshot: &WeatherSnapshot);

    fn poll(&mut self) {}
}

pub fn run<F, V>(initial: WeatherSnapshot, mut refresh: F, visual: &mut V) -> Result<()>
where
    F: FnMut(&mut String) -> Result<WeatherSnapshot>,
    V: WeatherVisual,
{
    let app = App::new(initial);
    // Establish the UI4 window while the Blueprint still owns its normal
    // startup context. The terminal alternate-screen handoff is independent
    // and should not gate the visual window.
    visual.publish_snapshot(&app.snapshot);
    let mut terminal = setup_terminal()?;
    let result = app.run(&mut terminal, &mut refresh, visual);
    restore_terminal(&mut terminal)?;
    result
}

fn setup_terminal() -> Result<Terminal<CrosstermBackend<io::Stdout>>> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(
        stdout,
        EnterAlternateScreen,
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
        SetCursorStyle::DefaultUserShape,
        Show,
        LeaveAlternateScreen
    )?;
    terminal.show_cursor()?;
    Ok(())
}

#[derive(Debug, Clone)]
struct App {
    snapshot: WeatherSnapshot,
    selected: usize,
    last_refresh: Instant,
    refresh_pending: bool,
    status: String,
    should_quit: bool,
}

impl App {
    fn new(snapshot: WeatherSnapshot) -> Self {
        Self {
            snapshot,
            selected: 0,
            last_refresh: Instant::now(),
            refresh_pending: true,
            status: String::from("loading live weather  r refresh  q quit"),
            should_quit: false,
        }
    }

    fn run<F, V>(
        mut self,
        terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
        refresh: &mut F,
        visual: &mut V,
    ) -> Result<()>
    where
        F: FnMut(&mut String) -> Result<WeatherSnapshot>,
        V: WeatherVisual,
    {
        while !self.should_quit {
            terminal.draw(|frame| self.draw(frame))?;
            visual.poll();

            if self.refresh_pending
                || self.last_refresh.elapsed() >= Duration::from_secs(REFRESH_SECS)
            {
                self.refresh_pending = false;
                self.refresh(refresh, visual);
            }

            match event::poll(Duration::from_millis(120)) {
                Ok(true) => match event::read() {
                    Ok(Event::Key(key)) => self.handle_key(key, refresh, visual),
                    Ok(_) => {}
                    Err(err) => {
                        self.status = format!("input read unavailable: {err}");
                        std::thread::sleep(Duration::from_millis(250));
                    }
                },
                Ok(false) => {}
                Err(err) => {
                    // Input is ancillary to the weather refresh and UI4
                    // snapshot. In particular, an unavailable SIGWINCH/event
                    // source must not unwind Frog, drop the immutable frame,
                    // and make a healthy live-weather update look like a
                    // graphics crash.
                    self.status = format!("input polling unavailable: {err}");
                    std::thread::sleep(Duration::from_millis(250));
                }
            }
        }
        Ok(())
    }

    fn handle_key<F, V>(&mut self, key: KeyEvent, refresh: &mut F, visual: &mut V)
    where
        F: FnMut(&mut String) -> Result<WeatherSnapshot>,
        V: WeatherVisual,
    {
        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => self.should_quit = true,
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.should_quit = true;
            }
            KeyCode::Char('r') => self.refresh(refresh, visual),
            KeyCode::Down | KeyCode::Char('j') => self.select_next(),
            KeyCode::Up | KeyCode::Char('k') => self.select_prev(),
            KeyCode::Home => self.selected = 0,
            KeyCode::End => {
                self.selected = self.snapshot.days.len().saturating_sub(1);
            }
            _ => {}
        }
    }

    fn refresh<F, V>(&mut self, refresh: &mut F, visual: &mut V)
    where
        F: FnMut(&mut String) -> Result<WeatherSnapshot>,
        V: WeatherVisual,
    {
        match refresh(&mut self.status) {
            Ok(snapshot) => {
                self.snapshot = snapshot;
                self.selected = self
                    .selected
                    .min(self.snapshot.days.len().saturating_sub(1));
                self.status = String::from("fresh weather ready");
                self.last_refresh = Instant::now();
                visual.publish_snapshot(&self.snapshot);
            }
            Err(err) => {
                self.status = format!("refresh failed: {err}");
            }
        }
    }

    fn select_next(&mut self) {
        if !self.snapshot.days.is_empty() {
            self.selected = (self.selected + 1).min(self.snapshot.days.len() - 1);
        }
    }

    fn select_prev(&mut self) {
        self.selected = self.selected.saturating_sub(1);
    }

    fn draw(&self, frame: &mut Frame<'_>) {
        let area = frame.area();
        frame.render_widget(Block::default().style(Style::default().bg(BG)), area);

        let root = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(6),
                Constraint::Min(14),
                Constraint::Length(3),
            ])
            .split(area);

        self.draw_header(frame, root[0]);

        let body = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(62), Constraint::Percentage(38)])
            .split(root[1]);
        self.draw_forecast_table(frame, body[0]);
        self.draw_day_details(frame, body[1]);
        self.draw_footer(frame, root[2]);
    }

    fn draw_header(&self, frame: &mut Frame<'_>, area: Rect) {
        let current = self
            .snapshot
            .current
            .as_ref()
            .map(|current| {
                format!(
                    "{} {}C feels {}C  {}  humidity {}%  wind {} km/h",
                    current.icon.glyph(),
                    current.temp_c,
                    current.feels_c,
                    current.summary,
                    current.humidity,
                    current.wind_kmh
                )
            })
            .unwrap_or_else(|| String::from("current weather unavailable"));

        let lines = vec![
            Line::from(vec![
                Span::styled(
                    "Frog",
                    Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
                ),
                Span::raw("  "),
                Span::styled(
                    format!(
                        "{} {}  {:.4}, {:.4}",
                        self.snapshot.location.country,
                        self.snapshot.location.name,
                        self.snapshot.location.lat,
                        self.snapshot.location.lon
                    ),
                    Style::default().fg(TEXT),
                ),
            ]),
            Line::styled(current, Style::default().fg(TEXT)),
            Line::styled(self.snapshot.source.as_str(), Style::default().fg(DIM)),
        ];

        frame.render_widget(
            Paragraph::new(lines)
                .block(panel_block("weather"))
                .style(Style::default().bg(PANEL))
                .wrap(Wrap { trim: true }),
            area,
        );
    }

    fn draw_forecast_table(&self, frame: &mut Frame<'_>, area: Rect) {
        let rows = self.snapshot.days.iter().enumerate().map(|(idx, day)| {
            let style = if idx == self.selected {
                Style::default().fg(Color::Black).bg(ACCENT)
            } else if idx % 2 == 0 {
                Style::default().fg(TEXT).bg(PANEL_2)
            } else {
                Style::default().fg(TEXT).bg(PANEL)
            };
            Row::new(vec![
                Cell::from(format!("{} {}", day.icon.glyph(), day.weekday)),
                Cell::from(day.summary.clone()),
                Cell::from(format!("{:>2}/{:>2}C", day.temp_min_c, day.temp_max_c)),
                Cell::from(format!("{:>3}%", day.rain_percent)),
                Cell::from(format!("{} {:>2}", day.wind_dir, day.wind_kmh)),
            ])
            .style(style)
            .height(2)
        });

        let table = Table::new(
            rows,
            [
                Constraint::Length(8),
                Constraint::Min(18),
                Constraint::Length(8),
                Constraint::Length(6),
                Constraint::Length(8),
            ],
        )
        .header(
            Row::new(["day", "summary", "range", "rain", "wind"]).style(
                Style::default()
                    .fg(DIM)
                    .bg(PANEL)
                    .add_modifier(Modifier::BOLD),
            ),
        )
        .block(panel_block("8 day forecast"))
        .column_spacing(1)
        .style(Style::default().bg(PANEL));

        frame.render_widget(table, area);
    }

    fn draw_day_details(&self, frame: &mut Frame<'_>, area: Rect) {
        let Some(day) = self.snapshot.days.get(self.selected) else {
            frame.render_widget(
                Paragraph::new("no daily forecast data").block(panel_block("details")),
                area,
            );
            return;
        };

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(7),
                Constraint::Length(4),
                Constraint::Min(5),
            ])
            .split(area);

        let title = format!("{} {}", day.icon.glyph(), day.weekday);
        let detail_lines = vec![
            Line::styled(
                title,
                Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
            ),
            Line::styled(day.summary.as_str(), Style::default().fg(TEXT)),
            Line::from(format!(
                "day {}C  feels {}C  night {}C",
                day.temp_day_c, day.feels_day_c, day.temp_night_c
            )),
            Line::from(format!(
                "humidity {}%  wind {} {} km/h  uvi {}",
                day.humidity, day.wind_dir, day.wind_kmh, day.uvi
            )),
        ];
        frame.render_widget(
            Paragraph::new(detail_lines)
                .block(panel_block("selected"))
                .style(Style::default().fg(TEXT).bg(PANEL))
                .wrap(Wrap { trim: true }),
            chunks[0],
        );

        let rain = day.rain_percent.clamp(0, 100) as u16;
        frame.render_widget(
            Gauge::default()
                .block(panel_block("precipitation"))
                .gauge_style(Style::default().fg(BLUE).bg(PANEL_2))
                .percent(rain),
            chunks[1],
        );

        let note = if self.snapshot.note.is_empty() {
            self.snapshot.updated_line.as_str()
        } else {
            self.snapshot.note.as_str()
        };
        let note_style = if self.snapshot.note.is_empty() {
            Style::default().fg(DIM).bg(PANEL)
        } else {
            Style::default().fg(WARN).bg(PANEL)
        };
        frame.render_widget(
            Paragraph::new(note)
                .block(panel_block("transport"))
                .style(note_style)
                .alignment(Alignment::Left)
                .wrap(Wrap { trim: true }),
            chunks[2],
        );
    }

    fn draw_footer(&self, frame: &mut Frame<'_>, area: Rect) {
        let elapsed = self.last_refresh.elapsed().as_secs();
        let next = REFRESH_SECS.saturating_sub(elapsed);
        let footer = Line::from(vec![
            Span::styled(self.status.as_str(), Style::default().fg(TEXT)),
            Span::raw("  "),
            Span::styled(
                format!("next auto refresh {}s", next),
                Style::default().fg(DIM),
            ),
        ]);
        frame.render_widget(
            Paragraph::new(footer)
                .block(
                    Block::default()
                        .borders(Borders::TOP)
                        .border_style(Style::default().fg(PANEL_2)),
                )
                .style(Style::default().fg(TEXT).bg(BG)),
            area,
        );
    }
}

fn panel_block(title: &'static str) -> Block<'static> {
    Block::default()
        .title(Title::from(title))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(PANEL_2))
}
