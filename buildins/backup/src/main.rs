use crossterm::{
    cursor::{Hide, MoveTo, Show},
    event::{self, Event, KeyCode, KeyEventKind},
    execute, queue,
    style::{Color, Print, ResetColor, SetForegroundColor},
    terminal::{self, Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen},
};
use std::{
    io::{self, Write},
    time::Duration,
};
use trueos::{env, platform, runtime, task, vshell};
mod model;
use model::{App, Screen};
const PINK: Color = Color::Rgb {
    r: 255,
    g: 55,
    b: 255,
};

fn main() {
    let app = App::new(env::args().skip(1));
    let Ok(runtime) = runtime::current_thread().build() else {
        let _ = vshell::shutdown_current_blueprint("backup:cancel");
        return;
    };
    let Ok(lease) = vshell::terminal_initial_lease() else {
        let _ = vshell::shutdown_current_blueprint("backup:cancel");
        return;
    };
    let session = runtime.block_on(async move {
        task::spawn(async move {
            let result = run(&lease, app).await;
            (lease, result)
        })
        .await
    });
    drop(runtime);
    if let Ok((lease, result)) = session {
        let reason = result.unwrap_or_else(|_| "backup:cancel".into());
        let _ = vshell::report_exit_reason(&reason);
        let _ = lease.release_to_shell();
        let _ = vshell::shutdown_current_blueprint(&reason);
    } else {
        let _ = vshell::shutdown_current_blueprint("backup:cancel");
    }
}
struct TerminalGuard;
impl TerminalGuard {
    fn enter() -> io::Result<Self> {
        terminal::enable_raw_mode()?;
        if let Err(e) = execute!(io::stdout(), EnterAlternateScreen, Hide) {
            let _ = terminal::disable_raw_mode();
            return Err(e);
        }
        Ok(Self)
    }
}
impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = execute!(io::stdout(), ResetColor, Show, LeaveAlternateScreen);
        let _ = terminal::disable_raw_mode();
    }
}
async fn run(lease: &vshell::TerminalLease, mut app: App) -> io::Result<String> {
    let _terminal = TerminalGuard::enter()?;
    draw(&app)?;
    lease
        .acknowledge_ready()
        .map_err(|e| io::Error::other(e.to_string()))?;
    loop {
        if event::poll(Duration::ZERO)? {
            match event::read()? {
                Event::Resize(_, _) => draw(&app)?,
                Event::Key(key)
                    if matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat) =>
                {
                    match key.code {
                        KeyCode::Up | KeyCode::Char('k') => app.move_selection(-1),
                        KeyCode::Down | KeyCode::Char('j') => app.move_selection(1),
                        KeyCode::Enter => {
                            if let Some(action) = app.activate() {
                                return Ok(action);
                            }
                        }
                        KeyCode::Left | KeyCode::Char('h') => {
                            if app.back() {
                                return Ok("backup:quit".into());
                            }
                        }
                        KeyCode::Esc | KeyCode::Char('q' | 'Q') => return Ok("backup:quit".into()),
                        _ => {}
                    }
                    draw(&app)?;
                }
                _ => {}
            }
        } else {
            platform::poll_once();
            task::yield_now().await;
        }
    }
}
fn bytes(n: u64) -> String {
    format!("{:.2} GiB", n as f64 / (1024.0 * 1024.0 * 1024.0))
}
fn disk_label(app: &App, index: usize) -> String {
    let d = &app.disks[index];
    format!("disc{:03} · {} · {}", d.id, bytes(d.bytes), d.label)
}
fn draw(app: &App) -> io::Result<()> {
    let mut out = io::stdout();
    queue!(
        out,
        Clear(ClearType::All),
        MoveTo(0, 0),
        SetForegroundColor(PINK),
        Print("BACKUP"),
        ResetColor,
        Print("  whole-disk backup & restore\r\n\r\n")
    )?;
    let mut rows = Vec::new();
    let description = match app.screen {
        Screen::Home => {
            rows.extend([
                "Back up a disk".into(),
                "Restore a local backup".into(),
                "Return".into(),
            ]);
            "Choose an operation.".into()
        }
        Screen::Source => {
            rows.extend((0..app.disks.len()).map(|i| disk_label(app, i)));
            "Choose the source disk. Its normal use will pause during backup.".into()
        }
        Screen::Method(source) => {
            rows.push("Network · encrypted, resumable, one client".into());
            if !app.destinations(source).is_empty() {
                rows.push("Another disk · save to its /backups folder".into());
            }
            format!(
                "Source: {}\r\n\r\nWait up to 5 seconds for completed I/O; busy disks are left alone.\r\nNormal disk access returns when the operation ends.{}",
                disk_label(app, source),
                if app.destinations(source).is_empty() {
                    "\r\nLocal backup needs another TRUEOSFS disk with sufficient free space."
                } else {
                    ""
                }
            )
        }
        Screen::Destination(source) => {
            rows.extend(app.destinations(source).iter().map(|&i| {
                format!(
                    "{} · {} free for backup",
                    disk_label(app, i),
                    bytes(app.disks[i].free.unwrap_or(0))
                )
            }));
            format!(
                "Source: {}\r\nChoose another mounted TRUEOSFS disk. Save into /backups.",
                disk_label(app, source)
            )
        }
        Screen::Images => {
            rows.extend(
                app.images
                    .iter()
                    .map(|i| format!("disc{:03} · {} · {}", i.root, bytes(i.bytes), i.label)),
            );
            "Choose a completed backup from a mounted TRUEOSFS /backups folder.".into()
        }
        Screen::Target(image) => {
            rows.extend(app.targets(image).iter().map(|&i| disk_label(app, i)));
            format!(
                "Image: {}\r\nChoose a different disk with the same size and block geometry.\r\nIts entire contents will be replaced.",
                app.images[image].label
            )
        }
        Screen::Confirm { image, disk } => {
            rows.push("Cancel · keep the target unchanged".into());
            rows.push("ERASE TARGET AND RESTORE THIS IMAGE".into());
            format!(
                "RESTORE · ERASE CONFIRMATION\r\n\r\nImage: {} on disc{:03}\r\nTarget: {}\r\n\r\nEvery sector on the target will be overwritten.\r\nThe saved image is verified before writing.\r\nStopping after writing starts leaves an incomplete target.",
                app.images[image].label,
                app.images[image].root,
                disk_label(app, disk)
            )
        }
    };
    queue!(out, Print(description.as_str()), Print("\r\n\r\n"))?;
    if rows.is_empty() {
        queue!(
            out,
            SetForegroundColor(Color::DarkGrey),
            Print("  No eligible disks or completed backups available.\r\n"),
            ResetColor
        )?;
    }
    let (width, height) = terminal::size().unwrap_or((100, 30));
    let page = (height as usize)
        .saturating_sub(description.lines().count() + 8)
        .max(1);
    let start = app.selected / page * page;
    for (i, label) in rows.iter().enumerate().skip(start).take(page) {
        let color = if i == app.selected {
            PINK
        } else {
            Color::White
        };
        let label: String = label
            .chars()
            .take(width.saturating_sub(5) as usize)
            .collect();
        queue!(
            out,
            SetForegroundColor(color),
            Print(if i == app.selected { "  › " } else { "    " }),
            Print(label),
            ResetColor,
            Print("\r\n")
        )?;
    }
    queue!(
        out,
        Print("\r\n"),
        SetForegroundColor(Color::DarkGrey),
        Print("↑/↓ or j/k select   Enter choose   ←/h back   Esc/q quit"),
        ResetColor
    )?;
    out.flush()
}
