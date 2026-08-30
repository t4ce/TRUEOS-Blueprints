#[cfg(target_os = "trueos")]
mod trueos_app {
    use std::{fmt::Write as _, net::Ipv4Addr, sync::Arc};

    use crossterm::terminal;
    use pi_snake::{Cell, Game, PORT, snake_glyph};
    use trueos::{
        logl,
        logl::level,
        net::TcpListener,
        runtime,
        sync::Mutex,
        time::{self, Duration, Instant},
        tokio::{
            self,
            io::{AsyncReadExt, AsyncWriteExt},
        },
        vshell,
    };

    const POLL_MS: u64 = 20;
    const RENDER_MS: u64 = 50;
    const INPUT_CAP: usize = 512;

    type SharedGame = Arc<Mutex<Game>>;

    /// Keeps the terminal handoff contract intact on every return path.
    struct RawMode;

    impl RawMode {
        fn enable() -> std::io::Result<Self> {
            terminal::enable_raw_mode()?;
            Ok(Self)
        }
    }

    impl Drop for RawMode {
        fn drop(&mut self) {
            let _ = terminal::disable_raw_mode();
        }
    }

    pub fn main() {
        let runtime = match runtime::current_thread_net().build() {
            Ok(runtime) => runtime,
            Err(error) => {
                logl::log(
                    level::ERROR,
                    format_args!("pi_snake: runtime failed: {error}"),
                );
                return;
            }
        };
        let local = tokio::task::LocalSet::new();
        local.block_on(&runtime, async {
            if let Err(error) = run().await {
                logl::log(level::ERROR, format_args!("pi_snake: {error}"));
            }
        });
    }

    async fn run() -> Result<(), &'static str> {
        let lease =
            vshell::terminal_initial_lease().map_err(|_| "could not claim Shell2 terminal")?;
        let surface = lease
            .surface_snapshot()
            .map_err(|_| "could not read Shell2 size")?;
        let game = Arc::new(Mutex::new(Game::new(
            surface.columns.saturating_sub(2) as u16,
            surface.rows.saturating_sub(4) as u16,
        )));

        // Do this only after the terminal lease is held. It is a direct cell
        // renderer: no compositor window and no retained UI surface.
        let _raw_mode = RawMode::enable().map_err(|_| "could not enable raw Shell2 input")?;
        vshell::attached_write(b"\x1b[?1049h\x1b[?25l\x1b[2J\x1b[H");
        lease
            .acknowledge_ready()
            .map_err(|_| "Shell2 terminal lease went stale")?;

        let listener = TcpListener::bind((Ipv4Addr::UNSPECIFIED, PORT))
            .await
            .map_err(|_| "could not bind TCP port 45329")?;
        logl::log(
            level::INFO,
            "pi_snake: Shell2 immediate UI, HTTP server on tcp/45329",
        );
        tokio::task::spawn_local(serve(listener, Arc::clone(&game)));

