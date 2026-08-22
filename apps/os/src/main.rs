use std::env;
use std::io::{self, Write};

use crossterm::{
    cursor::{Hide, MoveTo, Show},
    event::{self, Event, KeyCode, KeyEvent, KeyEventKind},
    execute, queue,
    style::{Attribute, Color, Print, ResetColor, SetAttribute, SetForegroundColor},
    terminal::{self, Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen},
};
use trueos::vshell;

const PINK: Color = Color::Rgb {
    r: 255,
    g: 55,
    b: 255,
};

#[derive(Clone)]
struct Disk {
    id: u32,
    name: String,
    size: String,
    mode: String,
    status: String,
    label: String,
}

#[derive(Clone, Copy)]
enum InstallSource {
    Local,
    Online,
}

#[derive(Clone, Copy)]
enum Action {
    Install { disk: usize, source: InstallSource },
    LiveUpdate,
}

enum Screen {
    Home,
    Disks,
    Source { disk: usize },
    Confirm(Action),
}

struct App {
    disks: Vec<Disk>,
    screen: Screen,
    selected: usize,
}

impl App {
    fn new(disks: Vec<Disk>) -> Self {
        Self {
            disks,
            screen: Screen::Home,
            selected: 0,
        }
    }

    fn item_count(&self) -> usize {
        match self.screen {
            Screen::Home => 3,
            Screen::Disks => self.disks.len().max(1),
            Screen::Source { .. } => 2,
            Screen::Confirm(_) => 2,
        }
    }

    fn move_selection(&mut self, delta: isize) {
        let count = self.item_count();
        if count == 0 {
            self.selected = 0;
            return;
        }
        self.selected = if delta < 0 {
            self.selected.checked_sub(1).unwrap_or(count - 1)
        } else {
            (self.selected + 1) % count
        };
    }

    fn back(&mut self) -> bool {
        match self.screen {
            Screen::Home => true,
            Screen::Disks => {
                self.screen = Screen::Home;
                self.selected = 0;
                false
            }
            Screen::Source { .. } => {
                self.screen = Screen::Disks;
                self.selected = 0;
                false
            }
            Screen::Confirm(Action::Install { disk, .. }) => {
                self.screen = Screen::Source { disk };
                self.selected = 0;
                false
            }
            Screen::Confirm(Action::LiveUpdate) => {
                self.screen = Screen::Home;
                self.selected = 0;
                false
            }
        }
    }

    fn activate(&mut self) -> Option<String> {
        match self.screen {
            Screen::Home => match self.selected {
                0 => {
                    self.screen = Screen::Disks;
                    self.selected = 0;
                    None
                }
                1 => {
                    self.screen = Screen::Confirm(Action::LiveUpdate);
                    self.selected = 0;
                    None
                }
                _ => Some(String::from("os:quit")),
            },
            Screen::Disks => {
                if self.disks.is_empty() {
                    return None;
                }
                let disk = self.selected.min(self.disks.len() - 1);
                self.screen = Screen::Source { disk };
                self.selected = 0;
                None
            }
            Screen::Source { disk } => {
                let source = if self.selected == 0 {
                    InstallSource::Local
                } else {
                    InstallSource::Online
                };
                self.screen = Screen::Confirm(Action::Install { disk, source });
                self.selected = 0;
                None
            }
            Screen::Confirm(action) => {
                if self.selected == 0 {
                    match action {
                        Action::Install { disk, .. } => self.screen = Screen::Source { disk },
                        Action::LiveUpdate => self.screen = Screen::Home,
                    }
                    self.selected = 0;
                    return None;
                }
                Some(match action {
                    Action::LiveUpdate => String::from("os:update:live"),
                    Action::Install { disk, source } => {
                        let Some(disk) = self.disks.get(disk) else {
                            return Some(String::from("os:cancel"));
                        };
                        let source = match source {
                            InstallSource::Local => "local",
                            InstallSource::Online => "online",
                        };
                        format!("os:install:{source}:{}", disk.id)
                    }
                })
            }
        }
    }
}

