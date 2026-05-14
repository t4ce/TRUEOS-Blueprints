use core::net::{Ipv4Addr, SocketAddr};

use trueos::vsys;
use trueos::{
    bp_error, bp_info, net,
    platform::{format, thread},
    runtime, task, time, tokio,
};

const PROBE_WAIT_BUDGET_MS: u64 = 1_500;
const PROBE_WAIT_SLICE_MS: u64 = 25;
const REMOTE_HTTP_HOST: &str = "example.com";

fn main() {
    bp_info!("tokio_net: start");

    if let Err(stage) = probe_runtime_bootstrap_surfaces() {
        bp_error!("tokio_net: bootstrap failed stage={}", stage);
        return;
    }

    bp_info!("tokio_net: stage runtime.current_thread_net.build");

    let runtime = match runtime::current_thread_net().build() {
        Ok(rt) => rt,
        Err(err) => {
            bp_error!("tokio_net: runtime build failed: {}", err);
            return;
        }
    };
    bp_info!("tokio_net: success runtime.current_thread_net.build");

    runtime.block_on(async {
        match run_probe().await {
            Ok(()) => bp_info!("tokio_net: done"),
            Err(stage) => bp_error!("tokio_net: failed stage={}", stage),
        }
    });
}

fn probe_runtime_bootstrap_surfaces() -> Result<(), &'static str> {
    bp_info!("tokio_net: stage thread.current.id");
    let thread_id = thread::current().id();
    bp_info!("tokio_net: success thread.current.id id={:?}", thread_id);

    bp_info!("tokio_net: stage thread.yield_now");
    thread::yield_now();
    bp_info!("tokio_net: success thread.yield_now");

    bp_info!("tokio_net: stage runtime.current_thread.builder_new_plain");
    let mut builder = tokio::runtime::Builder::new_current_thread();
    bp_info!("tokio_net: success runtime.current_thread.builder_new_plain");

    bp_info!("tokio_net: stage runtime.current_thread.builder_build_plain");
    let runtime = builder
        .build()
        .map_err(|_| "runtime.current_thread.builder_build_plain")?;
    bp_info!("tokio_net: success runtime.current_thread.build_plain");

    bp_info!("tokio_net: stage runtime.current_thread.drop_plain");
    drop(runtime);
    bp_info!("tokio_net: success runtime.current_thread.drop_plain");

    bp_info!("tokio_net: stage runtime.current_thread.build_time");
    let runtime = runtime::current_thread()
        .build()
        .map_err(|_| "runtime.current_thread.build_time")?;
    bp_info!("tokio_net: success runtime.current_thread.build_time");

    bp_info!("tokio_net: stage runtime.current_thread.drop_time");
    drop(runtime);
    bp_info!("tokio_net: success runtime.current_thread.drop_time");

    bp_info!("tokio_net: stage runtime.current_thread.builder_new_io_no_time");
    let mut builder = tokio::runtime::Builder::new_current_thread();
    bp_info!("tokio_net: success runtime.current_thread.builder_new_io_no_time");

    bp_info!("tokio_net: stage runtime.current_thread.build_io_no_time");
    builder.enable_io();
    let runtime = builder
        .build()
        .map_err(|_| "runtime.current_thread.build_io_no_time")?;
    bp_info!("tokio_net: success runtime.current_thread.build_io_no_time");

    bp_info!("tokio_net: stage runtime.current_thread.drop_io_no_time");
    drop(runtime);
    bp_info!("tokio_net: success runtime.current_thread.drop_io_no_time");

    bp_info!("tokio_net: stage mio.poll.wake.bootstrap");
    probe_mio_poll_surface()?;
    bp_info!("tokio_net: success mio.poll.wake.bootstrap");

    Ok(())
}

async fn run_probe() -> Result<(), &'static str> {
    bp_info!("tokio_net: stage net.socket2.new");
    probe_socket2_surface()?;

    bp_info!("tokio_net: stage mio.poll.wake");
    probe_mio_poll_surface()?;

    bp_info!("tokio_net: stage mio.net.udp.bind");
    probe_mio_udp_bind().await?;

    bp_info!("tokio_net: stage net.udp.bind");
    probe_udp_bind().await?;

    bp_info!("tokio_net: stage net.tcp.loopback_roundtrip");
    match probe_tcp_loopback().await {
        Ok(()) => bp_info!("tokio_net: success net.tcp.loopback_roundtrip"),
        Err(stage) => {
            bp_info!(
                "tokio_net: note net.tcp.loopback unavailable, fallback={}",
                stage
            );
            bp_info!("tokio_net: stage net.tcp.remote_http");
            match probe_remote_http().await {
                Ok(()) => {}
                Err(stage) => {
                    bp_info!(
                        "tokio_net: note net.tcp.remote_http unavailable, fallback={}",
                        stage
                    );
                }
            }
        }
    }

    Ok(())
}

