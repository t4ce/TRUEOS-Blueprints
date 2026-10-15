use futures_util::{SinkExt, StreamExt};
use std::{sync::Arc, time::Duration};
use tokio::{
    io::AsyncReadExt,
    net::TcpStream,
    sync::{mpsc, oneshot},
    time::{sleep, timeout},
};
use tokio_tungstenite::{
    Connector, MaybeTlsStream, WebSocketStream, client_async_tls_with_config, tungstenite::Message,
};

type Socket = WebSocketStream<MaybeTlsStream<TcpStream>>;
type Result<T = ()> = std::result::Result<T, String>;
const BUDGET: Duration = Duration::from_secs(12);
const CERT: &[u8] = include_bytes!("fixture/ca.der");

fn config(trust: bool) -> Arc<rustls::ClientConfig> {
    let mut roots = rustls::RootCertStore::empty();
    if trust {
        roots
            .add(rustls::pki_types::CertificateDer::from(CERT.to_vec()))
            .unwrap();
    }
    Arc::new(
        rustls::ClientConfig::builder()
            .with_root_certificates(roots)
            .with_no_client_auth(),
    )
}

async fn connect(host: &str, port: u16, secure: bool, path: &str) -> Result<Socket> {
    let tcp = TcpStream::connect(format!("{host}:{port}"))
        .await
        .map_err(|e| format!("TCP connect: {e}"))?;
    let scheme = if secure { "wss" } else { "ws" };
    let url = format!("{scheme}://network-probe.test:{port}{path}");
    let connector = secure.then(|| Connector::Rustls(config(true)));
    let (stream, response) = client_async_tls_with_config(url, tcp, None, connector)
        .await
        .map_err(|e| format!("upgrade/TLS: {e}"))?;
    if response.status().as_u16() != 101 {
        return Err("upgrade was not101".into());
    }
    Ok(stream)
}

async fn message(socket: &mut Socket) -> Result<Message> {
    socket
        .next()
        .await
        .ok_or("unexpected stream EOF")?
        .map_err(|e| format!("frame: {e}"))
}

async fn baseline(host: &str, port: u16, secure: bool) -> Result {
    let mut socket = connect(host, port, secure, "/echo").await?;
    socket
        .send(Message::Text("hello-probe".into()))
        .await
        .map_err(|e| e.to_string())?;
    if message(&mut socket).await?.into_data().as_ref() != b"hello-probe" {
        return Err("text echo mismatch".into());
    }
    socket.close(None).await.map_err(|e| e.to_string())
}

async fn large(host: &str, port: u16, secure: bool) -> Result {
    let mut socket = connect(host, port, secure, "/echo").await?;
    let payload: Vec<u8> = (0..300 * 1024)
        .map(|i| ((i * 19 + i / 251) % 253) as u8)
        .collect();
    socket
        .send(Message::Binary(payload.clone().into()))
        .await
        .map_err(|e| e.to_string())?;
    if message(&mut socket).await?.into_data().as_ref() != payload {
        return Err("300KiB payload mismatch".into());
    }
    socket.close(None).await.map_err(|e| e.to_string())
}

async fn fragmented(host: &str, port: u16) -> Result {
    let mut socket = connect(host, port, true, "/fragment").await?;
    if message(&mut socket).await?.into_data().as_ref() != "fragmented-€-witness".as_bytes() {
        return Err("fragment reassembly mismatch".into());
    }
    socket.close(None).await.map_err(|e| e.to_string())
}

async fn ping(host: &str, port: u16) -> Result {
    let mut socket = connect(host, port, true, "/ping").await?;
    if !matches!(message(&mut socket).await?, Message::Ping(_)) {
        return Err("missing server Ping".into());
    }
    socket.flush().await.map_err(|e| e.to_string())?;
    if message(&mut socket).await?.into_data().as_ref() != b"pong-observed" {
        return Err("server did not observe automatic Pong".into());
    }
    socket.close(None).await.map_err(|e| e.to_string())
}

async fn cancelled_read(host: &str, port: u16) -> Result {
    use tokio::io::AsyncWriteExt;
    let mut tcp = TcpStream::connect(format!("{host}:{port}"))
        .await
        .map_err(|e| e.to_string())?;
    tcp.write_all(b"C").await.map_err(|e| e.to_string())?;
    let mut byte = [0u8; 1];
    if timeout(Duration::from_millis(50), tcp.read(&mut byte))
        .await
        .is_ok()
    {
        return Err("delayed read completed before fixture sent bytes".into());
    }
    let mut bytes = [0u8; 15];
    tcp.read_exact(&mut bytes)
        .await
        .map_err(|e| format!("read after cancellation: {e}"))?;
    if &bytes != b"delayed-witness" {
        return Err("cancelled read consumed bytes".into());
    }
    if tcp.read(&mut byte).await.map_err(|e| e.to_string())? != 0 {
        return Err("missing EOF".into());
    }
    Ok(())
}

async fn close_case(host: &str, port: u16, abrupt: bool) -> Result {
    let mut socket = connect(host, port, true, if abrupt { "/reset" } else { "/close" }).await?;
    let result = socket.next().await;
    if abrupt {
        if matches!(result, Some(Ok(_))) {
            return Err("reset produced successful data".into());
        }
    } else {
        if !matches!(result, Some(Ok(Message::Close(_)))) {
            return Err(format!("missing Close frame: {result:?}"));
        }
        socket.flush().await.map_err(|e| e.to_string())?;
    }
    Ok(())
}

