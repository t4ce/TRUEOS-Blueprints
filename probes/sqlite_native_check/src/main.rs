//! Finite, loopback-only test client for one uniquely named SQLite test run.
#![no_std]
#![no_main]

extern crate alloc;
use alloc::{
    format,
    string::{String, ToString},
    vec::Vec,
};
use core::{
    net::{Ipv4Addr, SocketAddr},
    time::Duration,
};
use serde_json::{Value, json};
use trueos_native::{
    net::TcpStream,
    task::JoinSet,
    tokio::io::{AsyncReadExt, AsyncWriteExt},
};

#[global_allocator]
static ALLOCATOR: trueos_native::TrueosAllocator = trueos_native::TrueosAllocator;

#[panic_handler]
fn panic(_: &core::panic::PanicInfo<'_>) -> ! {
    trueos_native::panic_abort("sqlite-native-check: PANIC\n")
}

#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
    let result = (|| {
        let settings = settings()?;
        let runtime = trueos_native::runtime::current_thread()
            .enable_io()
            .build()
            .map_err(|error| error.to_string())?;
        runtime.block_on(async {
            trueos_native::time::timeout(Duration::from_secs(90), run(&settings))
                .await
                .map_err(|_| String::from("whole probe deadline"))?
        })
    })();
    if let Err(error) = result {
        trueos_native::logl::log(
            trueos_native::logl::level::ERROR,
            format_args!("sqlite-native-check: FAIL {error}"),
        );
    }
    trueos_native::entry::finish()
}

struct Settings {
    port: u16,
    run: String,
    verify: bool,
}

fn settings() -> Result<Settings, String> {
    let bytes =
        trueos_native::async_fs::block_on(trueos_native::async_fs::read_file(b"vFile:launch"))
            .map_err(|error| format!("launch script unavailable {error}"))?;
    let script = core::str::from_utf8(&bytes).map_err(|_| "launch UTF-8")?;
    let mut port = None;
    let mut run = None;
    let mut verify = false;
    for line in script.lines() {
        let mut words = line.split_whitespace();
        let Some(key) = words.next() else { continue };
        let value = words.next().ok_or("launch option has no value")?;
        if words.next().is_some() {
            return Err("extra launch words".into());
        }
        match key {
            "port" => port = Some(value.parse::<u16>().map_err(|_| "invalid port")?),
            "run" => {
                if value.is_empty()
                    || value.len() > 48
                    || !value
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
                {
                    return Err("invalid run name".into());
                }
                run = Some(value.to_string());
            }
            "phase" => {
                verify = match value {
                    "seed" => false,
                    "verify" => true,
                    _ => return Err("phase must be seed or verify".into()),
                }
            }
            _ => return Err("unknown launch option".into()),
        }
    }
    let port = port
        .filter(|port| *port > 0)
        .ok_or("explicit port required")?;
    Ok(Settings {
        port,
        run: run.ok_or("unique run name required")?,
        verify,
    })
}

fn checkpoint(settings: &Settings, name: &str) {
    trueos_native::logl::log(
        trueos_native::logl::level::INFO,
        format_args!(
            "sqlite-native-check: PASS run={} phase={} check={name}",
            settings.run,
            if settings.verify { "verify" } else { "seed" }
        ),
    );
}

fn require(condition: bool, message: &str) -> Result<(), String> {
    if condition {
        Ok(())
    } else {
        Err(message.into())
    }
}

async fn post(port: u16, body: Value, expected: u16) -> Result<Value, String> {
    trueos_native::time::timeout(Duration::from_secs(15), async {
        let mut stream = TcpStream::connect(SocketAddr::from((Ipv4Addr::LOCALHOST, port)))
            .await.map_err(|error| format!("connect {error}"))?;
        let body = serde_json::to_vec(&body).map_err(|error| error.to_string())?;
        let header = format!("POST /v1/batch HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n", body.len());
        stream.write_all(header.as_bytes()).await.map_err(|error| error.to_string())?;
        stream.write_all(&body).await.map_err(|error| error.to_string())?;
        let mut response = Vec::new();
        loop {
            let mut buffer = [0u8; 4096];
            let count = stream.read(&mut buffer).await.map_err(|error| error.to_string())?;
            if count == 0 { break; }
            response.extend_from_slice(&buffer[..count]);
            require(response.len() <= 1024 * 1024, "response limit")?;
        }
        let split = response.windows(4).position(|p| p == b"\r\n\r\n")
            .ok_or("response has no HTTP header")?;
        let header = core::str::from_utf8(&response[..split]).map_err(|_| "response header UTF-8")?;
        let status = header.lines().next().and_then(|line| line.split_whitespace().nth(1))
            .and_then(|value| value.parse::<u16>().ok()).ok_or("response status")?;
        require(status == expected, &format!("expected HTTP {expected}, got {status}"))?;
        // The service's finite response has Content-Length, without chunking.
        let length = header.lines().skip(1).find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse::<usize>().ok()).flatten()
        }).ok_or("response has no content length")?;
        let body = &response[split + 4..];
        require(body.len() == length, "response content length mismatch")?;
        serde_json::from_slice(body).map_err(|error| error.to_string())
    }).await.map_err(|_| String::from("HTTP request deadline"))?
}

fn statements(database: &str, sql: &[&str]) -> Value {
    json!({"database": database, "statements": sql.iter().map(|sql| json!({"sql":sql})).collect::<Vec<_>>()})
}

async fn query(settings: &Settings, sql: &str) -> Result<Value, String> {
    post(settings.port, statements(&settings.run, &[sql]), 200).await
}

