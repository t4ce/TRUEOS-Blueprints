use std::io::{self, BufWriter, Write};
use std::net::{IpAddr, SocketAddr, UdpSocket};
use std::time::{Duration, Instant};

use crossterm::{
    cursor::{Hide, MoveTo, Show},
    event::{self, Event, KeyCode, KeyModifiers},
    execute, queue,
    style::Print,
    terminal::{self, Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen},
};
use rdp::{ReceiveEvent, Receiver};
use trueos_rdp_protocol::{DEFAULT_PORT, MAX_DATAGRAM_LEN, SUBSCRIBE_TOKEN, StreamInfo};

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
use trueos::vmedia::{LiveVideo, LiveVideoStatus};

fn endpoint(text: &str) -> Result<SocketAddr, &'static str> {
    let address = text.trim();
    let endpoint = address
        .parse::<SocketAddr>()
        .or_else(|_| {
            address
                .parse::<IpAddr>()
                .map(|ip| SocketAddr::new(ip, DEFAULT_PORT))
        })
        .map_err(|_| "Enter an IP address, optionally followed by :port")?;
    if endpoint.port() == 0 || endpoint.ip().is_unspecified() || endpoint.ip().is_multicast() {
        return Err("Enter a unicast server IP and a nonzero port");
    }
    Ok(endpoint)
}

struct Connection {
    socket: UdpSocket,
    target: SocketAddr,
    info: Option<StreamInfo>,
    receiver: Option<Receiver>,
    #[cfg(any(target_os = "trueos", target_os = "zkvm"))]
    video: Option<LiveVideo>,
    epoch: Instant,
    subscribed: Instant,
    received: Instant,
    datagrams: u64,
    bytes: u64,
    pictures: u64,
    message: String,
    probe: bool,
}

impl Connection {
    fn new(target: SocketAddr, probe: bool) -> io::Result<Self> {
        let bind = if target.is_ipv4() {
            "0.0.0.0:0"
        } else {
            "[::]:0"
        };
        let socket = UdpSocket::bind(bind)?;
        socket.set_nonblocking(true)?;
        socket.send_to(SUBSCRIBE_TOKEN, target)?;
        let now = Instant::now();
        Ok(Self {
            socket,
            target,
            info: None,
            receiver: None,
            #[cfg(any(target_os = "trueos", target_os = "zkvm"))]
            video: None,
            epoch: now,
            subscribed: now,
            received: now,
            datagrams: 0,
            bytes: 0,
            pictures: 0,
            message: "Waiting for the Ubuntu server…".into(),
            probe,
        })
    }

    fn reset_video(&mut self) {
        #[cfg(any(target_os = "trueos", target_os = "zkvm"))]
        {
            self.video = None;
        }
        if let Some(receiver) = &mut self.receiver {
            receiver.invalidate();
        }
    }

    fn dispatch(&mut self, events: Vec<ReceiveEvent>) {
        for event in events {
            match event {
                ReceiveEvent::Reset => {
                    self.reset_video();
                    self.message = "Stream interrupted; waiting for a keyframe…".into();
                }
                ReceiveEvent::AccessUnit(bytes) => {
                    self.pictures += 1;
                    if self.probe {
                        self.message = "Receiving complete H.264 pictures".into();
                        continue;
                    }
                    #[cfg(any(target_os = "trueos", target_os = "zkvm"))]
                    {
                        // dispatch may already contain later dependent pictures
                        // when an earlier submission failed in this same batch.
                        if self.video.is_none() {
                            if !rdp::independently_decodable(&bytes) {
                                continue;
                            }
                            let fps = self.info.as_ref().unwrap().fps;
                            match LiveVideo::open(fps) {
                                Ok(video) => self.video = Some(video),
                                Err(error) => {
                                    self.reset_video();
                                    self.message = format!(
                                        "Native video unavailable ({error}); retrying at next keyframe"
                                    );
                                    break;
                                }
                            }
                        }
                        if let Err(error) = self.video.as_mut().unwrap().submit_access_unit(&bytes)
                        {
                            self.reset_video();
                            self.message = format!(
                                "Video input interrupted ({error}); waiting for a keyframe"
                            );
                            break;
                        }
                        self.message = "Streaming to native UI4 video".into();
                    }
                    #[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
                    {
                        let _ = bytes;
                        self.message = "Receiving video · native display requires TRUEOS".into();
                    }
                }
            }
        }
    }

