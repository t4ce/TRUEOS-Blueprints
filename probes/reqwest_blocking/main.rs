use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::{Duration, Instant};
use trueos::logl::{self, level};

const SERVER_DEADLINE: Duration = Duration::from_secs(8);
const STREAM_DEADLINE: Duration = Duration::from_secs(4);
const REQUEST_TIMEOUT: Duration = Duration::from_millis(150);
const SLOW_RESPONSE: Duration = Duration::from_millis(400);

fn main() {
    logl::log(level::INFO, format_args!("reqwest_blocking: start"));
    match run() {
        Ok(()) => logl::log(
            level::INFO,
            format_args!(
                "reqwest_blocking: PASS stream=2 header_timeout=PASS body_timeout=PASS recreate=PASS requests=4"
            ),
        ),
        Err(stage) => logl::log(
            level::ERROR,
            format_args!("reqwest_blocking: FAIL stage={stage}"),
        ),
    }
}

fn run() -> Result<(), &'static str> {
    let listener = TcpListener::bind("127.0.0.1:0").map_err(|e| fail("server.bind", e))?;
    let address = listener.local_addr().map_err(|e| fail("server.addr", e))?;
    listener
        .set_nonblocking(true)
        .map_err(|e| fail("server.nonblocking", e))?;
    let first_chunk_read = Arc::new(AtomicBool::new(false));
    let stop = Arc::new(AtomicBool::new(false));
    let server = {
        let first_chunk_read = first_chunk_read.clone();
        let stop = stop.clone();
        std::thread::Builder::new()
            .name("reqwest-probe-server".into())
            .spawn(move || serve(listener, first_chunk_read, stop))
            .map_err(|e| fail("server.spawn", e))?
    };

    // The blocking client starts an async Tokio helper thread. Calling it from
    // this ordinary thread lets its channel waits and wakeups run end to end.
    let client_result = request_sequence(address, &first_chunk_read);
    stop.store(true, Ordering::Release);
    let server_result = server.join().map_err(|_| "server.join")?;
    client_result?;
    if server_result? != 4 {
        return Err("server.request_count");
    }
    Ok(())
}

fn request_sequence(
    address: SocketAddr,
    first_chunk_read: &AtomicBool,
) -> Result<(), &'static str> {
    let base = format!("http://{address}");
    let client = reqwest::blocking::Client::builder()
        .no_proxy()
        .timeout(STREAM_DEADLINE)
        .build()
        .map_err(|e| fail("client.build", e))?;

    logl::log(
        level::INFO,
        format_args!("reqwest_blocking: phase=stream.send"),
    );
    let mut response = client
        .get(format!("{base}/stream"))
        .send()
        .map_err(|e| fail("stream.send", e))?;
    if response.status() != reqwest::StatusCode::OK {
        return Err("stream.status");
    }
    // The response must keep Reqwest's helper runtime alive after its client
    // handle is gone, through both reads and the final response drop.
    drop(client);
    let mut first = [0; 6];
    response
        .read_exact(&mut first)
        .map_err(|e| fail("stream.first_read", e))?;
    if first != *b"hello " {
        return Err("stream.first_body");
    }
    // The server waits for this acknowledgement before writing the rest.
    // A full-body buffer or failed thread wakeup cannot pass this exchange.
    first_chunk_read.store(true, Ordering::Release);
    let mut rest = Vec::new();
    response
        .read_to_end(&mut rest)
        .map_err(|e| fail("stream.rest_read", e))?;
    if rest != b"world" {
        return Err("stream.rest_body");
    }
    drop(response);
    logl::log(
        level::INFO,
        format_args!("reqwest_blocking: phase=stream.done"),
    );

    let client = reqwest::blocking::Client::builder()
        .no_proxy()
        .timeout(STREAM_DEADLINE)
        .build()
        .map_err(|e| fail("second_client.build", e))?;
    logl::log(
        level::INFO,
        format_args!("reqwest_blocking: phase=body_timeout.send"),
    );
    let mut stalled = client
        .get(format!("{base}/stall"))
        .timeout(REQUEST_TIMEOUT)
        .send()
        .map_err(|e| fail("body_timeout.send", e))?;
    if stalled.status() != reqwest::StatusCode::OK {
        return Err("body_timeout.status");
    }
    let mut first = [0; 2];
    stalled
        .read_exact(&mut first)
        .map_err(|e| fail("body_timeout.first_read", e))?;
    if first != *b"go" {
        return Err("body_timeout.first_body");
    }
    let mut rest = Vec::new();
    let body_error = stalled
        .read_to_end(&mut rest)
        .err()
        .ok_or("body_timeout.expected_error")?;
    if !body_error
        .get_ref()
        .and_then(|source| source.downcast_ref::<reqwest::Error>())
        .is_some_and(reqwest::Error::is_timeout)
    {
        logl::log(
            level::ERROR,
            format_args!("reqwest_blocking: body timeout error={body_error}"),
        );
        return Err("body_timeout.kind");
    }
    drop(stalled);
    logl::log(
        level::INFO,
        format_args!("reqwest_blocking: phase=body_timeout.done"),
    );

    logl::log(
        level::INFO,
        format_args!("reqwest_blocking: phase=timeout.send"),
    );
    let timeout_error = client
        .get(format!("{base}/timeout"))
        .timeout(REQUEST_TIMEOUT)
        .send()
        .err()
        .ok_or("timeout.expected_error")?;
    if !timeout_error.is_timeout() {
        logl::log(
            level::ERROR,
            format_args!("reqwest_blocking: timeout error={timeout_error}"),
        );
        return Err("timeout.kind");
    }
    drop(client); // Joins the helper runtime before a fresh client is built.
    logl::log(
        level::INFO,
        format_args!("reqwest_blocking: phase=timeout.done"),
    );

    let recreated = reqwest::blocking::Client::builder()
        .no_proxy()
        .timeout(STREAM_DEADLINE)
        .build()
        .map_err(|e| fail("recreate.build", e))?;
    let response = recreated
        .get(format!("{base}/again"))
        .send()
        .map_err(|e| fail("recreate.send", e))?;
    if response.status() != reqwest::StatusCode::CREATED {
        return Err("recreate.status");
    }
    let body = response.text().map_err(|e| fail("recreate.body", e))?;
    if body != "fresh" {
        return Err("recreate.body_value");
    }
    drop(recreated);
    logl::log(
        level::INFO,
        format_args!("reqwest_blocking: phase=recreate.done"),
    );
    Ok(())
}

