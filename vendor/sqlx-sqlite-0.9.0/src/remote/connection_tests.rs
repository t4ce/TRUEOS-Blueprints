//! Real TCP/HTTP tests of the production setup helpers and upgraded RPC actor.
use super::*;
use tokio::net::TcpListener;

#[derive(Clone, Copy)]
enum Mode {
    HeaderStall,
    Health { failures: usize, status: u16 },
    Upgrade { header_delay: Duration, open_delay: Duration, stall_open: bool },
}

async fn server(mode: Mode) -> (String, Arc<AtomicUsize>, tokio::task::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let accepted = Arc::new(AtomicUsize::new(0));
    let count = accepted.clone();
    let task = tokio::spawn(async move {
        loop {
            let (mut stream, _) = listener.accept().await.unwrap();
            let ordinal = count.fetch_add(1, Ordering::Relaxed);
            tokio::spawn(async move {
                let mut header = Vec::new();
                while !header.ends_with(b"\r\n\r\n") {
                    let byte = stream.read_u8().await.unwrap();
                    header.push(byte);
                    assert!(header.len() < 4096);
                }
                match mode {
                    Mode::HeaderStall => std::future::pending::<()>().await,
                    Mode::Health { failures, status } => {
                        let code = if ordinal < failures { status } else { 200 };
                        stream.write_all(format!("HTTP/1.1 {code} Fixture\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").as_bytes()).await.unwrap();
                    }
                    Mode::Upgrade { header_delay, open_delay, stall_open } => {
                        tokio::time::sleep(header_delay).await;
                        if stream.write_all(b"HTTP/1.1 101 Switching Protocols\r\nConnection: upgrade\r\nUpgrade: trueos-sqlite-v1\r\n\r\n").await.is_err() { return; }
                        let mut first = true;
                        loop {
                            let Ok(len) = stream.read_u32().await else { return; };
                            assert!(len < 4096);
                            let mut request = vec![0; len as usize];
                            stream.read_exact(&mut request).await.unwrap();
                            let command: serde_json::Value = serde_json::from_slice(&request).unwrap();
                            assert!(command["connection_id"].as_u64().is_some());
                            assert!(command["request_id"].as_u64().is_some());
                            if first {
                                assert_eq!(command["op"], "open");
                                if stall_open { std::future::pending::<()>().await; }
                                tokio::time::sleep(open_delay).await;
                                first = false;
                            }
                            let response = br#"{"results":[],"parameters":0,"declared_types":[],"depth":0,"error":null}"#;
                            if stream.write_u32(response.len() as u32).await.is_err() { return; }
                            if stream.write_all(response).await.is_err() { return; }
                            if command["op"] == "close" { return; }
                        }
                    }
                }
            });
        }
    });
    (endpoint, accepted, task)
}

fn run(test: impl std::future::Future<Output = ()>) {
    tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap().block_on(async {
        tokio::time::timeout(Duration::from_secs(5), test).await.expect("test exceeded outer deadline");
    });
}

#[test]
fn accepted_connection_stalling_upgrade_headers_obeys_setup_deadline() { run(async {
    let (endpoint, accepted, task) = server(Mode::HeaderStall).await;
    let started = Instant::now();
    let result = SqliteConnection::establish_until(&SqliteConnectOptions::new().in_memory(true), endpoint, started + Duration::from_millis(90)).await;
    assert!(result.is_err());
    assert_eq!(accepted.load(Ordering::Relaxed), 1);
    assert!(started.elapsed() >= Duration::from_millis(70));
    assert!(started.elapsed() < Duration::from_millis(500));
    task.abort();
}); }

#[test]
fn open_reply_stall_is_included_in_setup_deadline() { run(async {
    let (endpoint, _, task) = server(Mode::Upgrade { header_delay: Duration::ZERO, open_delay: Duration::ZERO, stall_open: true }).await;
    let started = Instant::now();
    let result = SqliteConnection::establish_until(&SqliteConnectOptions::new().in_memory(true), endpoint, started + Duration::from_millis(90)).await;
    assert!(result.is_err());
    assert!(started.elapsed() >= Duration::from_millis(70));
    assert!(started.elapsed() < Duration::from_millis(500));
    task.abort();
}); }

#[test]
fn setup_budget_is_shared_across_headers_and_open_not_reset_per_phase() { run(async {
    let (endpoint, _, task) = server(Mode::Upgrade { header_delay: Duration::from_millis(65), open_delay: Duration::from_millis(90), stall_open: false }).await;
    let started = Instant::now();
    let result = SqliteConnection::establish_until(&SqliteConnectOptions::new().in_memory(true), endpoint, started + Duration::from_millis(120)).await;
    assert!(result.is_err());
    assert!(started.elapsed() >= Duration::from_millis(100));
    assert!(started.elapsed() < Duration::from_millis(200));
    task.abort();
}); }