    fn poll(&mut self) -> io::Result<()> {
        if self.subscribed.elapsed() >= Duration::from_millis(500) {
            match self.socket.send_to(SUBSCRIBE_TOKEN, self.target) {
                Ok(_) => self.subscribed = Instant::now(),
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {}
                Err(error) => return Err(error),
            }
        }
        // Drain bursts without starving keyboard handling.
        let mut buffer = [0; MAX_DATAGRAM_LEN + 1];
        for _ in 0..256 {
            let (length, peer) = match self.socket.recv_from(&mut buffer) {
                Ok(result) => result,
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(error) => return Err(error),
            };
            if peer != self.target {
                continue;
            }
            let data = &buffer[..length];
            if let Ok(info) = StreamInfo::parse(data) {
                let changed = self.info.as_ref().is_none_or(|old| {
                    old.session_id != info.session_id
                        || old.width != info.width
                        || old.height != info.height
                        || old.fps != info.fps
                });
                if changed {
                    self.reset_video();
                    self.receiver = Some(Receiver::new(info.session_id));
                    self.info = Some(info);
                    self.message = "Connected; waiting for a keyframe…".into();
                }
            } else if let Some(receiver) = &mut self.receiver {
                let before = receiver.rejected;
                let events = receiver.push(data, self.epoch.elapsed().as_millis() as u64);
                if receiver.rejected == before {
                    self.datagrams += 1;
                    self.bytes += length as u64;
                    self.received = Instant::now();
                }
                self.dispatch(events);
            }
        }
        if let Some(receiver) = &mut self.receiver {
            let events = receiver.poll(self.epoch.elapsed().as_millis() as u64);
            self.dispatch(events);
        }
        if self.received.elapsed() > Duration::from_secs(3) {
            self.reset_video();
            self.receiver = None;
            self.info = None;
            self.message = "No video received · check server, IP and UDP port".into();
            self.received = Instant::now();
        }
        #[cfg(any(target_os = "trueos", target_os = "zkvm"))]
        if let Some(video) = &self.video {
            match video.status() {
                Ok(LiveVideoStatus::Ended) | Err(_) => {
                    self.reset_video();
                    self.message = "Native video stopped; waiting for a keyframe…".into();
                }
                _ => {}
            }
        }
        Ok(())
    }
}

struct Screen;
impl Screen {
    fn enter() -> io::Result<Self> {
        terminal::enable_raw_mode()?;
        let guard = Self;
        execute!(io::stdout(), EnterAlternateScreen, Hide)?;
        Ok(guard)
    }
}
impl Drop for Screen {
    fn drop(&mut self) {
        let _ = terminal::disable_raw_mode();
        let _ = execute!(io::stdout(), Show, LeaveAlternateScreen);
    }
}

fn draw(
    output: &mut impl Write,
    address: &str,
    status: &str,
    connection: Option<&Connection>,
) -> io::Result<()> {
    let (columns, rows) = terminal::size()?;
    let mut lines = vec![
        "RDP · Ubuntu window viewer".into(),
        String::new(),
        format!("Server IP: {address}▏"),
        format!("UDP port: {DEFAULT_PORT} (override with IP:port)"),
        String::new(),
        connection.map_or_else(|| status.into(), |c| c.message.clone()),
    ];
    if let Some(c) = connection {
        if let Some(info) = &c.info {
            lines.push(format!(
                "{}×{} · {} fps · {} pictures · {} recoveries",
                info.width,
                info.height,
                info.fps,
                c.pictures,
                c.receiver.as_ref().map_or(0, |rx| rx.resets)
            ));
        }
    }
    lines.extend([
        String::new(),
        "Enter connect · Esc disconnect · Ctrl+Q quit".into(),
        "View only · video opens in a separate native UI4 frame".into(),
    ]);
    queue!(output, MoveTo(0, 0), Clear(ClearType::All))?;
    for (row, line) in lines.iter().take(rows as usize).enumerate() {
        let visible: String = line
            .chars()
            .take(columns.saturating_sub(1) as usize)
            .collect();
        queue!(output, MoveTo(0, row as u16), Print(visible))?;
    }
    output.flush()
}

