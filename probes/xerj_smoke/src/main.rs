//! Native, finite XERJ integration check. See ../BUILDING.txt for its contract.

use serde_json::{Value, json};
use std::collections::VecDeque;
use std::fs;
use std::io::{self, Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const POLL: Duration = Duration::from_millis(10);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(20);
const READY_TIMEOUT: Duration = Duration::from_secs(90);
const APP_SERVER_TIMEOUT: Duration = Duration::from_secs(150);
const STOP_TIMEOUT: Duration = Duration::from_secs(20);
const CLI_TIMEOUT: Duration = Duration::from_secs(120);
const MAX_WIRE: usize = 2 * 1024 * 1024;
const INDEX: &str = "trueos-smoke-docs";
const NAMESPACE: &str = "trueos-smoke";
const DOC_ID: &str = "native-document";
const HTTP_MEMORY: &str = "http-memory";
const MCP_MEMORY: &str = "mcp-memory";
const TOKEN: &str = "trueosquartz";
const TEXT: &str = "trueosquartz verifies native Rust search and durable agent memory";
const CORPUS_PREFIX: &str = "trueos-smoke-corpus";
const SOURCE_FILE: &str = "quartz_fixture.rs";
const SOURCE_SYMBOL: &str = "trueos_smoke_quartz_calibration";
const SOURCE_LINE: u64 = 2;
const SOURCE_DEFINITION: &str = "pub fn trueos_smoke_quartz_calibration(value: u64) -> u64";
const SOURCE_TEXT: &str = "// Native AST and source-location fixture.\npub fn trueos_smoke_quartz_calibration(value: u64) -> u64 {\n    value + 37\n}\n";
const PROSE_FILE: &str = "native_notes.md";
const PROSE_TOKEN: &str = "trueosamberdocs";
const PROSE_TEXT: &str = "# Native autoindex fixture\n\ntrueosamberdocs verifies Markdown discovery beside a Rust source fixture.\n";
const SQLITE_FILE: &str = "quartz_fixture.sqlite3";
const SQLITE_TOKEN: &str = "trueossqlitequartz";
const SQLITE_TEXT: &str = "trueossqlitequartz verifies native SQLite source retrieval";
const SQLITE_BYTES: &[u8] = include_bytes!("../fixtures/quartz_fixture.sqlite3");
const FIXTURE_AUTH: &str = "ApiKey trueos-smoke-fixture";

#[cfg(target_os = "trueos")]
use trueos::child_hull as child;

// Host checks validate framing and assertions. Running the integration probe
// requires actual Hull children; there is no host-process substitute.
#[cfg(not(target_os = "trueos"))]
mod child {
    pub const ERR_QUEUE_FULL: i32 = -3;
    pub const ERR_NOT_FOUND: i32 = -2;
    pub const ERR_UNAVAILABLE: i32 = -4;
    pub const STATUS_STARTING: i32 = 1;
    pub const STATUS_RUNNING: i32 = 2;
    pub const STATUS_STOPPING: i32 = 3;
    pub const STATUS_EXITED: i32 = 4;
    pub fn spawn_program(_: &[u8]) -> Result<u64, i32> {
        Err(-4)
    }
    pub fn read(_: u64, _: &mut [u8]) -> Result<usize, i32> {
        Err(-4)
    }
    pub fn send(_: u64, _: &[u8]) -> Result<usize, i32> {
        Err(-4)
    }
    pub fn status(_: u64) -> i32 {
        -4
    }
    pub fn terminate(_: u64) -> Result<(), i32> {
        Err(-4)
    }
}

fn error(message: impl Into<String>) -> io::Error {
    io::Error::other(message.into())
}

fn require(condition: bool, message: impl Into<String>) -> io::Result<()> {
    if condition {
        Ok(())
    } else {
        Err(error(message))
    }
}

fn mark(stage: &str) {
    marker(format!("xerj-smoke v=1 stage={stage} ok=true"));
}

fn marker(line: String) {
    println!("{line}");
    let _ = io::stdout().flush();
    #[cfg(target_os = "trueos")]
    let _ = trueos::logl::log_record(trueos::logl::level::IMPORTANT, "xerj-smoke", line.as_str());
}

struct OwnedChild {
    handle: Option<u64>,
    label: &'static str,
    tail: VecDeque<u8>,
    pending: Vec<u8>,
}

impl OwnedChild {
    // Call only from run(), on the Blueprint entry thread. The child Hull
    // launch ABI requires that thread's guest context.
    fn spawn(program: &str, args: &[String], cwd: &Path, label: &'static str) -> io::Result<Self> {
        Self::spawn_with_env(program, args, cwd, label, &[])
    }

    fn spawn_with_env(
        program: &str,
        args: &[String],
        cwd: &Path,
        label: &'static str,
        extra_env: &[(&str, String)],
    ) -> io::Result<Self> {
        let request = Self::launch_request(program, args, cwd, extra_env)?;
        let handle = child::spawn_program(&request)
            .map_err(|code| error(format!("cannot launch {label} {program}: child ABI {code}")))?;
        println!("xerj-smoke v=1 stage=spawn child={label} handle={handle}");
        Ok(Self {
            handle: Some(handle),
            label,
            tail: VecDeque::new(),
            pending: Vec::new(),
        })
    }

    fn launch_request(
        program: &str,
        args: &[String],
        cwd: &Path,
        extra_env: &[(&str, String)],
    ) -> io::Result<Vec<u8>> {
        let mut request = json!({
            "program": program,
            "args": args,
            "cwd": cwd.to_string_lossy(),
            "env": {
                "XERJ_DISABLE_FEEDBACK": "true",
                "XERJ_AUTH": FIXTURE_AUTH,
                "NO_PROXY": "127.0.0.1,localhost",
                "no_proxy": "127.0.0.1,localhost",
                "RUST_LOG": "warn"
            }
        });
        for (key, value) in extra_env {
            request["env"][key] = json!(value);
        }
        serde_json::to_vec(&request).map_err(io::Error::other)
    }

    fn drain(&mut self, capture_rpc: bool) -> io::Result<()> {
        let Some(handle) = self.handle else {
            return Ok(());
        };
        let mut buffer = [0u8; 4096];
        for _ in 0..32 {
            let size = child::read(handle, &mut buffer)
                .map_err(|code| error(format!("{} stdout read failed: {code}", self.label)))?;
            if size == 0 {
                break;
            }
            self.tail.extend(&buffer[..size]);
            while self.tail.len() > 8192 {
                self.tail.pop_front();
            }
            if capture_rpc {
                require(
                    self.pending.len() + size <= MAX_WIRE,
                    format!("{} stdout exceeds 2 MiB", self.label),
                )?;
                self.pending.extend_from_slice(&buffer[..size]);
            }
        }
        Ok(())
    }

    fn running(&mut self) -> io::Result<()> {
        self.drain(false)?;
        match self.handle.map(child::status) {
            Some(child::STATUS_STARTING | child::STATUS_RUNNING) => Ok(()),
            status => Err(error(format!(
                "{} stopped unexpectedly: {status:?}; stdout tail: {}",
                self.label,
                self.tail_text()
            ))),
        }
    }

    fn tail_text(&self) -> String {
        String::from_utf8_lossy(&self.tail.iter().copied().collect::<Vec<_>>()).into_owned()
    }

    fn send(&mut self, message: &Value) -> io::Result<()> {
        let handle = self
            .handle
            .ok_or_else(|| error("MCP child is already stopped"))?;
        let mut bytes = serde_json::to_vec(message)?;
        bytes.push(b'\n');
        let started = Instant::now();
        loop {
            match child::send(handle, &bytes) {
                Ok(size) if size == bytes.len() => return Ok(()),
                Ok(size) => {
                    return Err(error(format!(
                        "partial MCP input frame: {size}/{}",
                        bytes.len()
                    )));
                }
                Err(child::ERR_QUEUE_FULL) if started.elapsed() < REQUEST_TIMEOUT => {
                    self.drain(true)?;
                    thread::sleep(POLL);
                }
                Err(code) => return Err(error(format!("MCP input send failed: {code}"))),
            }
        }
    }

    fn stop(&mut self) -> io::Result<()> {
        let Some(handle) = self.handle else {
            return Ok(());
        };
        match child::terminate(handle) {
            Ok(()) | Err(child::ERR_NOT_FOUND) => {}
            Err(code) => return Err(error(format!("cannot stop {}: {code}", self.label))),
        }
        let started = Instant::now();
        loop {
            // Teardown can remove the transport before the final poll.
            let _ = self.drain(false);
            match child::status(handle) {
                child::STATUS_EXITED | child::ERR_NOT_FOUND => {
                    self.handle = None;
                    return Ok(());
                }
                child::STATUS_STARTING | child::STATUS_RUNNING | child::STATUS_STOPPING => {}
                code => return Err(error(format!("lost {} during cleanup: {code}", self.label))),
            }
            if started.elapsed() >= STOP_TIMEOUT {
                return Err(error(format!(
                    "{} did not terminate within 20 seconds",
                    self.label
                )));
            }
            thread::sleep(POLL);
        }
    }
}

impl Drop for OwnedChild {
    fn drop(&mut self) {
        if let Err(failure) = self.stop() {
            println!(
                "xerj-smoke v=1 stage=cleanup ok=false child={} error={}",
                self.label,
                json!(failure.to_string())
            );
        }
    }
}

fn address(port: u16) -> SocketAddr {
    SocketAddr::from((Ipv4Addr::LOCALHOST, port))
}

fn reserve_ports(seed: u128) -> io::Result<(u16, Vec<TcpListener>)> {
    for offset in 0..128u128 {
        let base = 24000 + ((seed + offset * 3) % 24000) as u16;
        let listeners = (base..base + 3)
            .map(|port| TcpListener::bind(address(port)))
            .collect::<io::Result<Vec<_>>>();
        if let Ok(listeners) = listeners {
            return Ok((base, listeners));
        }
    }
    Err(error(
        "cannot reserve three unused consecutive loopback ports",
    ))
}

fn fresh_directory(root: &Path, seed: u128) -> io::Result<PathBuf> {
    for offset in 0..32 {
        let path = root.join(format!("xerj-smoke-{}", seed + offset));
        match fs::create_dir(&path) {
            Ok(()) => return Ok(path),
            Err(failure) if failure.kind() == io::ErrorKind::AlreadyExists => {}
            Err(failure) => return Err(failure),
        }
    }
    Err(error("cannot create a fresh isolated XERJ smoke directory"))
}

struct HttpReply {
    status: u16,
    body: Value,
}

fn parse_http(bytes: &[u8]) -> io::Result<HttpReply> {
    let split = bytes
        .windows(4)
        .position(|part| part == b"\r\n\r\n")
        .ok_or_else(|| error("HTTP response has no complete header"))?;
    let header = std::str::from_utf8(&bytes[..split]).map_err(io::Error::other)?;
    let status = header
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|value| value.parse::<u16>().ok())
        .ok_or_else(|| error("HTTP response has no status code"))?;
    let mut body = bytes[split + 4..].to_vec();
    let mut length = None;
    let mut chunked = false;
    for line in header.lines().skip(1) {
        if let Some((key, value)) = line.split_once(':') {
            if key.eq_ignore_ascii_case("content-length") {
                length = Some(value.trim().parse::<usize>().map_err(io::Error::other)?);
            }
            if key.eq_ignore_ascii_case("transfer-encoding") {
                chunked = value
                    .split(',')
                    .any(|part| part.trim().eq_ignore_ascii_case("chunked"));
            }
        }
    }
    if chunked {
        body = decode_chunks(&body)?;
    } else if let Some(length) = length {
        require(
            body.len() == length,
            format!("HTTP body length is {}, expected {length}", body.len()),
        )?;
    }
    // XERJ's readiness endpoint is plain text; data endpoints return JSON.
    let body = if body.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&body)
            .unwrap_or_else(|_| Value::String(String::from_utf8_lossy(&body).into_owned()))
    };
    Ok(HttpReply { status, body })
}

