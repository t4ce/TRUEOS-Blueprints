use core::net::{Ipv4Addr, SocketAddr};

use trueos::platform;
use trueos::{
    logl::{self, level},
    platform::{format, thread},
    t,
};

const PROBE_WAIT_BUDGET_MS: u64 = 1_500;
const PROBE_WAIT_SLICE_MS: u64 = 25;
const DNS_WAIT_BUDGET_MS: u64 = 5_000;
const REMOTE_HTTP_HOST: &str = "example.com";

fn main() {
    logl::log(level::INFO, format_args!("tokio_net: start"));

    if let Err(stage) = probe_runtime_bootstrap_surfaces() {
        logl::log(
            level::ERROR,
            format_args!("tokio_net: bootstrap failed stage={}", stage),
        );
        return;
    }

    logl::log(
        level::INFO,
        format_args!("tokio_net: stage runtime.current_thread_net.build"),
    );

    let runtime = match t::runtime::current_thread_net().build() {
        Ok(rt) => rt,
        Err(err) => {
            logl::log(
                level::ERROR,
                format_args!("tokio_net: runtime build failed: {}", err),
            );
            return;
        }
    };
    logl::log(
        level::INFO,
        format_args!("tokio_net: success runtime.current_thread_net.build"),
    );

    runtime.block_on(async {
        match run_probe().await {
            Ok(()) => logl::log(level::INFO, format_args!("tokio_net: done")),
            Err(stage) => logl::log(
                level::ERROR,
                format_args!("tokio_net: failed stage={}", stage),
            ),
        }
    });
}

fn probe_runtime_bootstrap_surfaces() -> Result<(), &'static str> {
    logl::log(
        level::INFO,
        format_args!("tokio_net: stage thread.current.id"),
    );
    let thread_id = thread::current().id();
    logl::log(
        level::INFO,
        format_args!("tokio_net: success thread.current.id id={:?}", thread_id),
    );

    logl::log(
        level::INFO,
        format_args!("tokio_net: stage thread.yield_now"),
    );
    thread::yield_now();
    logl::log(
        level::INFO,
        format_args!("tokio_net: success thread.yield_now"),
    );

    logl::log(
        level::INFO,
        format_args!("tokio_net: stage runtime.current_thread.builder_new_plain"),
    );
    let mut builder = t::tokio::runtime::Builder::new_current_thread();
    logl::log(
        level::INFO,
        format_args!("tokio_net: success runtime.current_thread.builder_new_plain"),
    );

    logl::log(
        level::INFO,
        format_args!("tokio_net: stage runtime.current_thread.builder_build_plain"),
    );
    let runtime = builder
        .build()
        .map_err(|_| "runtime.current_thread.builder_build_plain")?;
    logl::log(
        level::INFO,
        format_args!("tokio_net: success runtime.current_thread.build_plain"),
    );

    logl::log(
        level::INFO,
        format_args!("tokio_net: stage runtime.current_thread.drop_plain"),
    );
    drop(runtime);
    logl::log(
        level::INFO,
        format_args!("tokio_net: success runtime.current_thread.drop_plain"),
    );

    logl::log(
        level::INFO,
        format_args!("tokio_net: stage runtime.current_thread.build_time"),
    );
    let runtime = t::runtime::current_thread()
        .build()
        .map_err(|_| "runtime.current_thread.build_time")?;
    logl::log(
        level::INFO,
        format_args!("tokio_net: success runtime.current_thread.build_time"),
    );

    logl::log(
        level::INFO,
        format_args!("tokio_net: stage runtime.current_thread.drop_time"),
    );
    drop(runtime);
    logl::log(
        level::INFO,
        format_args!("tokio_net: success runtime.current_thread.drop_time"),
    );

    logl::log(
        level::INFO,
        format_args!("tokio_net: stage runtime.current_thread.builder_new_io_no_time"),
    );
    let mut builder = t::tokio::runtime::Builder::new_current_thread();
    logl::log(
        level::INFO,
        format_args!("tokio_net: success runtime.current_thread.builder_new_io_no_time"),
    );

    logl::log(
        level::INFO,
        format_args!("tokio_net: stage runtime.current_thread.build_io_no_time"),
    );
    builder.enable_io();
    let runtime = builder
        .build()
        .map_err(|_| "runtime.current_thread.build_io_no_time")?;
    logl::log(
        level::INFO,
        format_args!("tokio_net: success runtime.current_thread.build_io_no_time"),
    );

    logl::log(
        level::INFO,
        format_args!("tokio_net: stage runtime.current_thread.drop_io_no_time"),
    );
    drop(runtime);
    logl::log(
        level::INFO,
        format_args!("tokio_net: success runtime.current_thread.drop_io_no_time"),
    );

    logl::log(
        level::INFO,
        format_args!("tokio_net: stage mio.poll.wake.bootstrap"),
    );
    probe_mio_poll_surface()?;
    logl::log(
        level::INFO,
        format_args!("tokio_net: success mio.poll.wake.bootstrap"),
    );

    Ok(())
}