fn tui(initial: String) -> io::Result<()> {
    let _screen = Screen::enter()?;
    let mut output = BufWriter::new(io::stdout());
    let mut address = initial;
    let mut connection: Option<Connection> = None;
    let mut status = String::from("Enter the Ubuntu server's IP address");
    let mut drawn = Instant::now() - Duration::from_secs(1);
    loop {
        if let Some(active) = &mut connection {
            if let Err(error) = active.poll() {
                status = format!("Connection stopped: {error}");
                connection = None;
            }
        }
        if drawn.elapsed() >= Duration::from_millis(200) {
            draw(&mut output, &address, &status, connection.as_ref())?;
            drawn = Instant::now();
        }
        if event::poll(Duration::from_millis(2))? {
            match event::read()? {
                Event::Key(key) if key.is_press() => {
                    if key.modifiers.contains(KeyModifiers::CONTROL)
                        && matches!(key.code, KeyCode::Char('q' | 'c'))
                    {
                        return Ok(());
                    }
                    match key.code {
                        KeyCode::Enter => match endpoint(&address) {
                            Ok(target) => {
                                connection = None;
                                match Connection::new(target, false) {
                                    Ok(active) => connection = Some(active),
                                    Err(error) => status = format!("Cannot connect: {error}"),
                                }
                            }
                            Err(error) => status = error.into(),
                        },
                        KeyCode::Esc => {
                            connection = None;
                            status = "Disconnected".into();
                        }
                        KeyCode::Backspace => {
                            address.pop();
                        }
                        KeyCode::Char(ch)
                            if address.len() < 64
                                && (ch.is_ascii_hexdigit()
                                    || matches!(ch, '.' | ':' | '[' | ']')) =>
                        {
                            address.push(ch)
                        }
                        _ => {}
                    }
                    drawn = Instant::now() - Duration::from_secs(1);
                }
                Event::Resize(_, _) => drawn = Instant::now() - Duration::from_secs(1),
                _ => {}
            }
        }
    }
}

fn run() -> io::Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args
        .first()
        .is_some_and(|a| matches!(a.as_str(), "--help" | "-h"))
    {
        println!(
            "rdp [IP[:PORT]]\nrdp --probe SECONDS IP[:PORT]\nView-only Ubuntu window stream, UDP {DEFAULT_PORT}. Enter connects in the TUI."
        );
        return Ok(());
    }
    if args.first().is_some_and(|a| a == "--probe") {
        let seconds = args
            .get(1)
            .and_then(|s| s.parse::<u64>().ok())
            .filter(|s| (1..=3600).contains(s))
            .ok_or_else(|| io::Error::other("--probe requires seconds (1..3600) and server IP"))?;
        let target = endpoint(
            args.get(2)
                .ok_or_else(|| io::Error::other("missing server IP"))?,
        )
        .map_err(io::Error::other)?;
        let mut connection = Connection::new(target, true)?;
        while connection.epoch.elapsed() < Duration::from_secs(seconds) {
            connection.poll()?;
            std::thread::sleep(Duration::from_millis(1));
        }
        println!(
            "received pictures={} datagrams={} bytes={} status={}",
            connection.pictures, connection.datagrams, connection.bytes, connection.message
        );
        return if connection.pictures > 0 {
            Ok(())
        } else {
            Err(io::Error::other(
                "no independently decodable H.264 pictures received",
            ))
        };
    }
    if args.len() > 1 {
        return Err(io::Error::other("usage: rdp [IP[:PORT]]"));
    }
    tui(args.first().cloned().unwrap_or_default())
}

fn main() -> io::Result<()> {
    let result = run();
    #[cfg(any(target_os = "trueos", target_os = "zkvm"))]
    trueos::vshell::leave_terminal_handoff();
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_ip_and_port_without_dns() {
        assert_eq!(endpoint("192.0.2.1").unwrap().port(), 43402);
        assert_eq!(endpoint("192.0.2.1:5000").unwrap().port(), 5000);
        assert_eq!(endpoint("[::1]:5000").unwrap().port(), 5000);
        for invalid in ["hostname", "0.0.0.0", "224.0.0.1", "127.0.0.1:0"] {
            assert!(endpoint(invalid).is_err());
        }
    }
}