fn decode_chunks(mut bytes: &[u8]) -> io::Result<Vec<u8>> {
    let mut decoded = Vec::new();
    loop {
        let split = bytes
            .windows(2)
            .position(|part| part == b"\r\n")
            .ok_or_else(|| error("truncated HTTP chunk header"))?;
        let size = std::str::from_utf8(&bytes[..split])
            .map_err(io::Error::other)?
            .split(';')
            .next()
            .unwrap_or("")
            .trim();
        let size = usize::from_str_radix(size, 16).map_err(io::Error::other)?;
        bytes = &bytes[split + 2..];
        if size == 0 {
            require(
                bytes.starts_with(b"\r\n") || bytes.windows(4).any(|part| part == b"\r\n\r\n"),
                "truncated HTTP final chunk",
            )?;
            return Ok(decoded);
        }
        require(
            size <= bytes.len().saturating_sub(2),
            "truncated HTTP chunk body",
        )?;
        require(
            &bytes[size..size + 2] == b"\r\n",
            "invalid HTTP chunk terminator",
        )?;
        decoded.extend_from_slice(&bytes[..size]);
        bytes = &bytes[size + 2..];
    }
}

fn http(
    engine: &mut OwnedChild,
    port: u16,
    method: &str,
    path: &str,
    body: Option<Value>,
    timeout: Duration,
) -> io::Result<HttpReply> {
    require(!timeout.is_zero(), "HTTP deadline expired")?;
    let started = Instant::now();
    engine.running()?;
    let mut stream =
        TcpStream::connect_timeout(&address(port), timeout.min(Duration::from_millis(250)))?;
    stream.set_nonblocking(true)?;
    let body = body.map(|value| value.to_string()).unwrap_or_default();
    let request = format!(
        "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    );
    let mut sent = 0;
    let mut bytes = Vec::new();
    let mut buffer = [0u8; 8192];
    loop {
        engine.running()?;
        if sent < request.len() {
            match stream.write(&request.as_bytes()[sent..]) {
                Ok(0) => return Err(error("HTTP socket closed during request write")),
                Ok(size) => sent += size,
                Err(failure) if failure.kind() == io::ErrorKind::WouldBlock => {}
                Err(failure) => return Err(failure),
            }
        } else {
            match stream.read(&mut buffer) {
                Ok(0) => return parse_http(&bytes),
                Ok(size) => {
                    require(
                        bytes.len() + size <= MAX_WIRE,
                        "HTTP response exceeds 2 MiB",
                    )?;
                    bytes.extend_from_slice(&buffer[..size]);
                }
                Err(failure) if failure.kind() == io::ErrorKind::WouldBlock => {}
                Err(failure) => return Err(failure),
            }
        }
        require(
            started.elapsed() < timeout,
            format!("HTTP {method} {path} exceeded {} ms", timeout.as_millis()),
        )?;
        thread::sleep(POLL);
    }
}

fn http_json(
    engine: &mut OwnedChild,
    port: u16,
    method: &str,
    path: &str,
    body: Option<Value>,
) -> io::Result<Value> {
    let response = http(engine, port, method, path, body, REQUEST_TIMEOUT)?;
    require(
        (200..300).contains(&response.status),
        format!(
            "HTTP {method} {path} returned {}: {}",
            response.status, response.body
        ),
    )?;
    require(
        response.body.is_object() || response.body.is_array(),
        format!(
            "HTTP {method} {path} returned no JSON object or array: {}",
            response.body
        ),
    )?;
    Ok(response.body)
}

fn ready(engine: &mut OwnedChild, port: u16) -> io::Result<()> {
    let started = Instant::now();
    loop {
        let remaining = READY_TIMEOUT.saturating_sub(started.elapsed());
        let failure = match http(
            engine,
            port,
            "GET",
            "/health/ready",
            None,
            remaining.min(REQUEST_TIMEOUT),
        ) {
            Ok(response) if response.status == 200 && response.body == "ready" => return Ok(()),
            Ok(response) => format!("HTTP {}: {}", response.status, response.body),
            Err(failure) => failure.to_string(),
        };
        engine.running()?;
        require(
            started.elapsed() < READY_TIMEOUT,
            format!(
                "XERJ readiness exceeded 90 seconds: {failure}; stdout tail: {}",
                engine.tail_text()
            ),
        )?;
        thread::sleep(Duration::from_millis(100));
    }
}

fn verify_search(body: &Value) -> io::Result<()> {
    let hits = body
        .pointer("/hits/hits")
        .and_then(Value::as_array)
        .ok_or_else(|| error(format!("search has no hits array: {body}")))?;
    require(
        hits.iter().any(|hit| {
            hit["_id"] == DOC_ID
                && hit.pointer("/_source/text").and_then(Value::as_str) == Some(TEXT)
        }),
        format!("search did not retrieve the exact fixture document: {body}"),
    )
}

fn verify_memory(body: &Value, id: &str) -> io::Result<()> {
    let hits = body
        .get("hits")
        .and_then(Value::as_array)
        .ok_or_else(|| error(format!("memory recall has no hits array: {body}")))?;
    require(
        hits.iter()
            .any(|hit| hit["id"] == id && hit["text"] == TEXT),
        format!("memory recall did not retrieve {id}: {body}"),
    )
}

fn search_hits(body: &Value) -> io::Result<&Vec<Value>> {
    body.pointer("/hits/hits")
        .and_then(Value::as_array)
        .ok_or_else(|| error(format!("search has no hits array: {body}")))
}

fn verify_code_fixture(body: &Value, match_line: bool) -> io::Result<()> {
    require(
        search_hits(body)?.iter().any(|hit| {
            let source = &hit["_source"];
            source["ax_path"] == SOURCE_FILE
                && (source["name"] == SOURCE_SYMBOL || source["title"] == SOURCE_SYMBOL)
                && source["line"].as_u64() == Some(SOURCE_LINE)
                && source["code"]
                    .as_str()
                    .is_some_and(|code| code.contains(SOURCE_DEFINITION))
                && (!match_line || hit["match_line"].as_u64() == Some(SOURCE_LINE))
        }),
        format!("search did not retrieve the exact Rust AST definition and source line: {body}"),
    )
}

fn verify_corpus_documents(body: &Value) -> io::Result<()> {
    verify_code_fixture(body, false)?;
    verify_sqlite_fixture(body)?;
    require(
        search_hits(body)?.iter().any(|hit| {
            let source = &hit["_source"];
            source["ax_path"] == PROSE_FILE
                && source.as_object().is_some_and(|fields| {
                    fields.values().any(|value| {
                        value
                            .as_str()
                            .is_some_and(|text| text.contains(PROSE_TOKEN))
                    })
                })
        }),
        format!("search did not retrieve the exact Markdown fixture: {body}"),
    )
}

fn verify_sqlite_fixture(body: &Value) -> io::Result<()> {
    require(
        search_hits(body)?.iter().any(|hit| {
            let source = &hit["_source"];
            source["ax_path"] == SQLITE_FILE
                && source["id"].as_u64() == Some(37)
                && source["text"] == SQLITE_TEXT
                && source["ax_locator"] == "tcalibrations:r37"
        }),
        format!("search did not retrieve the exact SQLite table row: {body}"),
    )
}

fn parse_cli_json(bytes: &[u8]) -> io::Result<Value> {
    let value: Value = serde_json::from_slice(bytes).map_err(|failure| {
        error(format!(
            "autoindex stdout has no complete JSON result: {failure}"
        ))
    })?;
    require(value.is_object(), "autoindex JSON result is not an object")?;
    Ok(value)
}

fn verify_autoindex_plan(plan: &Value) -> io::Result<()> {
    let files = plan["files"]
        .as_object()
        .ok_or_else(|| error("dry-run plan has no files"))?;
    require(
        files.len() == 3,
        format!("dry-run did not plan exactly three fixture files: {plan}"),
    )?;
    require(
        files
            .values()
            .any(|file| file["rel"] == SOURCE_FILE && file["family"] == "code")
            && files.values().any(|file| file["rel"] == PROSE_FILE)
            && files.values().any(|file| file["rel"] == SQLITE_FILE),
        format!("dry-run omitted a fixture or did not recognize Rust source: {plan}"),
    )?;
    for key in ["junk_files", "duplicate_files", "refused_datasets"] {
        require(
            plan.get(key)
                .is_none_or(|value| value.as_array().is_some_and(Vec::is_empty)),
            format!("dry-run reported {key}: {plan}"),
        )?;
    }
    Ok(())
}

fn verify_autoindex_summary(summary: &Value, corpus: &Path) -> io::Result<()> {
    require(
        summary["doc_kind"] == "run"
            && summary["prefix"] == CORPUS_PREFIX
            && summary["root"].as_str() == corpus.to_str()
            && summary["run_id"].as_str().is_some_and(|id| !id.is_empty()),
        format!("autoindex result does not identify this fixture run: {summary}"),
    )?;
    for (key, expected) in [
        ("files_total", 3),
        ("files_indexed", 3),
        ("code_files", 1),
        ("code_files_indexed", 1),
        ("code_files_junked", 0),
        ("files_junk", 0),
        ("junk_records_total", 0),
    ] {
        require(
            summary[key].as_u64() == Some(expected),
            format!("autoindex result has incorrect {key}, expected {expected}: {summary}"),
        )?;
    }
    require(
        summary["records_total"]
            .as_u64()
            .is_some_and(|count| count >= 3),
        format!("autoindex result recorded no fixture data: {summary}"),
    )?;
    for key in [
        "datasets_refused",
        "files_refused",
        "skipped_appeared",
        "pdf_worker_unavailable",
        "files_removed",
    ] {
        require(
            summary
                .get(key)
                .is_none_or(|value| value.as_u64() == Some(0)),
            format!("autoindex result reports incomplete data in {key}: {summary}"),
        )?;
    }
    require(
        summary
            .get("catalog_alias_sweep_error")
            .is_none_or(Value::is_null),
        format!("autoindex catalog alias sweep failed: {summary}"),
    )
}

fn verify_finished_journal(bytes: &[u8], summary: &Value) -> io::Result<()> {
    let text = std::str::from_utf8(bytes).map_err(io::Error::other)?;
    let last = text
        .lines()
        .rev()
        .find(|line| !line.trim().is_empty())
        .ok_or_else(|| error("autoindex journal has no finish record"))?;
    let record: Value = serde_json::from_str(last).map_err(io::Error::other)?;
    require(
        record["kind"] == "finish" && record["summary"] == *summary,
        "autoindex journal did not finish with the stdout run summary",
    )
}

fn cli_result(cli: &mut OwnedChild, engine: &mut OwnedChild) -> io::Result<Value> {
    let started = Instant::now();
    loop {
        engine.running()?;
        let status = cli.handle.map(child::status);
        if let Err(failure) = cli.drain(true) {
            if status != Some(child::ERR_NOT_FOUND) {
                return Err(failure);
            }
        }
        match status {
            Some(child::STATUS_EXITED | child::ERR_NOT_FOUND) => {
                let output = std::mem::take(&mut cli.pending);
                cli.stop()?;
                return parse_cli_json(&output).map_err(|failure| {
                    error(format!(
                        "{}: {failure}; stdout tail: {}",
                        cli.label,
                        cli.tail_text()
                    ))
                });
            }
            Some(child::STATUS_STARTING | child::STATUS_RUNNING | child::STATUS_STOPPING) => {}
            status => {
                return Err(error(format!(
                    "lost {} before completion: {status:?}",
                    cli.label
                )));
            }
        }
        require(
            started.elapsed() < CLI_TIMEOUT,
            format!(
                "{} exceeded 120 seconds; stdout tail: {}",
                cli.label,
                cli.tail_text()
            ),
        )?;
        thread::sleep(POLL);
    }
}

fn autoindex_fixture(
    program: &str,
    root: &Path,
    engine: &mut OwnedChild,
    port: u16,
) -> io::Result<()> {
    let corpus = root.join("corpus");
    let state = root.join("autoindex-state");
    fs::create_dir(&corpus)?;
    fs::write(corpus.join(SOURCE_FILE), SOURCE_TEXT)?;
    fs::write(corpus.join(PROSE_FILE), PROSE_TEXT)?;
    fs::write(corpus.join(SQLITE_FILE), SQLITE_BYTES)?;
    let corpus = fs::canonicalize(corpus)?;
    let args = vec![
        "autoindex".to_owned(),
        corpus.to_string_lossy().into_owned(),
        "--prefix".to_owned(),
        CORPUS_PREFIX.to_owned(),
        "--url".to_owned(),
        format!("http://127.0.0.1:{port}"),
        "--state-dir".to_owned(),
        state.to_string_lossy().into_owned(),
        "--workers".to_owned(),
        "1".to_owned(),
        "--json".to_owned(),
        "--disable-feedback".to_owned(),
    ];
    let mut dry_args = args.clone();
    dry_args.push("--dry-run".to_owned());
    let mut dry = OwnedChild::spawn(program, &dry_args, root, "autoindex_dry_run")?;
    verify_autoindex_plan(&cli_result(&mut dry, engine)?)?;
    mark("autoindex_dry_run");

    let mut indexer = OwnedChild::spawn(program, &args, root, "autoindex")?;
    let summary = cli_result(&mut indexer, engine)?;
    verify_autoindex_summary(&summary, &corpus)?;
    let mut journal = fs::File::open(state.join("journal.ndjson"))?;
    let mut journal_bytes = Vec::new();
    Read::by_ref(&mut journal)
        .take((MAX_WIRE + 1) as u64)
        .read_to_end(&mut journal_bytes)?;
    require(
        journal_bytes.len() <= MAX_WIRE,
        "autoindex journal exceeds 2 MiB",
    )?;
    verify_finished_journal(&journal_bytes, &summary)?;
    let catalog = http_json(
        engine,
        port,
        "POST",
        "/autoindex-catalog/_search",
        Some(json!({
            "query":{"bool":{"filter":[{"term":{"doc_kind":"run"}},{"term":{"prefix":CORPUS_PREFIX}}]}},"size":5
        })),
    )?;
    require(
        search_hits(&catalog)?
            .iter()
            .any(|hit| hit["_source"]["run_id"] == summary["run_id"]),
        format!("autoindex finished run is not visible in its catalog: {catalog}"),
    )?;
    verify_corpus_documents(&http_json(
        engine,
        port,
        "POST",
        &format!("/{CORPUS_PREFIX}-*/_search"),
        Some(json!({"query":{"match_all":{}},"size":50})),
    )?)?;
    mark("autoindex_fixture_search");
    Ok(())
}

fn search_body() -> Value {
    json!({"query":{"match":{"text":TOKEN}},"size":5})
}
fn recall_body() -> Value {
    json!({"query":TOKEN,"k":5})
}

fn take_rpc_response(pending: &mut Vec<u8>, id: u64) -> io::Result<Option<Value>> {
    let Some(end) = pending.iter().position(|byte| *byte == b'\n') else {
        return Ok(None);
    };
    let line: Vec<u8> = pending.drain(..=end).collect();
    let response: Value = serde_json::from_slice(&line).map_err(|failure| {
        error(format!(
            "MCP stdout contains invalid JSON: {failure}; line={}",
            String::from_utf8_lossy(&line)
        ))
    })?;
    require(
        response["jsonrpc"] == "2.0",
        format!("invalid MCP JSON-RPC version: {response}"),
    )?;
    require(
        response["id"] == id,
        format!("unexpected MCP response id, expected {id}: {response}"),
    )?;
    require(
        response.get("error").is_none(),
        format!("MCP request {id} returned a protocol error: {response}"),
    )?;
    response
        .get("result")
        .cloned()
        .map(Some)
        .ok_or_else(|| error("MCP response has no result"))
}

fn rpc(
    mcp: &mut OwnedChild,
    engine: &mut OwnedChild,
    id: u64,
    method: &str,
    params: Value,
) -> io::Result<Value> {
    mcp.send(&json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}))?;
    let started = Instant::now();
    loop {
        engine.running()?;
        mcp.drain(true)?;
        if let Some(response) = take_rpc_response(&mut mcp.pending, id)? {
            return Ok(response);
        }
        require(
            matches!(
                mcp.handle.map(child::status),
                Some(child::STATUS_STARTING | child::STATUS_RUNNING)
            ),
            format!(
                "MCP child exited before response {id}; stdout tail: {}",
                mcp.tail_text()
            ),
        )?;
        require(
            started.elapsed() < REQUEST_TIMEOUT,
            format!("MCP {method} exceeded 20 seconds"),
        )?;
        thread::sleep(POLL);
    }
}

