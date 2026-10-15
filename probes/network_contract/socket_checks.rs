use std::io::{ErrorKind, Read, Write};
use std::net::{Shutdown, SocketAddr, TcpStream};
use std::time::{Duration, Instant};

const LIMIT: Duration = Duration::from_secs(30);

fn pending(error: &std::io::Error) -> bool {
    matches!(error.kind(), ErrorKind::WouldBlock | ErrorKind::Interrupted)
}

fn tick(deadline: Instant, phase: &str) -> Result<(), String> {
    if Instant::now() >= deadline {
        return Err(format!("{phase}: deadline exceeded"));
    }
    std::thread::sleep(Duration::from_millis(1));
    Ok(())
}

fn send(stream: &mut TcpStream, bytes: &[u8], deadline: Instant) -> Result<(), String> {
    let mut offset = 0;
    while offset < bytes.len() {
        match stream.write(&bytes[offset..]) {
            Ok(0) => return Err("write returned zero".into()),
            Ok(n) => offset += n,
            Err(e) if pending(&e) => (),
            Err(e) => return Err(format!("write: {e}")),
        }
        tick(deadline, "send")?;
    }
    Ok(())
}

fn open(address: &str, mode: u8) -> Result<(TcpStream, Instant), String> {
    let address: SocketAddr = address
        .parse()
        .map_err(|e| format!("numeric address: {e}"))?;
    let mut stream = TcpStream::connect_timeout(&address, Duration::from_secs(5))
        .map_err(|e| format!("connect: {e}"))?;
    stream
        .set_nonblocking(true)
        .map_err(|e| format!("nonblocking: {e}"))?;
    let deadline = Instant::now() + LIMIT;
    send(&mut stream, &[mode], deadline)?;
    Ok((stream, deadline))
}

fn receive(stream: &mut TcpStream, expected: &[u8], deadline: Instant) -> Result<(), String> {
    let mut received = vec![0; expected.len()];
    let mut offset = 0;
    while offset < received.len() {
        match stream.read(&mut received[offset..]) {
            Ok(0) => return Err(format!("premature EOF at {offset}/{}", expected.len())),
            Ok(n) => offset += n,
            Err(e) if pending(&e) => (),
            Err(e) => return Err(format!("read: {e}")),
        }
        tick(deadline, "receive")?;
    }
    if received != expected {
        return Err("received bytes differ".into());
    }
    Ok(())
}

fn echo(address: &str) -> Result<(), String> {
    let (mut stream, deadline) = open(address, b'E')?;
    let expected: Vec<u8> = (0..300 * 1024)
        .map(|i| ((i * 31 + i / 257) % 251) as u8)
        .collect();
    let (mut sent, mut received) = (0, 0);
    let mut buffer = [0; 8192];
    while received < expected.len() {
        if sent < expected.len() {
            match stream.write(&expected[sent..]) {
                Ok(0) => return Err("echo write returned zero".into()),
                Ok(n) => sent += n,
                Err(e) if pending(&e) => (),
                Err(e) => return Err(format!("echo write: {e}")),
            }
        }
        match stream.read(&mut buffer) {
            Ok(0) => {
                return Err(format!(
                    "echo premature EOF sent={sent} received={received}"
                ));
            }
            Ok(n) => {
                if received + n > expected.len() || buffer[..n] != expected[received..received + n]
                {
                    return Err(format!("echo mismatch at {received}"));
                }
                received += n;
            }
            Err(e) if pending(&e) => (),
            Err(e) => return Err(format!("echo read: {e}")),
        }
        tick(deadline, "300KiB interleaved echo")?;
    }
    if sent != expected.len() {
        return Err(format!("echo incomplete send: {sent}"));
    }
    Ok(())
}

fn peek(address: &str) -> Result<(), String> {
    let (mut stream, deadline) = open(address, b'P')?;
    let expected = b"peek-witness";
    let mut buffer = [0; 32];
    loop {
        match stream.peek(&mut buffer) {
            Ok(0) => return Err("peek premature EOF".into()),
            Ok(n) => {
                if n > expected.len() || buffer[..n] != expected[..n] {
                    return Err("peek bytes differ".into());
                }
                break;
            }
            Err(e) if pending(&e) => (),
            Err(e) => return Err(format!("peek: {e}")),
        }
        tick(deadline, "peek")?;
    }
    receive(&mut stream, expected, deadline)
}

fn half_close(address: &str) -> Result<(), String> {
    let (mut stream, deadline) = open(address, b'H')?;
    send(&mut stream, b"half-close-witness", deadline)?;
    stream
        .shutdown(Shutdown::Write)
        .map_err(|e| format!("shutdown write: {e}"))?;
    receive(&mut stream, b"half-close-ok", deadline)
}

fn delayed_eof(address: &str) -> Result<(), String> {
    let (mut stream, deadline) = open(address, b'C')?;
    receive(&mut stream, b"delayed-witness", deadline)?;
    let mut byte = [0];
    loop {
        match stream.read(&mut byte) {
            Ok(0) => return Ok(()),
            Ok(_) => return Err("unexpected trailing bytes before EOF".into()),
            Err(e) if pending(&e) => (),
            Err(e) => return Err(format!("EOF read: {e}")),
        }
        tick(deadline, "EOF after delayed witness")?;
    }
}

pub fn run(address: &str) -> Vec<(&'static str, Result<(), String>)> {
    let mut results = Vec::new();
    for (name, check) in [
        ("socket.echo_300k", echo as fn(&str) -> Result<(), String>),
        ("socket.peek_preserves_bytes", peek),
        ("socket.shutdown_write_fin", half_close),
        ("socket.delayed_data_then_eof", delayed_eof),
    ] {
        crate::record("BEGIN", name, "backend=std-posix");
        let result = check(address);
        crate::record(
            "END",
            name,
            result.as_ref().err().map(String::as_str).unwrap_or(""),
        );
        results.push((name, result));
    }
    results
}
