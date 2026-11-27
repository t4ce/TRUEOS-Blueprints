use std::{
    fmt::Display,
    io,
    ops::Range,
    pin::Pin,
    task::{Context, Poll, Waker},
};

use crossterm::{
    cursor::{Hide, MoveTo, Show},
    event::{Event, EventStream, MouseButton, MouseEventKind},
    queue,
    style::{Attribute, Print, SetAttribute},
    terminal::{self, Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen},
};
use futures_core::Stream;
use trueos::vshell::{self, TerminalLease};

use super::sky_light::{DEFAULT_KELVIN, Key, Levels, MAX_KELVIN, MIN_KELVIN};

const LABELS: [&str; 2] = ["sky   ", "sheen "];
const FIRST_COLUMN: u16 = LABELS[0].len() as u16;
const LEVEL_COUNT: u16 = 10;
const KEY_LABEL: &str = "key  ";
const KEY_ROW: u16 = LABELS.len() as u16;
const KELVIN_ROW: u16 = KEY_ROW + 1;
const KELVIN_VALUE_WIDTH: u16 = 5;
const MAX_TRACK_WIDTH: u16 = 16;
const EVENTS_PER_FRAME: usize = 8;

pub(super) struct SkyControls {
    events: EventStream,
    lease: Option<TerminalLease>,
    levels: Levels,
    kelvin: u16,
}

impl SkyControls {
    pub(super) fn open() -> io::Result<Self> {
        let lease = vshell::terminal_initial_lease()
            .map_err(|error| io::Error::other(error.to_string()))?;
        if let Err(error) = terminal::enable_raw_mode() {
            let _ = lease.release_to_shell();
            return Err(error);
        }
        let controls = Self {
            events: EventStream::new(),
            lease: Some(lease),
            levels: Levels::default(),
            kelvin: DEFAULT_KELVIN,
        };
        controls.draw(true)?;
        Ok(controls)
    }

    pub(super) fn poll(&mut self) -> io::Result<Option<Levels>> {
        let before = self.levels;
        let mut redraw = false;
        let mut reentered = false;
        let mut context = Context::from_waker(Waker::noop());
        for _ in 0..EVENTS_PER_FRAME {
            let Poll::Ready(Some(event)) = Pin::new(&mut self.events).poll_next(&mut context)
            else {
                break;
            };
            match event? {
                Event::Mouse(mouse) if mouse.kind == MouseEventKind::Down(MouseButton::Left) => {
                    self.select(mouse.row, mouse.column);
                }
                Event::Resize(_, _) => {
                    // Shell3 resets the screen when the app takes back its lease.
                    if let Ok(lease) = vshell::terminal_initial_lease()
                        && self.lease.as_ref() != Some(&lease)
                    {
                        self.lease = Some(lease);
                        reentered = true;
                    }
                    redraw = true;
                }
                _ => {}
            }
        }
        let changed = self.levels != before;
        if changed || redraw {
            self.draw(reentered)?;
        }
        Ok(changed.then_some(self.levels))
    }

    fn select(&mut self, row: u16, column: u16) {
        if row == KEY_ROW {
            let choice = terminal::size().ok().and_then(|(columns, _)| {
                self.key_choices()
                    .find(|(cells, _, _)| cells.end <= columns && cells.contains(&column))
            });
            if let Some((_, _, key)) = choice {
                self.levels.key = key;
            }
        } else if row == KELVIN_ROW {
            if matches!(self.levels.key, Key::Kelvin(_))
                && let Ok((columns, _)) = terminal::size()
                && let Some(kelvin) = kelvin_at(column, kelvin_track(columns))
            {
                self.kelvin = kelvin;
                self.levels.key = Key::Kelvin(kelvin);
            }
        } else if let Some(level) = level_at(column) {
            match row {
                0 => self.levels.sky = level,
                1 => self.levels.sheen = level,
                _ => {}
            }
        }
    }

    fn key_choices(&self) -> impl Iterator<Item = (Range<u16>, &'static str, Key)> {
        let mut start = KEY_LABEL.len() as u16;
        [
            ("white", Key::Neutral),
            ("sunrise", Key::Sunrise),
            ("kelvin", Key::Kelvin(self.kelvin)),
        ]
        .into_iter()
        .map(move |(label, key)| {
            let cells = start..start + label.len() as u16;
            start = cells.end + 1;
            (cells, label, key)
        })
    }