fn tool(
    mcp: &mut OwnedChild,
    engine: &mut OwnedChild,
    id: u64,
    name: &str,
    arguments: Value,
) -> io::Result<Value> {
    let result = rpc(
        mcp,
        engine,
        id,
        "tools/call",
        json!({"name":name,"arguments":arguments}),
    )?;
    require(
        result["isError"] == false,
        format!("MCP tool {name} failed: {result}"),
    )?;
    let text = result
        .pointer("/content/0/text")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            error(format!(
                "MCP tool {name} returned no text content: {result}"
            ))
        })?;
    serde_json::from_str(text).map_err(io::Error::other)
}

// App-server uses JSON-RPC-shaped lines without the mandatory MCP `jsonrpc`
// member, and interleaves lifecycle notifications with request responses.
fn take_app_server_response(pending: &mut Vec<u8>, id: u64) -> io::Result<Option<Value>> {
    while let Some(end) = pending.iter().position(|byte| *byte == b'\n') {
        let line: Vec<u8> = pending.drain(..=end).collect();
        let response: Value = serde_json::from_slice(&line)
            .map_err(|failure| error(format!("invalid app-server JSON: {failure}")))?;
        if response.get("id").is_none() && response["method"].is_string() {
            continue;
        }
        require(
            response["id"] == id,
            format!("unexpected app-server response id, expected {id}: {response}"),
        )?;
        require(
            response.get("error").is_none(),
            format!("app-server request {id} returned an error: {response}"),
        )?;
        return response
            .get("result")
            .cloned()
            .map(Some)
            .ok_or_else(|| error("app-server response has no result"));
    }
    Ok(None)
}