async fn verify(settings: &Settings) -> Result<(), String> {
    let result = query(settings, "SELECT value FROM counter WHERE id=1").await?;
    require(
        result["results"][0]["rows"][0][0]["value"] == 8,
        "persisted counter mismatch",
    )?;
    let result = query(settings, "SELECT n,t,hex(b),z FROM typed").await?;
    let row = &result["results"][0]["rows"][0];
    require(
        row[0]["value"] == -9223372036854775807i64
            && row[1]["value"] == "SQLite λ café"
            && row[2]["value"] == "00FF07"
            && row[3]["type"] == "null",
        "persisted typed values mismatch",
    )?;
    let result = query(settings, "PRAGMA integrity_check").await?;
    require(
        result["results"][0]["rows"][0][0]["value"] == "ok",
        "integrity check failed",
    )?;
    checkpoint(settings, "persisted_typed_counter_integrity");
    Ok(())
}

async fn run(settings: &Settings) -> Result<(), String> {
    let result = query(settings,
        "SELECT sqlite_compileoption_used('THREADSAFE=1'),sqlite_compileoption_used('TEMP_STORE=3'),sqlite_compileoption_used('OMIT_LOAD_EXTENSION'),sqlite_compileoption_used('OMIT_LOCALTIME')").await?;
    let row = result["results"][0]["rows"][0]
        .as_array()
        .ok_or("native compile options response")?;
    require(
        row.len() == 4 && row.iter().all(|value| value["value"] == 1),
        "service is not the native SQLite engine",
    )?;
    checkpoint(settings, "native_engine_options");
    if settings.verify {
        verify(settings).await?;
        checkpoint(settings, "COMPLETE");
        return Ok(());
    }
    // CREATE without IF NOT EXISTS makes accidental test-name reuse fail.
    post(
        settings.port,
        statements(
            &settings.run,
            &[
                "CREATE TABLE counter(id INTEGER PRIMARY KEY,value INTEGER NOT NULL)",
                "INSERT INTO counter VALUES(1,0)",
                "CREATE TABLE typed(n INTEGER,t TEXT,b BLOB,z TEXT)",
                "INSERT INTO typed VALUES(-9223372036854775807,'SQLite λ café',X'00FF07',NULL)",
            ],
        ),
        200,
    )
    .await?;
    checkpoint(settings, "create_insert");
    post(
        settings.port,
        statements(
            &settings.run,
            &[
                "UPDATE counter SET value=999 WHERE id=1",
                "INSERT INTO nonexistent VALUES(1)",
            ],
        ),
        400,
    )
    .await?;
    let result = query(settings, "SELECT value FROM counter WHERE id=1").await?;
    require(
        result["results"][0]["rows"][0][0]["value"] == 0,
        "failed batch did not roll back",
    )?;
    checkpoint(settings, "failed_batch_rollback");
    post(
        settings.port,
        statements(
            &settings.run,
            &[
                "UPDATE counter SET value=999 WHERE id=1",
                "ATTACH DATABASE ':memory:' AS outside",
            ],
        ),
        400,
    )
    .await?;
    let result = query(settings, "SELECT value FROM counter WHERE id=1").await?;
    require(
        result["results"][0]["rows"][0][0]["value"] == 0,
        "ATTACH did not roll back",
    )?;
    checkpoint(settings, "attach_isolation");
    let mut jobs = JoinSet::new();
    for _ in 0..8 {
        let port = settings.port;
        let body = statements(
            &settings.run,
            &["UPDATE counter SET value=value+1 WHERE id=1"],
        );
        jobs.spawn(async move { post(port, body, 200).await });
    }
    while let Some(result) = jobs.join_next().await {
        result.map_err(|_| "concurrent request task failed")??;
    }
    verify(settings).await?;
    checkpoint(settings, "eight_concurrent_updates");
    let result = query(settings,
        "SELECT CAST(strftime('%s','now') AS INTEGER),hex(randomblob(16)),hex(randomblob(16)),datetime('2000-01-01','localtime')").await?;
    let row = &result["results"][0]["rows"][0];
    let utc = row[0]["value"].as_i64().ok_or("UTC clock missing")?;
    require(
        (1_700_000_000..2_000_000_000).contains(&utc),
        "UTC clock outside expected era",
    )?;
    let first = row[1]["value"].as_str().ok_or("first random blob")?;
    let second = row[2]["value"].as_str().ok_or("second random blob")?;
    require(
        first.len() == 32
            && second.len() == 32
            && first != second
            && first != "00000000000000000000000000000000",
        "entropy mismatch",
    )?;
    require(
        row[3]["type"] == "null",
        "unsupported localtime must be NULL",
    )?;
    checkpoint(settings, "native_utc_entropy");
    post(settings.port, statements(&settings.run, &[
        "PRAGMA temp_store=FILE",
        "CREATE TEMP TABLE work(k INTEGER,payload TEXT)",
        "WITH RECURSIVE n(x) AS (VALUES(1) UNION ALL SELECT x+1 FROM n WHERE x<1024) INSERT INTO work SELECT x,printf('%01024d',x) FROM n",
        "SELECT group_concat(k,',') FROM (SELECT k FROM work ORDER BY payload DESC)",
        "DROP TABLE work",
    ]), 200).await.and_then(|result| {
        let text = result["results"][3]["rows"][0][0]["value"].as_str().ok_or("sort result")?;
        let ordered = text.split(',').map(|number| number.parse::<u32>()).collect::<Result<Vec<_>,_>>()
            .map_err(|_| "sort number")?;
        require(ordered == (1..=1024).rev().collect::<Vec<_>>(), "temp sort mismatch")
    })?;
    checkpoint(settings, "temp_sort_memory");
    checkpoint(settings, "COMPLETE");
    Ok(())
}