async fn untrusted(host: &str, port: u16) -> Result {
    let tcp = TcpStream::connect(format!("{host}:{port}"))
        .await
        .map_err(|e| e.to_string())?;
    match client_async_tls_with_config(
        format!("wss://network-probe.test:{port}/echo"),
        tcp,
        None,
        Some(Connector::Rustls(config(false))),
    )
    .await
    {
        Err(tokio_tungstenite::tungstenite::Error::Io(error))
            if error.kind() == std::io::ErrorKind::InvalidData
                && error.get_ref().is_some_and(|e| {
                    e.downcast_ref::<rustls::Error>()
                        .is_some_and(|e| matches!(e, rustls::Error::InvalidCertificate(_)))
                }) =>
        {
            Ok(())
        }
        Err(error) => Err(format!("expected typed InvalidCertificate, got {error}")),
        Ok(_) => Err("untrusted TLS server accepted".into()),
    }
}

// The same select/command/oneshot pattern used by Codex's WebSocket pump.
async fn pump_reuse(host: &str, port: u16) -> Result {
    let mut socket = connect(host, port, true, "/echo").await?;
    let (tx, mut rx) = mpsc::channel::<(Message, oneshot::Sender<Result>)>(4);
    let (event_tx, mut events) = mpsc::channel::<Result<Message>>(4);
    let task = tokio::spawn(async move {
        loop {
            tokio::select! {
                command = rx.recv() => {
                    let Some((message,ack)) = command else { break; };
                    let result = socket.send(message).await.map_err(|e| e.to_string());
                    let failed = result.is_err();
                    let _ = ack.send(result);
                    if failed { break; }
                }
                next = socket.next() => {
                    let Some(next) = next else { break; };
                    let result = next.map_err(|e| e.to_string());
                    let done = result.is_err() || matches!(result,Ok(Message::Close(_)));
                    if event_tx.send(result).await.is_err() || done { break; }
                }
            }
        }
    });
    let outcome = async {
        for round in 0..4 {
            if round != 0 {
                // Drop a timed-out receive then wake a pending socket reader
                // through a command on another task, reusing the same TLS stream.
                if timeout(Duration::from_millis(80), events.recv())
                    .await
                    .is_ok()
                {
                    return Err("unexpected idle event".into());
                }
                sleep(Duration::from_millis(350)).await;
            }
            let payload = format!("reuse-witness-{round}");
            let (ack, receive_ack) = oneshot::channel();
            tx.send((Message::Text(payload.clone().into()), ack))
                .await
                .map_err(|e| e.to_string())?;
            receive_ack.await.map_err(|e| e.to_string())??;
            let received = events.recv().await.ok_or("pump ended")??;
            if received.into_data().as_ref() != payload.as_bytes() {
                return Err(format!("pump round{round} mismatch"));
            }
        }
        Ok(())
    }
    .await;
    drop(tx);
    task.abort();
    let _ = task.await;
    outcome
}

async fn concurrent(host: &str, port: u16) -> Result {
    let mut joins = Vec::new();
    for _ in 0..4 {
        let host = host.to_string();
        joins.push(tokio::spawn(async move {
            timeout(BUDGET, baseline(&host, port, true))
                .await
                .map_err(|_| "parallel case timeout".to_string())?
        }));
    }
    let mut errors = Vec::new();
    for join in joins {
        match join.await {
            Ok(Ok(())) => (),
            Ok(Err(e)) => errors.push(e),
            Err(e) => errors.push(e.to_string()),
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("; "))
    }
}

pub async fn run(host: &str, base: u16, mode: &str, passed: &mut usize, failed: &mut usize) {
    macro_rules! case {
        ($name:expr,$future:expr) => {{
            let name = format!("{mode}.{}", $name);
            crate::record("BEGIN", &name, "");
            let start = std::time::Instant::now();
            let result = timeout(BUDGET, $future)
                .await
                .unwrap_or_else(|_| Err("case deadline exceeded".into()));
            crate::report(&name, result, passed, failed);
            crate::record(
                "END",
                &name,
                &format!("elapsed_ms={}", start.elapsed().as_millis()),
            );
        }};
    }
    case!("tcp.cancel_read_resume_eof", cancelled_read(host, base));
    case!("ws.text", baseline(host, base + 1, false));
    case!("wss.text", baseline(host, base + 2, true));
    case!("ws.binary_300k", large(host, base + 1, false));
    case!("wss.binary_300k", large(host, base + 2, true));
    case!("wss.fragmented", fragmented(host, base + 2));
    case!("wss.ping_pong", ping(host, base + 2));
    case!("wss.idle_pump_reuse", pump_reuse(host, base + 2));
    case!("wss.peer_close", close_case(host, base + 2, false));
    case!("wss.peer_reset", close_case(host, base + 2, true));
    case!("wss.untrusted_rejected", untrusted(host, base + 2));
    case!("wss.parallel_four", concurrent(host, base + 2));
}