fn app_server_rpc(
    codex: &mut OwnedChild,
    engine: &mut OwnedChild,
    id: u64,
    method: &str,
    params: Value,
) -> io::Result<Value> {
    codex.send(&json!({"id":id,"method":method,"params":params}))?;
    let started = Instant::now();
    loop {
        engine.running()?;
        codex.drain(true)?;
        if let Some(response) = take_app_server_response(&mut codex.pending, id)? {
            return Ok(response);
        }
        require(
            matches!(
                codex.handle.map(child::status),
                Some(child::STATUS_STARTING | child::STATUS_RUNNING)
            ),
            format!(
                "Codex app-server exited before {method}; stdout tail: {}",
                codex.tail_text()
            ),
        )?;
        require(
            started.elapsed() < APP_SERVER_TIMEOUT,
            format!("Codex app-server {method} exceeded 150 seconds"),
        )?;
        thread::sleep(POLL);
    }
}

fn app_server_tool_result(result: Value) -> io::Result<Value> {
    require(
        result["isError"] != true,
        format!("Codex MCP tool failed: {result}"),
    )?;
    let text = result
        .pointer("/content/0/text")
        .and_then(Value::as_str)
        .ok_or_else(|| error("Codex MCP tool returned no text result"))?;
    serde_json::from_str(text).map_err(io::Error::other)
}

fn project_default_service_status(result: &Value) -> io::Result<Value> {
    require(
        result["isError"] != true,
        "default XERJ status tool returned an error",
    )?;
    let text = result
        .pointer("/content/0/text")
        .and_then(Value::as_str)
        .ok_or_else(|| error("default XERJ status tool returned no text"))?;
    require(
        text.contains("the node at http://127.0.0.1:9200 is UP"),
        "default XERJ status did not confirm its configured node",
    )?;
    let health = if text.contains("cluster health: green") {
        "green"
    } else if text.contains("cluster health: yellow") {
        "yellow"
    } else {
        return Err(error(
            "default XERJ status did not report green/yellow cluster health",
        ));
    };
    // Status includes actual index names. Preserve only these health facts.
    Ok(json!({"nodeReachable":true,"clusterHealth":health,"isError":false}))
}