fn probe_socket2_surface() -> Result<(), &'static str> {
    let _socket = net::socket2::Socket::new(
        net::socket2::Domain::IPV4,
        net::socket2::Type::STREAM,
        Some(net::socket2::Protocol::TCP),
    )
    .map_err(|_| "net.socket2.new")?;
    bp_info!("tokio_net: success net.socket2.new");
    Ok(())
}

fn probe_mio_poll_surface() -> Result<(), &'static str> {
    let mut poll = net::mio::Poll::new().map_err(|_| "mio.poll.new")?;
    let mut events = net::mio::Events::with_capacity(4);
    let waker = net::mio::Waker::new(poll.registry(), net::mio::Token(0xB170))
        .map_err(|_| "mio.waker.new")?;
    waker.wake().map_err(|_| "mio.waker.wake")?;
    poll.poll(&mut events, Some(core::time::Duration::ZERO))
        .map_err(|_| "mio.poll.poll")?;
    let event_count = events.iter().count();
    if event_count == 0 {
        return Err("mio.poll.empty");
    }
    bp_info!("tokio_net: success mio.poll.wake events={}", event_count);
    Ok(())
}

async fn probe_mio_udp_bind() -> Result<(), &'static str> {
    let deadline = time::Instant::now() + time::Duration::from_millis(PROBE_WAIT_BUDGET_MS);

    loop {
        match net::mio::net::UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0).into()) {
            Ok(socket) => {
                let local = socket.local_addr().map_err(|_| "mio.net.udp.local_addr")?;
                bp_info!("tokio_net: success mio.net.udp.bind local={}", local);
                return Ok(());
            }
            Err(_) if time::Instant::now() < deadline => {
                vsys::poll_once();
                time::sleep(time::Duration::from_millis(PROBE_WAIT_SLICE_MS)).await;
            }
            Err(_) => return Err("mio.net.udp.bind"),
        }
    }
}

async fn probe_udp_bind() -> Result<(), &'static str> {
    let deadline = time::Instant::now() + time::Duration::from_millis(PROBE_WAIT_BUDGET_MS);

    loop {
        match try_probe_udp_bind().await {
            Ok(()) => return Ok(()),
            Err(stage) if time::Instant::now() < deadline => {
                vsys::poll_once();
                time::sleep(time::Duration::from_millis(PROBE_WAIT_SLICE_MS)).await;
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
    let socket = net::UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0))
        .await
        .map_err(|_| "net.udp.bind")?;
    let local = socket.local_addr().map_err(|_| "net.udp.local_addr")?;
    socket.writable().await.map_err(|_| "net.udp.writable")?;
    bp_info!("tokio_net: success net.udp.bind local={}", local);
    Ok(())
}

async fn probe_tcp_loopback() -> Result<(), &'static str> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let listener = net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .await
        .map_err(|_| "net.tcp.loopback.bind")?;
    let listen_addr = listener
        .local_addr()
        .map_err(|_| "net.tcp.loopback.local_addr")?;
    bp_info!("tokio_net: tcp loopback listen={}", listen_addr);

    let accept_task = task::spawn(async move {
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

    task::yield_now().await;

    let mut client = net::TcpStream::connect(listen_addr)
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
    bp_info!("tokio_net: success net.tcp.loopback peer={}", peer);
    Ok(())
}

async fn probe_remote_http() -> Result<(), &'static str> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let remote_http_addr = remote_http_addr();
    let mut stream = net::TcpStream::connect(remote_http_addr)
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
    bp_info!(
        "tokio_net: success net.tcp.remote_http addr={} bytes={} preview={}",
        remote_http_addr,
        read,
        preview
    );
    Ok(())
}

fn remote_http_addr() -> SocketAddr {
    SocketAddr::from(([93, 184, 216, 34], 80))
}