    fn draw(&self, setup: bool) -> io::Result<()> {
        let mut output = Vec::new();
        if setup {
            // Request button reports using SGR coordinates so motion does not fill the input queue.
            queue!(
                output,
                EnterAlternateScreen,
                Hide,
                Print("\x1b[?1006h\x1b[?1000h")
            )?;
        }
        let (columns, rows) = match terminal::size() {
            Ok(size) => size,
            Err(error) if !setup && error.kind() == io::ErrorKind::NotConnected => return Ok(()),
            Err(error) => return Err(error),
        };
        let digits = columns
            .saturating_sub(FIRST_COLUMN)
            .div_ceil(2)
            .min(LEVEL_COUNT);
        for (row, (label, selected)) in LABELS
            .into_iter()
            .zip([self.levels.sky, self.levels.sheen])
            .take(usize::from(rows))
            .enumerate()
        {
            draw_label(&mut output, row as u16, columns, label)?;
            for digit in 0..digits as u8 {
                if digit != 0 {
                    queue!(output, Print(' '))?;
                }
                draw_choice(&mut output, char::from(b'0' + digit), digit == selected)?;
            }
        }
        if rows > KEY_ROW {
            draw_label(&mut output, KEY_ROW, columns, KEY_LABEL)?;
            for (cells, label, key) in self.key_choices() {
                if cells.end <= columns {
                    queue!(output, MoveTo(cells.start, KEY_ROW))?;
                    draw_choice(&mut output, label, self.levels.key == key)?;
                }
            }
        }
        if rows > KELVIN_ROW {
            draw_label(&mut output, KELVIN_ROW, columns, "")?;
            if let Key::Kelvin(kelvin) = self.levels.key {
                draw_kelvin_track(&mut output, columns, kelvin)?;
            }
        }
        write_terminal(&output)?;
        if setup && let Some(lease) = &self.lease {
            lease
                .acknowledge_ready()
                .map_err(|error| io::Error::other(error.to_string()))?;
        }
        Ok(())
    }
}

impl Drop for SkyControls {
    fn drop(&mut self) {
        if let Some(lease) = self.lease.take()
            && lease.acknowledge_ready().is_ok()
        {
            let mut output = Vec::new();
            let _ = queue!(
                output,
                Print("\x1b[?1000l\x1b[?1006l"),
                LeaveAlternateScreen,
                Show
            );
            let _ = write_terminal(&output);
            let _ = lease.release_to_shell();
        }
        let _ = terminal::disable_raw_mode();
    }
}

fn draw_label(output: &mut Vec<u8>, row: u16, columns: u16, label: &str) -> io::Result<()> {
    let label = &label[..label.len().min(usize::from(columns))];
    queue!(
        output,
        MoveTo(0, row),
        Clear(ClearType::CurrentLine),
        Print(label)
    )
}

fn draw_choice(output: &mut Vec<u8>, choice: impl Display, selected: bool) -> io::Result<()> {
    if selected {
        queue!(
            output,
            SetAttribute(Attribute::Reverse),
            Print(choice),
            SetAttribute(Attribute::NoReverse)
        )
    } else {
        queue!(output, Print(choice))
    }
}

fn level_at(column: u16) -> Option<u8> {
    let offset = column.checked_sub(FIRST_COLUMN)?;
    let level = offset / 2;
    (offset.is_multiple_of(2) && level < LEVEL_COUNT).then_some(level as u8)
}

fn kelvin_track(columns: u16) -> Range<u16> {
    let start = KELVIN_VALUE_WIDTH + 1;
    let margin = 3 * KELVIN_VALUE_WIDTH + 3;
    let width = columns.saturating_sub(margin).min(MAX_TRACK_WIDTH);
    start..start + width
}

fn kelvin_at(column: u16, track: Range<u16>) -> Option<u16> {
    if track.len() < 2 || !track.contains(&column) {
        return None;
    }
    let offset = u32::from(column - track.start);
    let range = u32::from(MAX_KELVIN - MIN_KELVIN);
    let value = MAX_KELVIN - (offset * range / (track.len() as u32 - 1)) as u16;
    Some((value + 50) / 100 * 100)
}

fn draw_kelvin_track(output: &mut Vec<u8>, columns: u16, kelvin: u16) -> io::Result<()> {
    let track = kelvin_track(columns);
    if track.len() < 2 {
        if columns >= KELVIN_VALUE_WIDTH {
            queue!(output, Print(format_args!("{kelvin}K")))?;
        }
        return Ok(());
    }
    let span = track.len() as f32 - 1.0;
    let position = f32::from(MAX_KELVIN - kelvin) / f32::from(MAX_KELVIN - MIN_KELVIN);
    let selected = track.start + (position * span).round() as u16;
    queue!(output, Print(format_args!("{MAX_KELVIN}K ")))?;
    for column in track {
        let active = column == selected;
        draw_choice(output, if active { '|' } else { '-' }, active)?;
    }
    queue!(output, Print(format_args!(" {MIN_KELVIN}K {kelvin}K")))
}

fn write_terminal(output: &[u8]) -> io::Result<()> {
    if vshell::write(output) != output.len() {
        return Err(io::Error::new(
            io::ErrorKind::WriteZero,
            "sky controls terminal write",
        ));
    }
    Ok(())
}