fn codex_mcp_fixture(
    codex_program: &str,
    xerj_program: &str,
    run_root: &Path,
    engine: &mut OwnedChild,
    port: u16,
) -> io::Result<()> {
    let home = run_root.join("codex-home");
    fs::create_dir(&home)?;
    let workspace = run_root.join("corpus");
    let endpoint = format!("http://127.0.0.1:{port}");
    let config = format!(
        r#"model = "trueos-xerj-smoke"
model_provider = "smoke"
check_for_update_on_startup = false
approval_policy = "never"
sandbox_mode = "danger-full-access"
cli_auth_credentials_store = "file"
mcp_oauth_credentials_store = "file"
chatgpt_base_url = {endpoint}

[model_providers.smoke]
name = "Native XERJ smoke; no model turn"
base_url = {model_url}
wire_api = "responses"
requires_openai_auth = false

[features]
plugins = false
remote_plugin = false
shell_snapshot = false

[projects.{workspace}]
trust_level = "trusted"

[mcp_servers.xerj]
command = {xerj_program}
args = ["mcp", "--url", {endpoint}, "--disable-feedback"]
startup_timeout_sec = 60
tool_timeout_sec = 60

[mcp_servers.xerj.env]
XERJ_AUTH = {fixture_auth}
XERJ_DISABLE_FEEDBACK = "true"
NO_PROXY = "127.0.0.1,localhost"
no_proxy = "127.0.0.1,localhost"
"#,
        endpoint = json!(endpoint),
        model_url = json!(format!("{endpoint}/__unused_model_endpoint")),
        workspace = json!(workspace.to_string_lossy()),
        xerj_program = json!(xerj_program),
        fixture_auth = json!(FIXTURE_AUTH),
    );
    fs::write(home.join("config.toml"), &config)?;
    let args = vec![
        "app-server".to_owned(),
        "--listen".to_owned(),
        "stdio://".to_owned(),
    ];
    let extra_env = [
        ("CODEX_HOME", home.to_string_lossy().into_owned()),
        ("CODEX_SQLITE_HOME", home.to_string_lossy().into_owned()),
        ("OPENAI_API_KEY", String::new()),
        (
            "XDG_CONFIG_HOME",
            run_root.join("codex-config").to_string_lossy().into_owned(),
        ),
        (
            "XDG_CACHE_HOME",
            run_root.join("codex-cache").to_string_lossy().into_owned(),
        ),
    ];
    // Child cwd is also its native filesystem root: include the sibling home,
    // config and cache directories while thread/start selects corpus below it.
    let mut codex =
        OwnedChild::spawn_with_env(codex_program, &args, run_root, "codex", &extra_env)?;
    let initialized = app_server_rpc(
        &mut codex,
        engine,
        1,
        "initialize",
        json!({
            "clientInfo":{"name":"trueos_xerj_native_smoke","version":"1"},
            "capabilities":{"experimentalApi":true}
        }),
    )?;
    require(
        initialized["platformOs"] == std::env::consts::OS,
        format!("app-server platform differs from this native probe: {initialized}"),
    )?;
    require(
        initialized["codexHome"] == home.to_string_lossy().as_ref(),
        "app-server did not use this run's isolated CODEX_HOME",
    )?;
    codex.send(&json!({"method":"initialized"}))?;
    mark("codex_initialize");
    let duplicate_request = OwnedChild::launch_request(codex_program, &args, run_root, &extra_env)?;
    match child::spawn_program(&duplicate_request) {
        Err(child::ERR_UNAVAILABLE) => mark("codex_second_rejected"),
        Err(code) => {
            return Err(error(format!(
                "second Codex returned unexpected admission error: {code}"
            )));
        }
        Ok(handle) => {
            let mut duplicate = OwnedChild {
                handle: Some(handle),
                label: "unexpected_duplicate_codex",
                tail: VecDeque::new(),
                pending: Vec::new(),
            };
            duplicate.stop()?;
            return Err(error(
                "launcher admitted a second Codex while the first was active",
            ));
        }
    }
    let started = app_server_rpc(
        &mut codex,
        engine,
        2,
        "thread/start",
        json!({
            "cwd":workspace.to_string_lossy(),"model":"trueos-xerj-smoke","modelProvider":"smoke",
            "approvalPolicy":"never","sandbox":"danger-full-access","ephemeral":true
        }),
    )?;
    let thread_id = started
        .pointer("/thread/id")
        .and_then(Value::as_str)
        .ok_or_else(|| error("Codex thread/start returned no thread id"))?;
    let status = app_server_rpc(
        &mut codex,
        engine,
        3,
        "mcpServerStatus/list",
        json!({"threadId":thread_id}),
    )?;
    let servers = status["data"]
        .as_array()
        .ok_or_else(|| error("Codex MCP status has no server list"))?;
    let server = servers
        .iter()
        .find(|server| server["name"] == "xerj")
        .ok_or_else(|| error(format!("Codex did not load configured XERJ MCP: {status}")))?;
    require(
        server["serverInfo"]["name"] == "xerj-mcp" && server["pluginId"].is_null(),
        format!("Codex did not initialize direct XERJ MCP: {server}"),
    )?;
    for name in ["xerj_search", "xerj_memory_recall"] {
        require(
            server["tools"].get(name).is_some(),
            format!("Codex omitted MCP tool {name}"),
        )?;
    }
    mark("codex_mcp_tools");
    let search = app_server_tool_result(app_server_rpc(
        &mut codex,
        engine,
        4,
        "mcpServer/tool/call",
        json!({
            "threadId":thread_id,"server":"xerj","tool":"xerj_search",
            "arguments":{"index":INDEX,"query":{"match":{"text":TOKEN}},"size":5}
        }),
    )?)?;
    verify_search(&search)?;
    mark("codex_mcp_search");
    let code = app_server_tool_result(app_server_rpc(
        &mut codex,
        engine,
        5,
        "mcpServer/tool/call",
        json!({
            "threadId":thread_id,"server":"xerj","tool":"xerj_search",
            "arguments":{"prefix":CORPUS_PREFIX,"query":SOURCE_SYMBOL,"size":10}
        }),
    )?)?;
    verify_code_fixture(&code, true)?;
    mark("codex_mcp_code_fixture");
    let sqlite = app_server_tool_result(app_server_rpc(
        &mut codex,
        engine,
        7,
        "mcpServer/tool/call",
        json!({
            "threadId":thread_id,"server":"xerj","tool":"xerj_search",
            "arguments":{"prefix":CORPUS_PREFIX,"query":SQLITE_TOKEN,"size":10}
        }),
    )?)?;
    verify_sqlite_fixture(&sqlite)?;
    mark("codex_mcp_sqlite_fixture");
    let recalled = app_server_tool_result(app_server_rpc(
        &mut codex,
        engine,
        6,
        "mcpServer/tool/call",
        json!({
            "threadId":thread_id,"server":"xerj","tool":"xerj_memory_recall",
            "arguments":{"namespace":NAMESPACE,"query":TOKEN,"k":5}
        }),
    )?)?;
    verify_memory(&recalled, HTTP_MEMORY)?;
    verify_memory(&recalled, MCP_MEMORY)?;
    mark("codex_mcp_memory");
    // Retain the isolated retrieval proof even if the later shared-service
    // check fails. That phase has a separate, projected evidence document.
    fs::write(
        run_root.join("codex-mcp-evidence.json"),
        serde_json::to_vec_pretty(&json!({
            "status":"PASS","initialize":initialized,"mcpServer":server,
            "search":search,"codeSearch":code,"sqliteSearch":sqlite,"memoryRecall":recalled,
            "secondCodexRejected":true,"nativeProbeTargetOs":std::env::consts::OS,
            "modelTurnSubmitted":false,"pluginsEnabled":false
        }))?,
    )?;
    codex.stop()?;
    mark("codex_stopped");
    let (default_config, _) = config
        .split_once("[mcp_servers.xerj]")
        .ok_or_else(|| error("fixture config has no MCP override boundary"))?;
    fs::write(home.join("config.toml"), default_config)?;
    let mut default_env = extra_env.to_vec();
    // The production lifecycle deliberately discovers only its fixed service key.
    default_env.push(("XERJ_AUTH", String::new()));
    let default_request = OwnedChild::launch_request(codex_program, &args, run_root, &default_env)?;
    // Child STATUS_EXITED precedes the final native-job/carrier drain. The
    // launcher intentionally retains admission until that offline milestone.
    let admission_started = Instant::now();
    let mut admission_retries = 0u32;
    let relaunched_handle = loop {
        engine.running()?;
        match child::spawn_program(&default_request) {
            Ok(handle) => break handle,
            Err(child::ERR_UNAVAILABLE) if admission_started.elapsed() < READY_TIMEOUT => {
                admission_retries += 1;
                thread::sleep(Duration::from_millis(100));
            }
            Err(code) => return Err(error(format!("Codex relaunch admission failed: {code}"))),
        }
    };
    let mut relaunched = OwnedChild {
        handle: Some(relaunched_handle),
        label: "codex_relaunch",
        tail: VecDeque::new(),
        pending: Vec::new(),
    };
    let reinitialized = app_server_rpc(
        &mut relaunched,
        engine,
        1,
        "initialize",
        json!({
            "clientInfo":{"name":"trueos_xerj_native_smoke_relaunch","version":"1"},
            "capabilities":{"experimentalApi":true}
        }),
    )?;
    require(
        reinitialized["platformOs"] == std::env::consts::OS
            && reinitialized["codexHome"] == home.to_string_lossy().as_ref(),
        "relaunched Codex did not initialize with this run's native configuration",
    )?;
    relaunched.send(&json!({"method":"initialized"}))?;
    let default_thread = app_server_rpc(
        &mut relaunched,
        engine,
        2,
        "thread/start",
        json!({
            "cwd":workspace.to_string_lossy(),"model":"trueos-xerj-smoke","modelProvider":"smoke",
            "approvalPolicy":"never","sandbox":"danger-full-access","ephemeral":true
        }),
    )?;
    let default_thread_id = default_thread
        .pointer("/thread/id")
        .and_then(Value::as_str)
        .ok_or_else(|| error("default Codex thread/start returned no thread id"))?;
    let default_inventory = app_server_rpc(
        &mut relaunched,
        engine,
        3,
        "mcpServerStatus/list",
        json!({"threadId":default_thread_id}),
    )?;
    let default_server = default_inventory["data"]
        .as_array()
        .and_then(|servers| servers.iter().find(|server| server["name"] == "xerj"))
        .ok_or_else(|| error("native packaged default did not supply XERJ MCP"))?;
    require(
        default_server["serverInfo"]["name"] == "xerj-mcp"
            && default_server["pluginId"].is_null()
            && default_server["tools"].get("xerj_search").is_some()
            && default_server["tools"].get("xerj_status").is_some(),
        "packaged direct XERJ MCP did not initialize with search/status tools",
    )?;
    mark("codex_default_mcp_tools");
    let default_status = app_server_rpc(
        &mut relaunched,
        engine,
        4,
        "mcpServer/tool/call",
        json!({
            "threadId":default_thread_id,"server":"xerj","tool":"xerj_status","arguments":{}
        }),
    )?;
    let default_health = project_default_service_status(&default_status)?;
    let default_evidence = json!({
        "status":"PASS","initialize":reinitialized,"mcpOverridePresent":false,
        "serverInfo":default_server["serverInfo"],"pluginId":default_server["pluginId"],
        "toolNames":default_server["tools"].as_object().map(|tools| tools.keys().collect::<Vec<_>>()),
        "serviceStatus":default_health,"modelTurnSubmitted":false
    });
    mark("codex_default_service_ready");
    relaunched.stop()?;
    mark("codex_relaunch");
    let remaining_service = http(
        engine,
        9200,
        "GET",
        "/_security/_authenticate",
        None,
        REQUEST_TIMEOUT,
    )?;
    require(
        remaining_service.status == 401,
        "normal XERJ service did not remain authenticated after the Codex child exited",
    )?;
    let mut default_evidence = default_evidence;
    default_evidence["serviceAfterCodexChildExit"] = json!({"httpStatus":401,"leftRunning":true});
    fs::write(
        run_root.join("codex-default-mcp-evidence.json"),
        serde_json::to_vec_pretty(&default_evidence)?,
    )?;
    mark("codex_default_service_retained");
    fs::write(
        run_root.join("codex-mcp-evidence.json"),
        serde_json::to_vec_pretty(&json!({
            "status":"PASS","initialize":initialized,"mcpServer":server,
            "search":search,"codeSearch":code,"sqliteSearch":sqlite,"memoryRecall":recalled,
            "secondCodexRejected":true,"relaunchInitialize":reinitialized,
            "relaunchAdmissionRetries":admission_retries,
            "nativeProbeTargetOs":std::env::consts::OS,
            "modelTurnSubmitted":false,"pluginsEnabled":false
        }))?,
    )?;
    Ok(())
}