async fn run_probe() -> Result<(), &'static str> {
    logl::log(
        level::INFO,
        format_args!("tokio_net: stage net.lookup_host"),
    );
    let resolved = probe_lookup_host().await?;
    logl::log(
        level::INFO,
        format_args!("tokio_net: stage std.tcp.connect_timeout"),
    );
    probe_std_tcp_connect_timeout(resolved)?;

    logl::log(
        level::INFO,
        format_args!("tokio_net: stage net.socket2.new"),
    );
    probe_socket2_surface()?;

    logl::log(level::INFO, format_args!("tokio_net: stage mio.poll.wake"));
    probe_mio_poll_surface()?;

    logl::log(
        level::INFO,
        format_args!("tokio_net: stage mio.net.udp.bind"),
    );
    probe_mio_udp_bind().await?;

    logl::log(level::INFO, format_args!("tokio_net: stage net.udp.bind"));
    probe_udp_bind().await?;

    logl::log(
        level::INFO,
        format_args!("tokio_net: stage net.udp.loopback_roundtrip"),
    );
    probe_udp_loopback().await?;

    logl::log(
        level::INFO,
        format_args!("tokio_net: stage net.tcp.loopback_roundtrip"),
    );
    match probe_tcp_loopback().await {
        Ok(()) => logl::log(
            level::INFO,
            format_args!("tokio_net: success net.tcp.loopback_roundtrip"),
        ),
        Err(stage) => {
            logl::log(
                level::INFO,
                format_args!(
                    "tokio_net: note net.tcp.loopback unavailable, fallback={}",
                    stage
                ),
            );
            logl::log(
                level::INFO,
                format_args!("tokio_net: stage net.tcp.remote_http"),
            );
            match probe_remote_http().await {
                Ok(()) => {}
                Err(stage) => {
                    logl::log(
                        level::INFO,
                        format_args!(
                            "tokio_net: note net.tcp.remote_http unavailable, fallback={}",
                            stage
                        ),
                    );
                }
            }
        }
    }

    Ok(())
}

async fn probe_lookup_host() -> Result<SocketAddr, &'static str> {
    let mut addresses = t::time::timeout(
        t::time::Duration::from_millis(DNS_WAIT_BUDGET_MS),
        t::net::resolve_host(REMOTE_HTTP_HOST, 443),
    )
    .await
    .map_err(|_| "net.lookup_host.timeout")?
    .map_err(|_| "net.lookup_host.resolve")?;
    let address = addresses.into_iter().next().ok_or("net.lookup_host.empty")?;
    logl::log(
        level::INFO,
        format_args!(
            "tokio_net: success net.lookup_host host={} address={}",
            REMOTE_HTTP_HOST, address
        ),
    );
    Ok(address)
}

fn probe_std_tcp_connect_timeout(address: SocketAddr) -> Result<(), &'static str> {
    let stream = std::net::TcpStream::connect_timeout(
        &address,
        core::time::Duration::from_millis(DNS_WAIT_BUDGET_MS),
    )
    .map_err(|_| "std.tcp.connect_timeout")?;
    logl::log(
        level::INFO,
        format_args!(
            "tokio_net: success std.tcp.connect_timeout peer={}",
            address
        ),
    );
    drop(stream);
    Ok(())
}

