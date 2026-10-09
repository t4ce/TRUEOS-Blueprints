use std::{
    io::{self, Write},
    time::Duration,
};

use crossterm::{
    cursor::{Hide, MoveTo, Show},
    event::{self, Event, KeyCode, KeyEvent, KeyEventKind},
    execute, queue,
    style::{Attribute, Color, Print, ResetColor, SetAttribute, SetForegroundColor},
    terminal::{self, Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen},
};
use trueos::{env, platform, runtime, task, vshell};

mod pxeproc;

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

#[derive(Clone)]
struct NonReplicatableVm {
    id: u8,
    label: String,
}

#[derive(Clone, Copy)]
enum InstallSource {
    Local,
    Online,
    PxeprocLocal,
    PxeprocOnline,
}

#[derive(Clone, Copy)]
enum Action {
    Install { disk: usize, source: InstallSource },
    LiveUpdate,
    LiveUpdateLan,
    Shutdown,
    Reboot,
}

enum Screen {
    Home,
    Disks,
    Source { disk: usize },
    PxeprocSource { disk: usize },
    Confirm(Action),
}

struct App {
    disks: Vec<Disk>,
    non_replicatable_vms: Vec<NonReplicatableVm>,
    screen: Screen,
    selected: usize,
}

impl App {
    fn new(disks: Vec<Disk>, non_replicatable_vms: Vec<NonReplicatableVm>) -> Self {
        Self {
            disks,
            non_replicatable_vms,
            screen: Screen::Home,
            selected: 0,
        }
    }