#[test]
fn non_success_health_status_is_never_ready() { run(async {
    for status in [404, 503] {
        let (endpoint, accepted, task) = server(Mode::Health { failures: usize::MAX, status }).await;
        let client = reqwest::Client::builder().no_proxy().build().unwrap();
        assert!(!health_ready(&client, &endpoint, Instant::now() + Duration::from_secs(1)).await);
        assert_eq!(accepted.load(Ordering::Relaxed), 1);
        task.abort();
    }
}); }

#[test]
fn health_retries_until_successful_status() { run(async {
    let (endpoint, accepted, task) = server(Mode::Health { failures: 2, status: 503 }).await;
    let client = reqwest::Client::builder().no_proxy().build().unwrap();
    let started = Instant::now();
    wait_for_service(&client, &endpoint, started + Duration::from_secs(2)).await.unwrap();
    assert_eq!(accepted.load(Ordering::Relaxed), 3);
    assert!(started.elapsed() >= Duration::from_millis(590));
    assert!(started.elapsed() < Duration::from_millis(1500));
    task.abort();
}); }

#[test]
fn stalled_health_headers_and_backoff_cannot_exceed_shared_deadline() { run(async {
    let (endpoint, _, task) = server(Mode::HeaderStall).await;
    let client = reqwest::Client::builder().no_proxy().build().unwrap();
    let started = Instant::now();
    assert!(!health_ready(&client, &endpoint, started + Duration::from_secs(2)).await);
    assert!(started.elapsed() >= Duration::from_millis(700));
    assert!(started.elapsed() < Duration::from_millis(1500), "health probe must not consume the whole setup budget");
    task.abort();
    for mode in [Mode::HeaderStall, Mode::Health { failures: usize::MAX, status: 503 }] {
        let (endpoint, _, task) = server(mode).await;
        let client = reqwest::Client::builder().no_proxy().build().unwrap();
        let started = Instant::now();
        assert!(wait_for_service(&client, &endpoint, started + Duration::from_millis(90)).await.is_err());
        assert!(started.elapsed() >= Duration::from_millis(70));
        assert!(started.elapsed() < Duration::from_millis(500));
        task.abort();
    }
}); }

#[test]
fn upgraded_rpc_survives_past_setup_deadline_and_remains_persistent() { run(async {
    let (endpoint, accepted, task) = server(Mode::Upgrade { header_delay: Duration::ZERO, open_delay: Duration::ZERO, stall_open: false }).await;
    let mut connection = SqliteConnection::establish_until(&SqliteConnectOptions::new().in_memory(true), endpoint, Instant::now() + Duration::from_millis(100)).await.unwrap();
    tokio::time::sleep(Duration::from_millis(150)).await;
    connection.request(Command::Ping).await.unwrap();
    connection.request(Command::Ping).await.unwrap();
    connection.request(Command::Close).await.unwrap();
    assert_eq!(accepted.load(Ordering::Relaxed), 1);
    task.abort();
}); }

#[test]
fn operation_labels_are_static_and_do_not_contain_payloads() {
    for (command, expected) in [
        (Command::Open { database: "private database".into(), create: true, read_only: false }, "open"),
        (Command::Execute { sql: "private SQL".into(), params: vec![], timeout_ms: 5000 }, "execute"),
        (Command::Prepare { sql: "private SQL".into() }, "prepare"),
        (Command::Begin { sql: Some("private SQL".into()) }, "begin"),
        (Command::Commit, "commit"),
        (Command::Rollback, "rollback"),
        (Command::Ping, "ping"),
        (Command::Close, "close"),
    ] {
        assert_eq!(command.operation(), expected);
    }
}

#[test]
fn truncated_reply_body_fails_without_waiting_for_deadline() { run(async {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let peer = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut headers = Vec::new();
        while !headers.ends_with(b"\r\n\r\n") { headers.push(stream.read_u8().await.unwrap()); }
        stream.write_all(b"HTTP/1.1 101 Switching Protocols\r\nConnection: upgrade\r\nUpgrade: trueos-sqlite-v1\r\n\r\n").await.unwrap();
        let len = stream.read_u32().await.unwrap() as usize;
        assert!(len < 4096);
        let mut request = vec![0; len];
        stream.read_exact(&mut request).await.unwrap();
        let mut partial = 100u32.to_be_bytes().to_vec();
        partial.push(b'{');
        stream.write_all(&partial).await.unwrap();
    });
    let started = Instant::now();
    assert!(SqliteConnection::establish_until(&SqliteConnectOptions::new().in_memory(true), endpoint, started + Duration::from_secs(2)).await.is_err());
    assert!(started.elapsed() < Duration::from_millis(500));
    peer.await.unwrap();
}); }