fn probe_socket2_surface() -> Result<(), &'static str> {
    let _socket = t::net::socket2::Socket::new(
        t::net::socket2::Domain::IPV4,
        t::net::socket2::Type::STREAM,
        Some(t::net::socket2::Protocol::TCP),
    )
    .map_err(|_| "net.socket2.new")?;
    logl::log(
        level::INFO,
        format_args!("tokio_net: success net.socket2.new"),
    );
    Ok(())
}

fn probe_mio_poll_surface() -> Result<(), &'static str> {
    let mut poll = t::net::mio::Poll::new().map_err(|_| "mio.poll.new")?;
    let mut events = t::net::mio::Events::with_capacity(4);
    let waker = t::net::mio::Waker::new(poll.registry(), t::net::mio::Token(0xB170))
        .map_err(|_| "mio.waker.new")?;
    waker.wake().map_err(|_| "mio.waker.wake")?;
    poll.poll(&mut events, Some(core::time::Duration::ZERO))
        .map_err(|_| "mio.poll.poll")?;
    let event_count = events.iter().count();
    if event_count == 0 {
        return Err("mio.poll.empty");
    }
    logl::log(
        level::INFO,
        format_args!("tokio_net: success mio.poll.wake events={}", event_count),
    );
    Ok(())
}

async fn probe_mio_udp_bind() -> Result<(), &'static str> {
    let deadline = t::time::Instant::now() + t::time::Duration::from_millis(PROBE_WAIT_BUDGET_MS);

    loop {
        match t::net::mio::net::UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0).into()) {
            Ok(socket) => {
                let local = socket.local_addr().map_err(|_| "mio.net.udp.local_addr")?;
                logl::log(
                    level::INFO,
                    format_args!("tokio_net: success mio.net.udp.bind local={}", local),
                );
                return Ok(());
            }
            Err(_) if t::time::Instant::now() < deadline => {
                platform::poll_once();
                t::time::sleep(t::time::Duration::from_millis(PROBE_WAIT_SLICE_MS)).await;
            }
            Err(_) => return Err("mio.net.udp.bind"),
        }
    }
}

async fn probe_udp_bind() -> Result<(), &'static str> {
    let deadline = t::time::Instant::now() + t::time::Duration::from_millis(PROBE_WAIT_BUDGET_MS);

    loop {
        match try_probe_udp_bind().await {
            Ok(()) => return Ok(()),
            Err(stage) if t::time::Instant::now() < deadline => {
                platform::poll_once();
                t::time::sleep(t::time::Duration::from_millis(PROBE_WAIT_SLICE_MS)).await;
                if stage == "net.udp.bind" {
                    continue;
                }
                return Err(stage);
            }
            Err(stage) => return Err(stage),
        }
    }
}

async fn try_probe_udp_bind() -> Result<(), &'static str> {
    let socket = t::net::UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0))
        .await
        .map_err(|_| "net.udp.bind")?;
    let local = socket.local_addr().map_err(|_| "net.udp.local_addr")?;
    socket.writable().await.map_err(|_| "net.udp.writable")?;
    logl::log(
        level::INFO,
        format_args!("tokio_net: success net.udp.bind local={}", local),
    );
    Ok(())
}

async fn probe_udp_loopback() -> Result<(), &'static str> {
    let sender = t::net::UdpSocket::bind((Ipv4Addr::LOCALHOST, 0))
        .await
        .map_err(|_| "net.udp.loopback.sender_bind")?;
    let receiver = t::net::UdpSocket::bind((Ipv4Addr::LOCALHOST, 0))
        .await
        .map_err(|_| "net.udp.loopback.receiver_bind")?;
    let receiver_addr = receiver
        .local_addr()
        .map_err(|_| "net.udp.loopback.receiver_local_addr")?;

    let sent = sender
        .send_to(b"ping", receiver_addr)
        .await
        .map_err(|_| "net.udp.loopback.send_to")?;
    if sent != 4 {
        return Err("net.udp.loopback.send_len");
    }

    let mut buffer = [0u8; 4];
    let (received, peer) = t::time::timeout(
        t::time::Duration::from_millis(PROBE_WAIT_BUDGET_MS),
        receiver.recv_from(&mut buffer),
    )
    .await
    .map_err(|_| "net.udp.loopback.recv_timeout")?
    .map_err(|_| "net.udp.loopback.recv_from")?;
    if received != buffer.len() || buffer != *b"ping" {
        return Err("net.udp.loopback.payload");
    }

    logl::log(
        level::INFO,
        format_args!(
            "tokio_net: success net.udp.loopback_roundtrip peer={} destination={}",
            peer, receiver_addr
        ),
    );
    Ok(())
}