// Exact wire types from XERJ's proto/xerj.proto. A unary client needs neither
// generated server stubs nor a host protoc executable.
#[derive(Clone, PartialEq, prost::Message)]
struct GrpcHealthRequest {}

#[derive(Clone, PartialEq, prost::Message)]
struct GrpcHealthResponse {
    #[prost(string, tag = "1")]
    status: String,
    #[prost(int32, tag = "2")]
    num_indices: i32,
    #[prost(int64, tag = "3")]
    total_docs: i64,
}

fn verify_grpc_health(response: &GrpcHealthResponse) -> io::Result<()> {
    require(
        matches!(response.status.as_str(), "green" | "yellow")
            && response.num_indices > 0
            && response.total_docs > 0,
        format!("gRPC health did not report the indexed fixture: {response:?}"),
    )
}

fn grpc_health(engine: &mut OwnedChild, port: u16, stage: &str) -> io::Result<()> {
    // All Hull control stays outside the runtime. Its sole thread drives the
    // HTTP/2 client; no runtime worker can accidentally call the child ABI.
    engine.running()?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_io()
        .enable_time()
        .build()?;
    let result = runtime.block_on(async {
        tokio::time::timeout(REQUEST_TIMEOUT, async {
            let endpoint =
                tonic::transport::Endpoint::from_shared(format!("http://127.0.0.1:{port}"))
                    .map_err(io::Error::other)?
                    .connect_timeout(REQUEST_TIMEOUT)
                    .timeout(REQUEST_TIMEOUT);
            let channel = endpoint.connect().await.map_err(io::Error::other)?;
            let mut client = tonic::client::Grpc::new(channel)
                .max_decoding_message_size(4096)
                .max_encoding_message_size(4096);
            client.ready().await.map_err(io::Error::other)?;
            let mut request = tonic::Request::new(GrpcHealthRequest {});
            request.set_timeout(REQUEST_TIMEOUT);
            let response: tonic::Response<GrpcHealthResponse> = client
                .unary(
                    request,
                    tonic::codegen::http::uri::PathAndQuery::from_static(
                        "/xerj.v1.XerjSearch/Health",
                    ),
                    tonic::codec::ProstCodec::default(),
                )
                .await
                .map_err(io::Error::other)?;
            // Tonic validates HTTP/2 framing, protobuf and the final gRPC status.
            Ok::<_, io::Error>(response.into_inner())
        })
        .await
    });
    drop(runtime);
    engine.running()?;
    let response = result.map_err(|_| error("gRPC Health exceeded 20 seconds"))??;
    verify_grpc_health(&response)?;
    println!(
        "xerj-smoke v=1 stage={stage} ok=true status={} indices={} docs={}",
        json!(response.status),
        response.num_indices,
        response.total_docs
    );
    Ok(())
}

fn verify_persisted(engine: &mut OwnedChild, port: u16) -> io::Result<()> {
    verify_search(&http_json(
        engine,
        port,
        "POST",
        &format!("/{INDEX}/_search"),
        Some(search_body()),
    )?)?;
    let memories = http_json(
        engine,
        port,
        "POST",
        &format!("/_memory/{NAMESPACE}/_recall"),
        Some(recall_body()),
    )?;
    verify_memory(&memories, HTTP_MEMORY)?;
    verify_memory(&memories, MCP_MEMORY)?;
    verify_corpus_documents(&http_json(
        engine,
        port,
        "POST",
        &format!("/{CORPUS_PREFIX}-*/_search"),
        Some(json!({"query":{"match_all":{}},"size":50})),
    )?)
}

#[cfg(all(target_os = "trueos", feature = "openssl-check"))]
fn crypto_roundtrip() -> Result<bool, openssl::error::ErrorStack> {
    use openssl::{
        asn1::Asn1Time,
        ec::{EcGroup, EcKey},
        hash::MessageDigest,
        nid::Nid,
        pkey::PKey,
        sign::{Signer, Verifier},
    };
    let mut random = [0u8; 32];
    openssl::rand::rand_bytes(&mut random)?;
    if random.iter().all(|&byte| byte == 0) {
        return Ok(false);
    }
    let digest = openssl::sha::sha256(b"abc");
    let expected = [
        0xba, 0x78, 0x16, 0xbf, 0x8f, 0x01, 0xcf, 0xea, 0x41, 0x41, 0x40, 0xde, 0x5d, 0xae, 0x22,
        0x23, 0xb0, 0x03, 0x61, 0xa3, 0x96, 0x17, 0x7a, 0x9c, 0xb4, 0x10, 0xff, 0x61, 0xf2, 0x00,
        0x15, 0xad,
    ];
    let group = EcGroup::from_curve_name(Nid::X9_62_PRIME256V1)?;
    let key = PKey::from_ec_key(EcKey::generate(&group)?)?;
    let mut signer = Signer::new(MessageDigest::sha256(), &key)?;
    signer.update(b"TRUEOS native passkey crypto fixture")?;
    let signature = signer.sign_to_vec()?;
    let mut verifier = Verifier::new(MessageDigest::sha256(), &key)?;
    verifier.update(b"TRUEOS native passkey crypto fixture")?;
    let accepted = verifier.verify(&signature)?;
    let mut verifier = Verifier::new(MessageDigest::sha256(), &key)?;
    verifier.update(b"tampered TRUEOS native passkey crypto fixture")?;
    let rejected = !verifier.verify(&signature)?;
    let utc = Asn1Time::from_unix(1_234_567_890)?.to_string();
    Ok(digest == expected && accepted && rejected && utc == "Feb 13 23:31:30 2009 GMT")
}

#[cfg(all(target_os = "trueos", feature = "openssl-check"))]
fn native_crypto() -> io::Result<()> {
    // Concurrent callers exercise OpenSSL initialization, mutexes, native
    // thread-local state, and the hardware entropy source. Print no key bytes.
    let mut jobs = Vec::new();
    for _ in 0..2 {
        jobs.push(
            thread::Builder::new()
                .name("crypto-smoke".into())
                .stack_size(4 * 1024 * 1024)
                .spawn(crypto_roundtrip)?,
        );
    }
    for job in jobs {
        let result = job
            .join()
            .map_err(|_| error("native crypto worker panicked"))?
            .map_err(|failure| error(format!("native crypto failed: {failure}")))?;
        require(
            result,
            "native crypto signature/hash/time validation failed",
        )?;
    }
    let result = crypto_roundtrip()
        .map_err(|failure| error(format!("entry-thread crypto failed: {failure}")))?;
    require(result, "entry-thread crypto validation failed")?;
    mark("crypto_native");
    Ok(())
}

