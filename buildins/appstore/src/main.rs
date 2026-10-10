mod catalog;
mod pullbot;
mod mouse;
use std::io::{self, Write};
use std::time::{Duration};
use crossterm::{execute, queue, cursor::{MoveTo, Hide, Show}, style::{Color, SetForegroundColor, SetBackgroundColor, ResetColor, Print}, terminal::{self, Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen, DisableLineWrap, EnableLineWrap}, event::{self, Event, KeyCode, KeyEventKind, KeyModifiers, EnableMouseCapture, DisableMouseCapture, MouseButton, MouseEventKind}};
use trueos::{runtime, task::LocalSet};
const CATALOG: &str = "https://trueos.eu/apps";
const MAX_APP: usize = 512 * 1024 * 1024;
struct Terminal;
impl Terminal {
    fn enter() -> io::Result<Self> {
        terminal::enable_raw_mode()?;
        let terminal = Self;
        execute!(io::stdout(), EnterAlternateScreen, DisableLineWrap, Hide, EnableMouseCapture)?;
        Ok(terminal)
    }
}
impl Drop for Terminal {
    fn drop(&mut self) {
        let _ = execute!(io::stdout(), DisableMouseCapture, ResetColor, Show, EnableLineWrap, LeaveAlternateScreen);
        let _ = terminal::disable_raw_mode();
    }
}
fn visible(apps: &[catalog::OnlineApp], query: &str) -> Vec<usize> {
    if let Ok(id) = query.parse::<usize>() { return (id < apps.len()).then_some(id).into_iter().collect(); }
    let query = query.to_ascii_lowercase();
    apps.iter().enumerate().filter_map(|(id, app)| app.name.to_ascii_lowercase().contains(&query).then_some(id)).collect()
}
fn draw(apps: &[catalog::OnlineApp], query: &str, selected: usize, message: &str) -> io::Result<()> {
    let (width, height) = terminal::size()?;
    let width = width as usize;
    let rows = mouse::page_size(height);
    let ids = visible(apps, query);
    let start = selected / rows * rows;
    let mut out = io::stdout().lock();
    queue!(out, ResetColor, Clear(ClearType::All), MoveTo(0,0), SetForegroundColor(Color::Cyan), Print("TRUEOS APPSTORE"), ResetColor,
        MoveTo(0,1), Print(format!("{} apps · {} matches", apps.len(), ids.len())),
        MoveTo(0,3), Print(format!("Search / ID: {query}").chars().take(width).collect::<String>()))?;
    for (offset, (position, id)) in ids.iter().enumerate().skip(start).take(rows).enumerate() {
        let (x, y, cell_width) = mouse::cell(width as u16, offset);
        queue!(out, MoveTo(x, y))?;
        if position == selected { queue!(out, SetBackgroundColor(Color::DarkCyan), SetForegroundColor(Color::White))?; }
        let text = format!(" {:>2}  {}", id, apps[*id].name);
        let text = text.chars().take(cell_width).collect::<String>();
        queue!(out, Print(format!("{text}{}", " ".repeat(cell_width.saturating_sub(text.chars().count())))), ResetColor)?;
    }
    queue!(out, MoveTo(0,height.saturating_sub(2)), SetForegroundColor(Color::DarkGrey), Print("Hover select · Click/Enter launch online · Space → AppDB only · Esc quit".chars().take(width).collect::<String>()), ResetColor,
        MoveTo(0,height.saturating_sub(1)), Print(message.chars().take(width).collect::<String>()))?;
    out.flush()
}
async fn fetch(client: &reqwest::Client, url: &str, limit: usize) -> Result<Vec<u8>, String> {
    let mut response = client.get(url).send().await.map_err(|e| e.to_string())?.error_for_status().map_err(|e| e.to_string())?;
    if response.content_length().is_some_and(|len| len > limit as u64) { return Err("Download exceeds size limit".into()); }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|e| e.to_string())? {
        if chunk.len() > limit.saturating_sub(bytes.len()) { return Err("Download exceeds size limit".into()); }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}