fn main() {
    let disks = env::args().skip(1).filter_map(parse_disk).collect();
    let lease = match vshell::terminal_initial_lease() {
        Ok(lease) => lease,
        Err(_) => {
            let _ = vshell::shutdown_current_blueprint("os terminal lease unavailable");
            return;
        }
    };

    let result = run(&lease, disks);
    let reason = result.unwrap_or_else(|_| String::from("os:cancel"));
    let _ = vshell::report_exit_reason(reason.as_str());
    let _ = lease.release_to_shell();
    // Shutdown also records an exit reason. Reuse the action token so it
    // cannot overwrite the control-plane result during rapid teardown.
    let _ = vshell::shutdown_current_blueprint(reason.as_str());
}

fn parse_disk(arg: String) -> Option<Disk> {
    let fields = arg
        .strip_prefix("disk=")?
        .splitn(6, '|')
        .collect::<Vec<_>>();
    if fields.len() != 6 {
        return None;
    }
    Some(Disk {
        id: fields[0].parse().ok()?,
        name: String::from(fields[1]),
        size: String::from(fields[2]),
        mode: String::from(fields[3]),
        status: String::from(fields[4]),
        label: String::from(fields[5]),
    })
}

fn run(lease: &vshell::TerminalLease, disks: Vec<Disk>) -> io::Result<String> {
    let _terminal = TerminalGuard::enter()?;
    let mut app = App::new(disks);
    draw(&app)?;
    lease
        .acknowledge_ready()
        .map_err(|error| io::Error::new(io::ErrorKind::Other, error.to_string()))?;

    loop {
        match event::read()? {
            Event::Resize(_, _) => draw(&app)?,
            Event::Key(key) if matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat) => {
                if let Some(reason) = handle_key(&mut app, key) {
                    return Ok(reason);
                }
                draw(&app)?;
            }
            _ => {}
        }
    }
}

fn handle_key(app: &mut App, key: KeyEvent) -> Option<String> {
    match key.code {
        KeyCode::Up | KeyCode::Char('k') => app.move_selection(-1),
        KeyCode::Down | KeyCode::Char('j') => app.move_selection(1),
        KeyCode::Enter | KeyCode::Right | KeyCode::Char('l') => return app.activate(),
        KeyCode::Left | KeyCode::Char('h') => {
            if app.back() {
                return Some(String::from("os:quit"));
            }
        }
        KeyCode::Esc | KeyCode::Char('q' | 'Q') => return Some(String::from("os:quit")),
        _ => {}
    }
    None
}

struct TerminalGuard;

impl TerminalGuard {
    fn enter() -> io::Result<Self> {
        terminal::enable_raw_mode()?;
        let mut out = io::stdout();
        if let Err(error) = execute!(
            &mut out,
            EnterAlternateScreen,
            Hide,
            Clear(ClearType::All),
            MoveTo(0, 0)
        ) {
            let _ = terminal::disable_raw_mode();
            return Err(error);
        }
        Ok(Self)
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let mut out = io::stdout();
        let _ = execute!(
            &mut out,
            ResetColor,
            SetAttribute(Attribute::Reset),
            Show,
            LeaveAlternateScreen
        );
        let _ = out.flush();
        let _ = terminal::disable_raw_mode();
    }
}

fn draw(app: &App) -> io::Result<()> {
    let mut out = io::stdout();
    queue!(
        &mut out,
        Clear(ClearType::All),
        MoveTo(0, 0),
        SetForegroundColor(PINK),
        SetAttribute(Attribute::Bold),
        Print("TRUE OS"),
        SetAttribute(Attribute::Reset),
        ResetColor,
        Print("  administration\r\n"),
        SetForegroundColor(Color::DarkGrey),
        Print("Install writes a disk. Live update replaces only the running kernel.\r\n"),
        ResetColor,
        Print("\r\n")
    )?;

    match app.screen {
        Screen::Home => draw_home(&mut out, app.selected)?,
        Screen::Disks => draw_disks(&mut out, app)?,
        Screen::Source { disk } => draw_sources(&mut out, app, disk)?,
        Screen::Confirm(action) => draw_confirm(&mut out, app, action)?,
    }

    queue!(
        &mut out,
        Print("\r\n"),
        SetForegroundColor(Color::DarkGrey),
        Print("↑/↓ or j/k select   Enter choose   ←/h back   Esc/q quit"),
        ResetColor
    )?;
    out.flush()
}