fn serve(
    listener: TcpListener,
    first_chunk_read: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
) -> Result<usize, &'static str> {
    let deadline = Instant::now() + SERVER_DEADLINE;
    let mut count = 0;
    while count < 4 && !stop.load(Ordering::Acquire) && Instant::now() < deadline {
        let (mut stream, _) = match listener.accept() {
            Ok(connection) => connection,
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(2));
                continue;
            }
            Err(e) => return Err(fail("server.accept", e)),
        };
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .map_err(|e| fail("server.read_timeout", e))?;
        let path = read_path(&mut stream)?;
        match path.as_str() {
            "/stream" => {
                stream
                    .write_all(
                        b"HTTP/1.1 200 OK\r\nContent-Length: 11\r\nConnection: close\r\n\r\nhello ",
                    )
                    .map_err(|e| fail("server.stream_first", e))?;
                let ack_deadline = Instant::now() + STREAM_DEADLINE;
                while !first_chunk_read.load(Ordering::Acquire)
                    && !stop.load(Ordering::Acquire)
                    && Instant::now() < ack_deadline
                {
                    std::thread::sleep(Duration::from_millis(2));
                }
                if !first_chunk_read.load(Ordering::Acquire) {
                    return Err("server.stream_ack");
                }
                stream
                    .write_all(b"world")
                    .map_err(|e| fail("server.stream_rest", e))?;
            }
            "/timeout" => {
                std::thread::sleep(SLOW_RESPONSE);
                // The client has timed out and may have closed this connection.
                let _ = stream.write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\nConnection: close\r\n\r\nlate",
                );
            }
            "/stall" => {
                stream
                    .write_all(
                        b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\nConnection: close\r\n\r\ngo",
                    )
                    .map_err(|e| fail("server.stall_first", e))?;
                std::thread::sleep(SLOW_RESPONSE);
                let _ = stream.write_all(b"on");
            }
            "/again" => stream
                .write_all(
                    b"HTTP/1.1 201 Created\r\nContent-Length: 5\r\nConnection: close\r\n\r\nfresh",
                )
                .map_err(|e| fail("server.again_write", e))?,
            _ => return Err("server.unexpected_path"),
        }
        count += 1;
    }
    Ok(count)
}

fn read_path(stream: &mut TcpStream) -> Result<String, &'static str> {
    let mut request = Vec::new();
    while request.len() < 2048 && !request.ends_with(b"\r\n\r\n") {
        let mut byte = [0];
        stream
            .read_exact(&mut byte)
            .map_err(|e| fail("server.request_read", e))?;
        request.push(byte[0]);
    }
    if !request.ends_with(b"\r\n\r\n") {
        return Err("server.request_headers");
    }
    let request = std::str::from_utf8(&request).map_err(|_| "server.request_utf8")?;
    let path = request
        .lines()
        .next()
        .and_then(|line| line.strip_prefix("GET "))
        .and_then(|line| line.split_once(' ').map(|(path, _)| path))
        .ok_or("server.request_line")?;
    Ok(path.to_owned())
}

fn fail(stage: &'static str, error: impl std::fmt::Display) -> &'static str {
    logl::log(
        level::ERROR,
        format_args!("reqwest_blocking: stage={stage} error={error}"),
    );
    stage
}