async fn probe_tcp_loopback() -> Result<(), &'static str> {
    use t::io::{AsyncReadExt, AsyncWriteExt};

    let listener = t::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .await
        .map_err(|_| "net.tcp.loopback.bind")?;
    let listen_addr = listener
        .local_addr()
        .map_err(|_| "net.tcp.loopback.local_addr")?;
    logl::log(
        level::INFO,
        format_args!("tokio_net: tcp loopback listen={}", listen_addr),
    );

    let accept_task = t::task::spawn(async move {
        let (mut server, peer) = listener
            .accept()
            .await
            .map_err(|_| "net.tcp.loopback.accept")?;

        let mut buf = [0u8; 4];
        server
            .read_exact(&mut buf)
            .await
            .map_err(|_| "net.tcp.loopback.server_read")?;
        if buf != *b"ping" {
            return Err("net.tcp.loopback.server_value");
        }
        server
            .write_all(b"pong")
            .await
            .map_err(|_| "net.tcp.loopback.server_write")?;
        Ok::<SocketAddr, &'static str>(peer)
    });

    t::task::yield_now().await;

    let mut client = t::net::TcpStream::connect(listen_addr)
        .await
        .map_err(|_| "net.tcp.loopback.connect")?;
    client
        .writable()
        .await
        .map_err(|_| "net.tcp.loopback.client_writable")?;
    client
        .write_all(b"ping")
        .await
        .map_err(|_| "net.tcp.loopback.client_write")?;
    client
        .readable()
        .await
        .map_err(|_| "net.tcp.loopback.client_readable")?;

    let mut buf = [0u8; 4];
    client
        .read_exact(&mut buf)
        .await
        .map_err(|_| "net.tcp.loopback.client_read")?;
    if buf != *b"pong" {
        return Err("net.tcp.loopback.client_value");
    }

    let peer = accept_task
        .await
        .map_err(|_| "net.tcp.loopback.accept_task")??;
    logl::log(
        level::INFO,
        format_args!("tokio_net: success net.tcp.loopback peer={}", peer),
    );
    Ok(())
}

async fn probe_remote_http() -> Result<(), &'static str> {
    use t::io::{AsyncReadExt, AsyncWriteExt};

    let remote_http_addr = remote_http_addr();
    let mut stream = t::net::TcpStream::connect(remote_http_addr)
        .await
        .map_err(|_| "net.tcp.remote.connect")?;
    stream
        .writable()
        .await
        .map_err(|_| "net.tcp.remote.writable")?;

    let request = format!(
        "GET / HTTP/1.0\r\nHost: {}\r\nUser-Agent: trueos-blueprint-tokio-net\r\n\r\n",
        REMOTE_HTTP_HOST
    );
    stream
        .write_all(request.as_bytes())
        .await
        .map_err(|_| "net.tcp.remote.write")?;
    stream
        .readable()
        .await
        .map_err(|_| "net.tcp.remote.readable")?;

    let mut buf = [0u8; 64];
    let read = stream
        .read(&mut buf)
        .await
        .map_err(|_| "net.tcp.remote.read")?;
    if read == 0 {
        return Err("net.tcp.remote.eof");
    }

    let preview = core::str::from_utf8(&buf[..read]).unwrap_or("<non-utf8>");
    logl::log(
        level::INFO,
        format_args!(
            "tokio_net: success net.tcp.remote_http addr={} bytes={} preview={}",
            remote_http_addr, read, preview
        ),
    );
    Ok(())
}

fn remote_http_addr() -> SocketAddr {
    SocketAddr::from(([93, 184, 216, 34], 80))
}