fn row(out: &mut io::Stdout, selected: bool, label: impl std::fmt::Display) -> io::Result<()> {
    if selected {
        queue!(
            out,
            SetForegroundColor(PINK),
            SetAttribute(Attribute::Bold),
            Print("  › "),
            Print(label),
            SetAttribute(Attribute::Reset),
            ResetColor,
            Print("\r\n")
        )
    } else {
        queue!(out, Print("    "), Print(label), Print("\r\n"))
    }
}

fn heading(out: &mut io::Stdout, text: &str) -> io::Result<()> {
    queue!(
        out,
        SetForegroundColor(PINK),
        Print("┌─ "),
        Print(text),
        Print(" ─────────────────────────────────────────────┐\r\n"),
        ResetColor
    )
}

fn draw_home(out: &mut io::Stdout, selected: usize) -> io::Result<()> {
    heading(out, "OS")?;
    row(out, selected == 0, "Install TRUEOS to disk")?;
    row(out, selected == 1, "Live update running TRUEOS")?;
    row(out, selected == 2, "Quit")?;
    queue!(out, Print("\r\n    Choose one operation.\r\n"))
}

fn draw_disks(out: &mut io::Stdout, app: &App) -> io::Result<()> {
    heading(out, "INSTALL · TARGET DISK")?;
    if app.disks.is_empty() {
        row(out, true, "No eligible top-level disks")?;
        return Ok(());
    }
    for (index, disk) in app.disks.iter().enumerate() {
        row(
            out,
            app.selected == index,
            format!(
                "{}  {}  {}  {}  {}",
                disk.name, disk.size, disk.mode, disk.status, disk.label
            ),
        )?;
    }
    Ok(())
}

fn draw_sources(out: &mut io::Stdout, app: &App, disk: usize) -> io::Result<()> {
    heading(out, "INSTALL · SOURCE")?;
    row(out, app.selected == 0, "Local · install this booted TRUEOS")?;
    row(
        out,
        app.selected == 1,
        "Online · fetch the current release, then install",
    )?;
    if let Some(disk) = app.disks.get(disk) {
        queue!(
            out,
            Print("\r\n    Target: "),
            SetForegroundColor(PINK),
            Print(format!("{} · {} · {}", disk.name, disk.size, disk.label)),
            ResetColor,
            Print("\r\n")
        )?;
    }
    Ok(())
}

fn draw_confirm(out: &mut io::Stdout, app: &App, action: Action) -> io::Result<()> {
    heading(out, "CONFIRM")?;
    match action {
        Action::LiveUpdate => queue!(
            out,
            Print("    Fetch the current release and replace the running kernel.\r\n"),
            Print("    No disk installation will be performed.\r\n\r\n")
        )?,
        Action::Install { disk, source } => {
            let disk = app.disks.get(disk);
            let source = match source {
                InstallSource::Local => "local boot payload",
                InstallSource::Online => "online current release",
            };
            queue!(
                out,
                Print(format!(
                    "    Install {source} onto {}.\r\n",
                    disk.map(|disk| disk.name.as_str())
                        .unwrap_or("missing disk")
                )),
                SetForegroundColor(Color::Yellow),
                Print("    The selected disk will be repartitioned.\r\n\r\n"),
                ResetColor
            )?;
        }
    }
    row(out, app.selected == 0, "Cancel")?;
    row(out, app.selected == 1, "Proceed")
}