fn run() -> io::Result<()> {
    #[cfg(all(target_os = "trueos", feature = "openssl-check"))]
    native_crypto()?;
    require(
        cfg!(target_os = "trueos"),
        "run xerj-smoke.bp on TRUEOS; host checks do not launch XERJ",
    )?;
    let mut args = std::env::args().skip(1);
    let program = args.next().unwrap_or_else(|| "xerj.bp".to_owned());
    let cwd = std::env::current_dir()?;
    let root = args
        .next()
        .map(PathBuf::from)
        .unwrap_or_else(|| cwd.clone());
    let codex_program = args.next().unwrap_or_else(|| "codex.bp".to_owned());
    require(
        args.next().is_none(),
        "usage: xerj-smoke [xerj.bp|/path/xerj.bp] [existing-state-parent] [codex.bp|/path/codex.bp]",
    )?;
    let root = fs::canonicalize(root)?;
    let seed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(io::Error::other)?
        .as_nanos();
    let (port, reservations) = reserve_ports(seed)?;
    let run_root = fresh_directory(&root, seed)?;
    // XERJ owns persistent engine state under its own native access key. The
    // probe passes a unique path but neither creates nor reads that state.
    let data = Path::new("/apps/xerj/smoke").join(
        run_root
            .file_name()
            .ok_or_else(|| error("fixture run directory has no basename"))?,
    );
    let engine_args: Vec<String> = [
        "--insecure".to_owned(),
        "--bind".to_owned(),
        "127.0.0.1".to_owned(),
        "--port".to_owned(),
        port.to_string(),
        "--no-auto-port".to_owned(),
        "--data-dir".to_owned(),
        data.to_string_lossy().into_owned(),
        "--embed-mode".to_owned(),
        "lexical".to_owned(),
        "--disable-feedback".to_owned(),
    ]
    .into();
    println!(
        "xerj-smoke v=1 stage=begin program={} data={} es_port={} rest_port={} grpc_port={} embed_mode=lexical feedback=false",
        json!(program),
        json!(data.to_string_lossy()),
        port,
        port + 1,
        port + 2
    );
    drop(reservations);
    let mut engine = OwnedChild::spawn(&program, &engine_args, &run_root, "engine")?;
    ready(&mut engine, port)?;
    ready(&mut engine, port + 1)?;
    mark("http_ready");
    autoindex_fixture(&program, &run_root, &mut engine, port)?;

    http_json(
        &mut engine,
        port,
        "PUT",
        &format!("/{INDEX}"),
        Some(json!({"mappings":{"properties":{"text":{"type":"text"}}}})),
    )?;
    let indexed = http_json(
        &mut engine,
        port,
        "PUT",
        &format!("/{INDEX}/_doc/{DOC_ID}?refresh=true"),
        Some(json!({"text":TEXT})),
    )?;
    require(
        indexed["_id"] == DOC_ID,
        format!("index reply did not acknowledge document id: {indexed}"),
    )?;
    verify_search(&http_json(
        &mut engine,
        port,
        "POST",
        &format!("/{INDEX}/_search"),
        Some(search_body()),
    )?)?;
    mark("http_index_search");
    grpc_health(&mut engine, port + 2, "grpc_health")?;

    let memory = http_json(
        &mut engine,
        port,
        "POST",
        &format!("/_memory/{NAMESPACE}"),
        Some(json!({"id":HTTP_MEMORY,"text":TEXT})),
    )?;
    require(
        memory["id"] == HTTP_MEMORY && memory["created"] == true,
        format!("memory store did not acknowledge write: {memory}"),
    )?;
    verify_memory(
        &http_json(
            &mut engine,
            port,
            "POST",
            &format!("/_memory/{NAMESPACE}/_recall"),
            Some(recall_body()),
        )?,
        HTTP_MEMORY,
    )?;
    mark("http_memory");

    let mcp_args = vec![
        "mcp".to_owned(),
        "--url".to_owned(),
        format!("http://127.0.0.1:{port}"),
        "--disable-feedback".to_owned(),
    ];
    let mut mcp = OwnedChild::spawn(&program, &mcp_args, &run_root, "mcp")?;
    let initialized = rpc(
        &mut mcp,
        &mut engine,
        1,
        "initialize",
        json!({
            "protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"trueos-xerj-smoke","version":"1"}
        }),
    )?;
    require(
        initialized["protocolVersion"] == "2025-06-18"
            && initialized
                .pointer("/serverInfo/name")
                .and_then(Value::as_str)
                == Some("xerj-mcp"),
        format!("MCP initialize returned the wrong protocol/server: {initialized}"),
    )?;
    mcp.send(&json!({"jsonrpc":"2.0","method":"notifications/initialized"}))?;
    let listed = rpc(&mut mcp, &mut engine, 2, "tools/list", json!({}))?;
    let names = listed["tools"]
        .as_array()
        .ok_or_else(|| error("MCP tools/list returned no tools"))?;
    for required in ["xerj_search", "xerj_memory_store", "xerj_memory_recall"] {
        require(
            names.iter().any(|item| item["name"] == required),
            format!("MCP omitted tool {required}"),
        )?;
    }
    mark("mcp_initialize_tools");
    verify_search(&tool(
        &mut mcp,
        &mut engine,
        3,
        "xerj_search",
        json!({"index":INDEX,"query":{"match":{"text":TOKEN}},"size":5}),
    )?)?;
    mark("mcp_search");
    let stored = tool(
        &mut mcp,
        &mut engine,
        4,
        "xerj_memory_store",
        json!({"namespace":NAMESPACE,"id":MCP_MEMORY,"text":TEXT}),
    )?;
    require(
        stored["id"] == MCP_MEMORY && stored["created"] == true,
        format!("MCP memory store did not acknowledge write: {stored}"),
    )?;
    let recalled = tool(
        &mut mcp,
        &mut engine,
        5,
        "xerj_memory_recall",
        json!({"namespace":NAMESPACE,"query":TOKEN,"k":5}),
    )?;
    verify_memory(&recalled, HTTP_MEMORY)?;
    verify_memory(&recalled, MCP_MEMORY)?;
    mark("mcp_memory");
    verify_code_fixture(
        &tool(
            &mut mcp,
            &mut engine,
            6,
            "xerj_search",
            json!({"prefix":CORPUS_PREFIX,"query":SOURCE_SYMBOL,"size":10}),
        )?,
        true,
    )?;
    mark("mcp_code_fixture");
    mcp.stop()?;
    mark("mcp_stopped");

    codex_mcp_fixture(&codex_program, &program, &run_root, &mut engine, port)?;

    let flushed = http_json(&mut engine, port, "POST", "/_flush", None)?;
    require(
        flushed.pointer("/_shards/failed").and_then(Value::as_u64) == Some(0)
            && flushed
                .pointer("/_shards/successful")
                .and_then(Value::as_u64)
                .is_some_and(|count| count >= 2),
        format!("engine did not flush both test indices successfully: {flushed}"),
    )?;
    engine.stop()?;
    mark("engine_stopped");
    let mut restarted = OwnedChild::spawn(&program, &engine_args, &run_root, "engine_restart")?;
    ready(&mut restarted, port)?;
    verify_persisted(&mut restarted, port)?;
    grpc_health(&mut restarted, port + 2, "grpc_health_restart")?;
    mark("persistence_restart");
    restarted.stop()?;
    mark("children_stopped");
    marker(format!(
        "xerj-smoke v=1 stage=end ok=true data={} evidence_root={}",
        json!(data.to_string_lossy()),
        json!(run_root.to_string_lossy())
    ));
    Ok(())
}