async fn run() -> Result<Option<String>, String> {
    let client = reqwest::Client::builder().timeout(Duration::from_secs(90)).build().map_err(|e| e.to_string())?;
    let mut apps = Vec::new();
    let mut query = String::new();
    let mut selected = 0;
    draw(&apps, &query, selected, "Loading catalog…").map_err(|e|e.to_string())?;
    loop {
        match fetch(&client, CATALOG, 1024 * 1024).await {
            Ok(html) => { apps = catalog::parse(std::str::from_utf8(&html).map_err(|e| e.to_string())?); break; }
            Err(error) => {
                draw(&apps, &query, selected, &format!("{error} · R retry / Esc quit")).map_err(|e|e.to_string())?;
                loop {
                    if event::poll(Duration::from_millis(0)).map_err(|e|e.to_string())? {
                        if let Event::Key(key) = event::read().map_err(|e|e.to_string())? {
                            if key.kind == KeyEventKind::Release { continue; }
                            match key.code { KeyCode::Esc => return Ok(None), KeyCode::Char('r' | 'R') => break, _ => {} }
                        }
                    }
                    trueos::time::sleep(Duration::from_millis(25)).await;
                }
            }
        }
    }
    let mut message = String::from("Click an app to download and launch; Space adds it to AppDB without starting.");
    let mut dirty = true;
    loop {
        if dirty {
            draw(&apps, &query, selected, &message).map_err(|e|e.to_string())?;
            dirty = false;
        }
        // Batch input before rendering, but yield regularly under mouse traffic.
        let mut events = 0;
        while events < 64 && event::poll(Duration::from_millis(0)).map_err(|e|e.to_string())? {
            events += 1;
            let previous_selected = selected;
            let previous_query = query.clone();
            let ids = visible(&apps, &query);
            let mut launch = false;
            let mut save = false;
            match event::read().map_err(|e|e.to_string())? {
                Event::Key(key) if key.kind != KeyEventKind::Release => {
                    if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') { return Ok(None); }
                    match key.code {
                        KeyCode::Esc => return Ok(None),
                        KeyCode::Up => selected = selected.saturating_sub(mouse::COLUMNS),
                        KeyCode::Down => selected = (selected + mouse::COLUMNS).min(ids.len().saturating_sub(1)),
                        KeyCode::Left => selected = selected.saturating_sub(1),
                        KeyCode::Right => selected = (selected + 1).min(ids.len().saturating_sub(1)),
                        KeyCode::Char(' ') => save = true,
                        KeyCode::PageUp => selected = selected.saturating_sub(mouse::page_size(terminal::size().map_err(|e|e.to_string())?.1)),
                        KeyCode::PageDown => selected = (selected + mouse::page_size(terminal::size().map_err(|e|e.to_string())?.1)).min(ids.len().saturating_sub(1)),
                        KeyCode::Backspace => { query.pop(); selected = 0; }
                        KeyCode::Char(ch) => { if query.len() < 128 { query.push(ch); selected = 0; } }
                        KeyCode::Enter => launch = true,
                        _ => {}
                    }
                }
                Event::Mouse(mouse_event) => match mouse_event.kind {
                    MouseEventKind::ScrollUp => { selected = selected.saturating_sub(1); }
                    MouseEventKind::ScrollDown => { selected = (selected + 1).min(ids.len().saturating_sub(1)); }
                    MouseEventKind::Moved | MouseEventKind::Down(MouseButton::Left) => {
                        let (width, height) = terminal::size().map_err(|e|e.to_string())?;
                        if let Some(position) = mouse::hit_test(width, height, selected, ids.len(), mouse_event.column, mouse_event.row) {
                            selected = position;
                            launch = matches!(mouse_event.kind, MouseEventKind::Down(MouseButton::Left));
                        }
                    }
                    _ => {}
                },
                Event::Resize(_, _) => dirty = true,
                _ => {}
            }
            dirty |= selected != previous_selected || query != previous_query;
            if launch || save {
                if let Some(id) = ids.get(selected) {
                    let app = &apps[*id];
                    draw(&apps,&query,selected,&format!("Downloading {}…",app.name)).map_err(|e|e.to_string())?;
                    dirty = true;
                    match download(&client, app).await {
                        Ok(path) if save => {
                            message = match trueos::async_fs::write_file(b"vFile:appdb-install", path.as_bytes()).await {
                                Ok(()) => format!("Added {} to AppDB; not started.", app.name),
                                Err(error) => format!("AppDB import failed: {error}"),
                            };
                        }
                        Ok(path) => return Ok(Some(path)),
                        Err(error) => message = error,
                    }
                }
            }
        }
        trueos::time::sleep(Duration::from_millis(25)).await;
    }
}
async fn download(client: &reqwest::Client, app: &catalog::OnlineApp) -> Result<String, String> {
    let bytes = fetch(client, catalog::url(app), MAX_APP).await?;
    if !catalog::verify(app, &bytes) { return Err("Blueprint SHA-256 mismatch".into()); }
    let dir = "common/dl/appstore";
    trueos::async_fs::create_dir_all(dir.as_bytes()).await.map_err(|e|format!("Create download directory: {e}"))?;
    let path = format!("{dir}/{}", app.archive_name);
    trueos::async_fs::write_file(path.as_bytes(), &bytes).await.map_err(|e|format!("Save download: {e}"))?;
    Ok(path)
}
fn main() {
    // Consume the one-shot launch stream before any terminal/UI initialization.
    if let Ok(script) = trueos::async_fs::block_on(trueos::async_fs::read_file(b"vFile:launch")) {
        match runtime::current_thread_net().build() {
            Ok(runtime) => {
                LocalSet::new().block_on(&runtime, pullbot::drain(&script));
                runtime.shutdown_background();
            }
            Err(error) => pullbot::report(&format!("runtime failed: {error}")),
        }
        let _ = trueos::vshell::shutdown_current_blueprint("appstore pullbot drained");
        return;
    }
    let terminal = match Terminal::enter() { Ok(t) => t, Err(e) => { eprintln!("appstore: {e}"); return; } };
    let lease = trueos::vshell::terminal_initial_lease().ok();
    let runtime = match runtime::current_thread_net().build() { Ok(rt) => rt, Err(e) => { drop(terminal); eprintln!("appstore: {e}"); return; } };
    let result = LocalSet::new().block_on(&runtime, run());
    runtime.shutdown_background();
    drop(terminal);
    if let Some(lease) = lease { let _ = lease.release_to_shell(); }
    match result {
        Ok(Some(path)) => if let Err(code) = trueos::vshell::launch_with_script(&path, "") { eprintln!("appstore: launch failed ({code})"); },
        Ok(None) => {},
        Err(error) => eprintln!("appstore: {error}"),
    }
}