    fn item_count(&self) -> usize {
        match self.screen {
            Screen::Home => 6,
            Screen::Disks => self.disks.len().max(1),
            Screen::Source { .. } => 3,
            Screen::PxeprocSource { .. } => 2,
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
            Screen::PxeprocSource { disk } => {
                self.screen = Screen::Source { disk };
                self.selected = 0;
                false
            }
            Screen::Confirm(Action::Install { disk, source }) => {
                self.screen = if matches!(
                    source,
                    InstallSource::PxeprocLocal | InstallSource::PxeprocOnline
                ) {
                    Screen::PxeprocSource { disk }
                } else {
                    Screen::Source { disk }
                };
                self.selected = 0;
                false
            }
            Screen::Confirm(Action::LiveUpdate | Action::LiveUpdateLan) => {
                self.screen = Screen::Home;
                self.selected = 0;
                false
            }
            Screen::Confirm(Action::Shutdown | Action::Reboot) => {
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
                2 => {
                    self.screen = Screen::Confirm(Action::LiveUpdateLan);
                    self.selected = 0;
                    None
                }
                3 => {
                    self.screen = Screen::Confirm(Action::Shutdown);
                    self.selected = 0;
                    None
                }
                4 => {
                    self.screen = Screen::Confirm(Action::Reboot);
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
                if self.selected == 2 {
                    self.screen = Screen::PxeprocSource { disk };
                    self.selected = 0;
                    return None;
                }
                let source = if self.selected == 0 {
                    InstallSource::Local
                } else {
                    InstallSource::Online
                };
                self.screen = Screen::Confirm(Action::Install { disk, source });
                self.selected = 0;
                None
            }
            Screen::PxeprocSource { disk } => {
                let source = if self.selected == 0 {
                    InstallSource::PxeprocLocal
                } else {
                    InstallSource::PxeprocOnline
                };
                self.screen = Screen::Confirm(Action::Install { disk, source });
                self.selected = 0;
                None
            }
            Screen::Confirm(action) => {
                if self.selected == 0 {
                    match action {
                        Action::Install { disk, source } => {
                            self.screen = if matches!(
                                source,
                                InstallSource::PxeprocLocal | InstallSource::PxeprocOnline
                            ) {
                                Screen::PxeprocSource { disk }
                            } else {
                                Screen::Source { disk }
                            }
                        }
                        Action::LiveUpdate
                        | Action::LiveUpdateLan
                        | Action::Shutdown
                        | Action::Reboot => self.screen = Screen::Home,
                    }
                    self.selected = 0;
                    return None;
                }
                Some(match action {
                    Action::LiveUpdate => String::from("os:update:live"),
                    Action::LiveUpdateLan => String::from("os:update:lan"),
                    Action::Shutdown => String::from("os:shutdown"),
                    Action::Reboot => String::from("os:reboot"),
                    Action::Install { disk, source } => {
                        let Some(disk) = self.disks.get(disk) else {
                            return Some(String::from("os:cancel"));
                        };
                        let source = match source {
                            InstallSource::Local => "local",
                            InstallSource::Online => "online",
                            InstallSource::PxeprocLocal => "pxeproc-local",
                            InstallSource::PxeprocOnline => "pxeproc-online",
                        };
                        format!("os:install:{source}:{}", disk.id)
                    }
                })
            }
        }
    }
}

fn main() {
    let arguments = env::args().skip(1).collect::<Vec<_>>();
    let disks = arguments.iter().cloned().filter_map(parse_disk).collect();
    let non_replicatable_vms = arguments
        .into_iter()
        .filter_map(parse_non_replicatable_vm)
        .collect();

    let runtime = match runtime::current_thread().build() {
        Ok(runtime) => runtime,
        Err(_) => {
            let _ = vshell::shutdown_current_blueprint("os runtime unavailable");
            return;
        }
    };
    let lease = match vshell::terminal_initial_lease() {
        Ok(lease) => lease,
        Err(_) => {
            let _ = vshell::shutdown_current_blueprint("os terminal lease unavailable");
            return;
        }
    };

    let session = runtime.block_on(async move {
        task::spawn(async move {
            let result = run(&lease, disks, non_replicatable_vms).await;
            (lease, result)
        })
        .await
    });
    drop(runtime);

    let (lease, result) = match session {
        Ok(session) => session,
        Err(_) => {
            let _ = vshell::report_exit_reason("os:cancel");
            let _ = vshell::shutdown_current_blueprint("os:cancel");
            return;
        }
    };
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

fn parse_non_replicatable_vm(arg: String) -> Option<NonReplicatableVm> {
    let (id, label) = arg.strip_prefix("nonrep=")?.split_once('|')?;
    Some(NonReplicatableVm {
        id: id.parse().ok()?,
        label: String::from(label),
    })
}

async fn run(
    lease: &vshell::TerminalLease,
    disks: Vec<Disk>,
    non_replicatable_vms: Vec<NonReplicatableVm>,
) -> io::Result<String> {
    let _terminal = TerminalGuard::enter()?;
    let mut app = App::new(disks, non_replicatable_vms);
    draw(&app)?;
    lease
        .acknowledge_ready()
        .map_err(|error| io::Error::new(io::ErrorKind::Other, error.to_string()))?;

    // Drive the same visible UI and confirmation state machine as keyboard input.
    for code in pxeproc::launch_keys().await.map_err(io::Error::other)? {
        trueos::time::sleep(Duration::from_millis(150)).await;
        if let Some(reason) = handle_key(&mut app, KeyEvent::new(code, event::KeyModifiers::NONE)) {
            return Ok(reason);
        }
        draw(&app)?;
    }

    loop {
        if event::poll(Duration::ZERO)? {
            match event::read()? {
                Event::Resize(_, _) => draw(&app)?,
                Event::Key(key)
                    if matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat) =>
                {
                    if let Some(reason) = handle_key(&mut app, key) {
                        return Ok(reason);
                    }
                    draw(&app)?;
                }
                _ => {}
            }
            continue;
        }

        // Keep terminal readiness cooperative in userspace. A zero-timeout
        // Crossterm probe never enters TRUEOS's blocking poll path; yielding
        // both the guest and the Tokio task lets other work run without
        // parking this Hull on the terminal vthread wait primitive.
        platform::poll_once();
        task::yield_now().await;
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
        Screen::PxeprocSource { disk } => draw_pxeproc_sources(&mut out, app, disk)?,
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
    row(out, selected == 2, "Live update from LAN · 192.168.178.111")?;
    row(out, selected == 3, "Shutdown TRUEOS")?;
    row(out, selected == 4, "Reboot TRUEOS")?;
    row(out, selected == 5, "Return")?;
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
    row(out, app.selected == 2, "Install pxeproc")?;
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

fn draw_pxeproc_sources(out: &mut io::Stdout, app: &App, disk: usize) -> io::Result<()> {
    heading(out, "INSTALL PXEPROC · SOURCE")?;
    row(out, app.selected == 0, "Local · install this booted TRUEOS")?;
    row(
        out,
        app.selected == 1,
        "Online · fetch the current release, then install",
    )?;
    if let Some(disk) = app.disks.get(disk) {
        queue!(
            out,
            Print(format!(
                "\r\n    Target: {} · {} · {}\r\n",
                disk.name, disk.size, disk.label
            ))
        )?;
    }
    queue!(
        out,
        Print("    Each disk boot opens OS and live-updates from 192.168.178.111.\r\n")
    )
}

fn draw_confirm(out: &mut io::Stdout, app: &App, action: Action) -> io::Result<()> {
    heading(out, "CONFIRM")?;
    match action {
        Action::LiveUpdate | Action::LiveUpdateLan => {
            queue!(
                out,
                Print(if matches!(action, Action::LiveUpdateLan) {
                    "    Fetch the LAN image from 192.168.178.111 and replace the running kernel.\r\n"
                } else {
                    "    Fetch the current release and replace the running kernel.\r\n"
                }),
                Print("    No disk installation will be performed.\r\n")
            )?;
            if app.non_replicatable_vms.is_empty() {
                queue!(out, Print("\r\n"))?;
            } else {
                queue!(
                    out,
                    SetForegroundColor(Color::Yellow),
                    Print("    Any non-replicatable app still running at commit is discarded.\r\n"),
                    Print("    Currently affected:\r\n")
                )?;
                for vm in &app.non_replicatable_vms {
                    queue!(out, Print(format!("      vm{} · {}\r\n", vm.id, vm.label)))?;
                }
                queue!(out, ResetColor, Print("\r\n"))?;
            }
        }
        Action::Install { disk, source } => {
            let disk = app.disks.get(disk);
            let source = match source {
                InstallSource::Local => "local boot payload",
                InstallSource::Online => "online current release",
                InstallSource::PxeprocLocal => "pxeproc using the local boot payload",
                InstallSource::PxeprocOnline => "pxeproc using the online current release",
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
        Action::Shutdown => {
            queue!(
                out,
                Print("    Shut down TRUEOS now.\r\n"),
                SetForegroundColor(Color::Yellow),
                Print("    Running work will stop.\r\n\r\n"),
                ResetColor
            )?;
        }
        Action::Reboot => {
            queue!(
                out,
                Print("    Reboot TRUEOS now.\r\n"),
                SetForegroundColor(Color::Yellow),
                Print("    Running work will stop.\r\n\r\n"),
                ResetColor
            )?;
        }
    }
    row(out, app.selected == 0, "Return")?;
    row(
        out,
        app.selected == 1,
        if matches!(action, Action::LiveUpdate | Action::LiveUpdateLan)
            && !app.non_replicatable_vms.is_empty()
        {
            "Discard apps and proceed"
        } else {
            "Proceed"
        },
    )
}