        let started = Instant::now();
        let mut last_render = 0_u64;
        let mut last_surface_generation = surface.generation;
        let mut escape = EscapeInput::default();
        loop {
            let now_ms = started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64;
            let mut bytes = [0u8; INPUT_CAP];
            let read = vshell::attached_read_available(&mut bytes);
            let mut should_exit = false;
            {
                let mut game = game.lock().await;
                for &byte in &bytes[..read] {
                    if let Some(key) = escape.push(byte) {
                        if key == 'q' || key == 'Q' {
                            should_exit = true;
                        } else {
                            game.input(0, key, now_ms);
                        }
                    }
                }
                if let Ok(surface) = lease.surface_snapshot()
                    && surface.generation != last_surface_generation
                {
                    last_surface_generation = surface.generation;
                    game.resize(
                        surface.columns.saturating_sub(2) as u16,
                        surface.rows.saturating_sub(4) as u16,
                    );
                }
                game.update(now_ms);
                if game.take_dirty() || now_ms.saturating_sub(last_render) >= RENDER_MS {
                    vshell::attached_write(render(&game).as_bytes());
                    last_render = now_ms;
                }
            }
            if should_exit {
                break;
            }
            time::sleep(Duration::from_millis(POLL_MS)).await;
        }
        vshell::attached_write(b"\x1b[?25h\x1b[?1049l");
        let _ = lease.release_to_shell();
        Ok(())
    }

    async fn serve(listener: TcpListener, game: SharedGame) {
        loop {
            match listener.accept().await {
                Ok((stream, _)) => {
                    let game = Arc::clone(&game);
                    tokio::task::spawn_local(async move {
                        let _ = handle_connection(stream, game).await;
                    });
                }
                Err(error) => logl::log(
                    level::WARN,
                    format_args!("pi_snake: accept failed: {error}"),
                ),
            }
        }
    }

    async fn handle_connection(
        mut stream: trueos::net::TcpStream,
        game: SharedGame,
    ) -> std::io::Result<()> {
        let request = read_request(&mut stream).await?;
        if !request.starts_with("GET ") && !request.starts_with("POST ") {
            // A bare TCP client can control P2 with a single WASD/3 byte.
            if let Some(key) = request.chars().find(|key| !key.is_whitespace()) {
                let mut game = game.lock().await;
                let _ = game.join_remote();
                let now_ms = game.clock_ms;
                game.input(1, key, now_ms);
            }
            stream.write_all(b"pi_snake p2 ready\n").await?;
            return Ok(());
        }
        let (head, body) = request
            .split_once("\r\n\r\n")
            .unwrap_or((request.as_str(), ""));
        let mut fields = head.lines().next().unwrap_or_default().split_whitespace();
        let method = fields.next().unwrap_or_default();
        let target = fields.next().unwrap_or("/");
        let (content_type, response, found) = match (method, target) {
            ("GET", "/") => ("text/html; charset=utf-8", INDEX_HTML.to_owned(), true),
            ("GET", "/app.js") => (
                "application/javascript; charset=utf-8",
                APP_JS.to_owned(),
                true,
            ),
            ("GET", "/api/state") => (
                "application/json; charset=utf-8",
                snapshot_json(&*game.lock().await),
                true,
            ),
            ("POST", "/api/join") => {
                let joined = game.lock().await.join_remote();
                (
                    "application/json; charset=utf-8",
                    format!("{{\"ok\":true,\"joined\":{joined}}}"),
                    true,
                )
            }
            ("POST", "/api/input") => {
                if let Some(key) = form_key(body) {
                    let mut game = game.lock().await;
                    let _ = game.join_remote();
                    let now_ms = game.clock_ms;
                    game.input(1, key, now_ms);
                }
                (
                    "application/json; charset=utf-8",
                    "{\"ok\":true}".to_owned(),
                    true,
                )
            }
            _ => ("text/plain; charset=utf-8", "not found".to_owned(), false),
        };
        let status = if found { "200 OK" } else { "404 Not Found" };
        let response = format!(
            "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n{response}",
            response.len()
        );
        stream.write_all(response.as_bytes()).await?;
        let _ = stream.shutdown().await;
        Ok(())
    }

    async fn read_request(stream: &mut trueos::net::TcpStream) -> std::io::Result<String> {
        let mut bytes = Vec::with_capacity(1024);
        let mut buffer = [0u8; 1024];
        loop {
            let count = stream.read(&mut buffer).await?;
            if count == 0 || bytes.len() >= 8 * 1024 {
                break;
            }
            bytes.extend_from_slice(&buffer[..count]);
            let Some(header_end) = bytes.windows(4).position(|window| window == b"\r\n\r\n") else {
                continue;
            };
            let header_end = header_end + 4;
            let headers = String::from_utf8_lossy(&bytes[..header_end]);
            let content_length = headers
                .lines()
                .find_map(|line| line.strip_prefix("Content-Length:"))
                .and_then(|value| value.trim().parse::<usize>().ok())
                .unwrap_or(0);
            if bytes.len() >= header_end.saturating_add(content_length) {
                break;
            }
        }
        Ok(String::from_utf8_lossy(&bytes).into_owned())
    }

    fn form_key(body: &str) -> Option<char> {
        body.split('&').find_map(|pair| {
            let (name, value) = pair.split_once('=')?;
            (name == "key").then(|| value.chars().next()).flatten()
        })
    }

    fn snapshot_json(game: &Game) -> String {
        let mut out = format!(
            "{{\"width\":{},\"height\":{},\"apples\":[",
            game.width, game.height
        );
        json_cells(&mut out, &game.apples);
        out.push_str("],\"players\":[");
        for (index, player) in game.players.iter().enumerate() {
            if index != 0 {
                out.push(',');
            }
            let _ = write!(
                out,
                "{{\"id\":{},\"joined\":{},\"started\":{},\"paused\":{},\"piChart\":{},\"expected\":",
                index + 1,
                player.joined,
                player.started,
                player.awaiting_pi || player.awaiting_direction,
                player.pi_chart
            );
            match player.expected_pi() {
                Some(character) => {
                    let _ = write!(out, "\"{character}\"");
                }
                None => out.push_str("null"),
            }
            out.push_str(",\"snake\":[");
            let cells: Vec<Cell> = player.snake.iter().copied().collect();
            json_cells(&mut out, &cells);
            out.push_str("]}");
        }
        let _ = write!(out, "],\"status\":\"{}\"}}", json_escape(&game.status));
        out
    }

    fn json_cells(out: &mut String, cells: &[Cell]) {
        for (index, cell) in cells.iter().enumerate() {
            if index != 0 {
                out.push(',');
            }
            let _ = write!(out, "[{},{}]", cell.x, cell.y);
        }
    }

    fn json_escape(input: &str) -> String {
        input.replace('\\', "\\\\").replace('"', "\\\"")
    }

    fn render(game: &Game) -> String {
        let mut cells = vec![' '; game.width as usize * game.height as usize];
        for apple in &game.apples {
            put(&mut cells, game.width, *apple, '@');
        }
        for (id, player) in game.players.iter().enumerate() {
            let snake_length = player.snake.len();
            for (index, cell) in player.snake.iter().enumerate() {
                put(
                    &mut cells,
                    game.width,
                    *cell,
                    snake_glyph(index, snake_length),
                );
            }
            if player.pi_chart {
                draw_pi_chart(
                    &mut cells,
                    game.width,
                    game.height,
                    if id == 0 {
                        2
                    } else {
                        game.width.saturating_sub(7)
                    },
                    1,
                );
            }
        }
        let mut out = String::from(
            "\x1b[H\x1b[2J\x1b[1;36mPI SNAKE\x1b[0m  P1 WASD/arrows · P2 browser :45329 · q exits\r\n",
        );
        out.push('+');
        for _ in 0..game.width {
            out.push('-');
        }
        out.push_str("+\r\n");
        for y in 0..game.height {
            out.push('|');
            for x in 0..game.width {
                let cell = cells[y as usize * game.width as usize + x as usize];
                match cell {
                    '@' => out.push_str("\x1b[31m@\x1b[0m"),
                    'π' => out.push_str("\x1b[35mπ\x1b[0m"),
                    ' ' => out.push(' '),
                    digit => {
                        out.push_str("\x1b[32m");
                        out.push(digit);
                        out.push_str("\x1b[0m");
                    }
                }
            }
            out.push_str("|\r\n");
        }
        out.push('+');
        for _ in 0..game.width {
            out.push('-');
        }
        let _ = write!(out, "+\r\n{}\r\n", game.status);
        out
    }

    fn put(cells: &mut [char], width: u16, cell: Cell, character: char) {
        let index = cell.y as usize * width as usize + cell.x as usize;
        if let Some(slot) = cells.get_mut(index) {
            *slot = character;
        }
    }

    fn draw_pi_chart(cells: &mut [char], width: u16, height: u16, x: u16, y: u16) {
        for (dy, row) in ["πππππ", " π π ", " π π ", " π π "].iter().enumerate() {
            for (dx, character) in row.chars().enumerate() {
                let point = Cell {
                    x: x.saturating_add(dx as u16),
                    y: y.saturating_add(dy as u16),
                };
                if point.x < width && point.y < height && character != ' ' {
                    put(cells, width, point, character);
                }
            }
        }
    }

    #[derive(Default)]
    struct EscapeInput {
        state: u8,
    }

    impl EscapeInput {
        fn push(&mut self, byte: u8) -> Option<char> {
            match (self.state, byte) {
                (0, 0x1b) => {
                    self.state = 1;
                    None
                }
                (1, b'[') => {
                    self.state = 2;
                    None
                }
                (2, b'A') => {
                    self.state = 0;
                    Some('w')
                }
                (2, b'B') => {
                    self.state = 0;
                    Some('s')
                }
                (2, b'C') => {
                    self.state = 0;
                    Some('d')
                }
                (2, b'D') => {
                    self.state = 0;
                    Some('a')
                }
                (_, byte) => {
                    self.state = 0;
                    (byte.is_ascii()).then_some(byte as char)
                }
            }
        }
    }

    const INDEX_HTML: &str = include_str!("../web/index.html");
    const APP_JS: &str = include_str!("../web/app.js");
}

#[cfg(target_os = "trueos")]
fn main() {
    trueos_app::main();
}

#[cfg(not(target_os = "trueos"))]
fn main() {
    eprintln!("pi_snake is a TRUEOS Blueprint; build it with `cargo bp pi_snake`.");
}