fn main() {
    if let Err(failure) = run() {
        marker(format!(
            "xerj-smoke v=1 stage=end ok=false error={}",
            json!(failure.to_string())
        ));
        let _ = io::stdout().flush();
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grpc_health_requires_a_healthy_nonempty_engine() {
        let mut response = GrpcHealthResponse {
            status: "green".into(),
            num_indices: 1,
            total_docs: 1,
        };
        assert!(verify_grpc_health(&response).is_ok());
        response.status = "yellow".into();
        assert!(verify_grpc_health(&response).is_ok());
        response.status = "red".into();
        assert!(verify_grpc_health(&response).is_err());
        response.status = "green".into();
        response.num_indices = 0;
        assert!(verify_grpc_health(&response).is_err());
        response.num_indices = 1;
        response.total_docs = 0;
        assert!(verify_grpc_health(&response).is_err());
    }

    fn fixture_summary() -> Value {
        json!({
            "doc_kind":"run", "run_id":"owned-run", "root":"/owned/corpus",
            "prefix":CORPUS_PREFIX, "files_total":3, "files_indexed":3,
            "code_files":1, "code_files_indexed":1, "code_files_junked":0,
            "files_junk":0, "junk_records_total":0, "records_total":3
        })
    }

    fn fixture_code_hit() -> Value {
        json!({"match_line":SOURCE_LINE,"_source":{
            "ax_path":SOURCE_FILE,"title":SOURCE_SYMBOL,"line":SOURCE_LINE,
            "code":format!("{SOURCE_DEFINITION} {{\n    value + 37\n}}")
        }})
    }

    #[test]
    fn cli_completion_requires_a_complete_report_and_exact_three_file_coverage() {
        assert!(parse_cli_json(b"").is_err());
        assert!(parse_cli_json(b"xerj-done ok=true\n").is_err());
        assert!(parse_cli_json(b"{}\n{}").is_err());
        assert!(parse_cli_json(b"{\"doc_kind\":").is_err());
        let report = fixture_summary();
        let parsed = parse_cli_json(&serde_json::to_vec(&report).unwrap()).unwrap();
        verify_autoindex_summary(&parsed, Path::new("/owned/corpus")).unwrap();
        for (key, value) in [
            ("files_indexed", json!(1)),
            ("code_files_indexed", json!(0)),
            ("files_junk", json!(1)),
            ("datasets_refused", json!(1)),
            ("files_refused", json!(1)),
            ("skipped_appeared", json!(1)),
            ("prefix", json!("another-run")),
            ("catalog_alias_sweep_error", json!("refused")),
        ] {
            let mut incomplete = report.clone();
            incomplete[key] = value;
            assert!(
                verify_autoindex_summary(&incomplete, Path::new("/owned/corpus")).is_err(),
                "accepted {key}"
            );
        }
        assert!(verify_autoindex_summary(&report, Path::new("/other/corpus")).is_err());
    }

    #[test]
    fn dry_run_requires_rust_markdown_and_sqlite_without_junk_or_refused_datasets() {
        let plan = json!({"files":{
            "rust-key":{"rel":SOURCE_FILE,"family":"code"},
            "markdown-key":{"rel":PROSE_FILE,"family":"txt"},
            "sqlite-key":{"rel":SQLITE_FILE,"family":"sqlite"}
        },"junk_files":[],"duplicate_files":[]});
        verify_autoindex_plan(&plan).unwrap();
        let mut refused = plan.clone();
        refused["refused_datasets"] = json!([{"reason":"mapping refused"}]);
        assert!(verify_autoindex_plan(&refused).is_err());
        let mut missing = plan;
        missing["files"].as_object_mut().unwrap().remove("rust-key");
        assert!(verify_autoindex_plan(&missing).is_err());
    }

    #[test]
    fn journal_completion_must_match_the_stdout_run_and_be_the_last_record() {
        let summary = fixture_summary();
        let finished = format!(
            "{{\"kind\":\"run\"}}\n{}\n",
            json!({"kind":"finish","summary":summary})
        );
        verify_finished_journal(finished.as_bytes(), &summary).unwrap();
        assert!(verify_finished_journal(b"", &summary).is_err());
        assert!(
            verify_finished_journal(
                format!("{finished}{{\"kind\":\"resume\"}}\n").as_bytes(),
                &summary
            )
            .is_err()
        );
        let mut wrong_run = summary.clone();
        wrong_run["run_id"] = json!("different-run");
        assert!(verify_finished_journal(finished.as_bytes(), &wrong_run).is_err());
    }

    #[test]
    fn code_retrieval_requires_the_ast_definition_and_exact_source_line() {
        let hit = fixture_code_hit();
        let response = json!({"hits":{"hits":[hit]}});
        verify_code_fixture(&response, true).unwrap();
        for (key, value) in [
            ("line", json!(1)),
            ("code", json!("matching file body")),
            ("ax_path", json!("unrelated.rs")),
        ] {
            let mut wrong = response.clone();
            wrong["hits"]["hits"][0]["_source"][key] = value;
            assert!(verify_code_fixture(&wrong, true).is_err(), "accepted {key}");
        }
        let mut wrong_line = response.clone();
        wrong_line["hits"]["hits"][0]["match_line"] = json!(1);
        assert!(verify_code_fixture(&wrong_line, true).is_err());
        assert!(verify_corpus_documents(&response).is_err());
        let all = json!({"hits":{"hits":[fixture_code_hit(),{"_source":{"ax_path":PROSE_FILE,"text":PROSE_TEXT}},fixture_sqlite_hit()]}});
        verify_corpus_documents(&all).unwrap();
        assert_eq!(
            SOURCE_TEXT
                .lines()
                .nth((SOURCE_LINE - 1) as usize)
                .unwrap()
                .trim_end_matches(" {"),
            SOURCE_DEFINITION
        );
    }

    fn fixture_sqlite_hit() -> Value {
        json!({"_source":{
            "ax_path":SQLITE_FILE,"id":37,"text":SQLITE_TEXT,
            "ax_locator":"tcalibrations:r37"
        }})
    }

    #[test]
    fn sqlite_retrieval_requires_the_exact_native_database_row() {
        assert!(SQLITE_BYTES.starts_with(b"SQLite format 3\0"));
        let response = json!({"hits":{"hits":[fixture_sqlite_hit()]}});
        verify_sqlite_fixture(&response).unwrap();
        for (key, value) in [
            ("id", json!(38)),
            ("id", json!("37")),
            ("text", json!(SQLITE_TOKEN)),
            ("ax_path", json!("other.sqlite3")),
            ("ax_locator", json!("tother:r37")),
        ] {
            let mut wrong = response.clone();
            wrong["hits"]["hits"][0]["_source"][key] = value;
            assert!(verify_sqlite_fixture(&wrong).is_err(), "accepted {key}");
        }
    }

    #[test]
    fn rpc_framing_retains_fragments_and_rejects_errors_and_wrong_ids() {
        let mut wire = br#"{"jsonrpc":"2.0","id":1,"result":{"ok":true}}"#.to_vec();
        assert!(take_rpc_response(&mut wire, 1).unwrap().is_none());
        wire.extend_from_slice(b"\n{\"jsonrpc\":\"2.0\",\"id\":2,\"result\":{}}\n");
        assert_eq!(
            take_rpc_response(&mut wire, 1).unwrap().unwrap()["ok"],
            true
        );
        assert_eq!(take_rpc_response(&mut wire, 2).unwrap().unwrap(), json!({}));
        assert!(wire.is_empty());
        let mut wrong_id = b"{\"jsonrpc\":\"2.0\",\"id\":2,\"result\":{}}\n".to_vec();
        assert!(take_rpc_response(&mut wrong_id, 1).is_err());
        let mut failed = b"{\"jsonrpc\":\"2.0\",\"id\":1,\"error\":{\"code\":-1}}\n".to_vec();
        assert!(take_rpc_response(&mut failed, 1).is_err());
        let mut contaminated = b"startup banner\n".to_vec();
        assert!(take_rpc_response(&mut contaminated, 1).is_err());
    }

    #[test]
    fn app_server_framing_skips_notifications_and_rejects_errors_and_wrong_ids() {
        let mut wire =
            b"{\"method\":\"thread/started\",\"params\":{}}\n{\"id\":2,\"result\":{\"ok\":true}}"
                .to_vec();
        assert!(take_app_server_response(&mut wire, 2).unwrap().is_none());
        wire.push(b'\n');
        assert_eq!(
            take_app_server_response(&mut wire, 2).unwrap().unwrap()["ok"],
            true
        );
        assert!(wire.is_empty());
        let mut wrong_id = b"{\"id\":3,\"result\":{}}\n".to_vec();
        assert!(take_app_server_response(&mut wrong_id, 2).is_err());
        let mut failed = b"{\"id\":2,\"error\":{\"code\":-1}}\n".to_vec();
        assert!(take_app_server_response(&mut failed, 2).is_err());
        let mut request = b"{\"id\":2,\"method\":\"request/approval\"}\n".to_vec();
        assert!(take_app_server_response(&mut request, 2).is_err());
        assert!(app_server_tool_result(json!({"isError":true,"content":[{"text":"{}"}]})).is_err());
        assert!(app_server_tool_result(json!({"content":[{"text":"not JSON"}]})).is_err());
        assert_eq!(
            app_server_tool_result(json!({"content":[{"text":"{\"ok\":true}"}]})).unwrap()["ok"],
            true
        );
    }

    #[test]
    fn default_service_status_requires_reachability_and_health_without_retaining_index_names() {
        let result = json!({"isError":false,"content":[{"text":
            "xerj_status: the node at http://127.0.0.1:9200 is UP (root endpoint answered HTTP 200).\n  cluster health: green\n  indices: private-corpus"
        }]});
        let projected = project_default_service_status(&result).unwrap();
        assert_eq!(projected["clusterHealth"], "green");
        assert!(!projected.to_string().contains("private-corpus"));
        assert!(
            project_default_service_status(&json!({"isError":true,"content":[{"text":"UP"}]}))
                .is_err()
        );
        assert!(project_default_service_status(&json!({"content":[{"text":"xerj_status: the node at http://127.0.0.1:9200 is UP\ncluster health: red"}]})).is_err());
    }

    #[test]
    fn http_parser_requires_complete_bodies_and_decodes_chunked_json() {
        let direct =
            parse_http(b"HTTP/1.1 200 OK\r\nContent-Length: 11\r\n\r\n{\"ok\":true}").unwrap();
        assert_eq!(direct.status, 200);
        assert_eq!(direct.body["ok"], true);
        let readiness = parse_http(b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\n\r\nready").unwrap();
        assert_eq!(readiness.body, "ready");
        assert!(parse_http(b"HTTP/1.1 200 OK\r\nContent-Length: 12\r\n\r\n{\"ok\":true}").is_err());
        let chunked = parse_http(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n5\r\n{\"ok\r\n6\r\n\":true}\r\n0\r\n\r\n");
        assert!(chunked.is_err(), "incorrect chunk boundaries must fail");
        let chunked = parse_http(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n5\r\n{\"ok\"\r\n6\r\n:true}\r\n0\r\n\r\n").unwrap();
        assert_eq!(chunked.body["ok"], true);
        let chunked = parse_http(
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\nb\r\n{\"ok\":true}\r\n0\r\n\r\n",
        )
        .unwrap();
        assert_eq!(chunked.body["ok"], true);
        assert!(decode_chunks(b"b\r\n{\"ok\":true}\r\n0\r\n").is_err());
    }

    #[test]
    fn search_and_recall_require_the_exact_owned_fixture() {
        assert!(verify_search(&json!({"hits":{"hits":[]}})).is_err());
        assert!(
            verify_search(&json!({"hits":{"hits":[{"_id":DOC_ID,"_source":{"text":"wrong"}}]}}))
                .is_err()
        );
        verify_search(&json!({"hits":{"hits":[{"_id":DOC_ID,"_source":{"text":TEXT}}]}})).unwrap();
        assert!(verify_memory(&json!({"hits":[{"id":"other","text":TEXT}]}), HTTP_MEMORY).is_err());
        verify_memory(
            &json!({"hits":[{"id":HTTP_MEMORY,"text":TEXT}]}),
            HTTP_MEMORY,
        )
        .unwrap();
    }

    #[test]
    fn reserved_ports_are_three_distinct_live_loopback_listeners() {
        let (base, reservations) = reserve_ports(1500).unwrap();
        assert_eq!(reservations.len(), 3);
        for port in base..base + 3 {
            assert!(TcpListener::bind(address(port)).is_err());
        }
    }
}
