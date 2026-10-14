//! Deterministic Codex entry paths with debugger-visible scenario boundaries.
//!
//! Dependency depth is not stack depth. These scenarios perform real work and
//! expose intervals in which a debugger can observe the emitted call frames.

use std::hint::black_box;
use std::sync::atomic::{AtomicU64, Ordering, compiler_fence};

use codex_protocol::models::FileSystemPermissions;
use codex_transport::http::{EncodedJsonBody, Request, RequestBody, RequestCompression};
use codex_utils_paths::absolute_path::AbsolutePathBuf;
use codex_utils_paths::path_uri::{PathConvention, PathUri};
use codex_utils_text::stream_parser::{AssistantTextStreamParser, ProposedPlanSegment};

#[unsafe(no_mangle)]
pub static DEPTHCHECKER_GATE: AtomicU64 = AtomicU64::new(0);
#[unsafe(no_mangle)]
pub static DEPTHCHECKER_ACTIVE_SCENARIO: AtomicU64 = AtomicU64::new(0);
#[unsafe(no_mangle)]
pub static DEPTHCHECKER_PASS_MASK: AtomicU64 = AtomicU64::new(0);
#[unsafe(no_mangle)]
pub static DEPTHCHECKER_SKIP_MASK: AtomicU64 = AtomicU64::new(0);
#[unsafe(no_mangle)]
pub static DEPTHCHECKER_COMPLETE_MASK: AtomicU64 = AtomicU64::new(0);

/// Set DEPTHCHECKER_GATE to 1 after installing begin/end breakpoint handlers.
#[unsafe(no_mangle)]
#[inline(never)]
pub extern "C" fn depthchecker_ready() {
    compiler_fence(Ordering::SeqCst);
    black_box(DEPTHCHECKER_GATE.load(Ordering::SeqCst));
}

/// The debugger arms its real leaf/frame capture at this exported boundary.
#[unsafe(no_mangle)]
#[inline(never)]
pub extern "C" fn depthchecker_begin(id: u64) {
    DEPTHCHECKER_ACTIVE_SCENARIO.store(id, Ordering::SeqCst);
    compiler_fence(Ordering::SeqCst);
    black_box(id);
}

/// Called only after that scenario's result assertions and resource cleanup.
#[unsafe(no_mangle)]
#[inline(never)]
pub extern "C" fn depthchecker_end(id: u64) {
    compiler_fence(Ordering::SeqCst);
    if (1..=7).contains(&id) {
        DEPTHCHECKER_PASS_MASK.fetch_or(1 << (id - 1), Ordering::SeqCst);
    }
    DEPTHCHECKER_ACTIVE_SCENARIO.store(0, Ordering::SeqCst);
    black_box(id);
}

#[unsafe(no_mangle)]
#[inline(never)]
pub extern "C" fn depthchecker_complete(pass_mask: u64) {
    DEPTHCHECKER_COMPLETE_MASK.store(pass_mask, Ordering::SeqCst);
    compiler_fence(Ordering::SeqCst);
    black_box(pass_mask);
}

fn report(message: impl std::fmt::Display) {
    #[cfg(target_os = "trueos")]
    trueos::logl::log(trueos::logl::level::INFO, format_args!("{message}"));
    #[cfg(not(target_os = "trueos"))]
    println!("{message}");
}

#[inline(never)]
fn parse_fragmented_text() -> u64 {
    let mut parser = AssistantTextStreamParser::new(true);
    let fragments = [
        "Intro\n<proposed",
        "_plan>\n- step <oai-mem-cit",
        "ation>doc1</oai-mem-citation>\n",
        "</proposed_plan>\nOutro",
    ];
    let mut visible = String::new();
    let mut citations = Vec::new();
    let mut plan = Vec::new();
    for fragment in fragments {
        let chunk = parser.push_str(black_box(fragment));
        visible.push_str(&chunk.visible_text);
        citations.extend(chunk.citations);
        plan.extend(chunk.plan_segments);
    }
    let final_chunk = parser.finish();
    visible.push_str(&final_chunk.visible_text);
    citations.extend(final_chunk.citations);
    plan.extend(final_chunk.plan_segments);
    assert_eq!(visible, "Intro\nOutro");
    assert_eq!(citations, ["doc1"]);
    assert!(plan.contains(&ProposedPlanSegment::ProposedPlanStart));
    assert!(plan.contains(&ProposedPlanSegment::ProposedPlanEnd));
    let plan_text: String = plan
        .iter()
        .filter_map(|segment| match segment {
            ProposedPlanSegment::ProposedPlanDelta(text) => Some(text.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(plan_text, "- step \n");
    black_box((visible.len() + citations[0].len() + plan_text.len()) as u64)
}

/// Scenario 1: migrated text crate -> citation/plan parsers -> real string work.
#[unsafe(no_mangle)]
#[inline(never)]
pub extern "C" fn depthchecker_text_stream_run() -> u64 {
    parse_fragmented_text()
}

/// Scenario 2: path configuration resolution -> typed URI -> URL/percent parsing.
#[unsafe(no_mangle)]
#[inline(never)]
pub extern "C" fn depthchecker_path_resolution_run() -> u64 {
    let base = PathUri::parse(black_box("file:///depthchecker/project")).unwrap();
    let home = PathUri::parse(black_box("file:///depthchecker/home")).unwrap();
    let resolved = PathUri::resolve_config_path_uri(
        black_box("../shared/a b.txt"),
        PathConvention::Posix,
        Some(&base),
        Some(&home),
    )
    .unwrap();
    assert_eq!(
        resolved.to_string(),
        "file:///depthchecker/shared/a%20b.txt"
    );
    assert_eq!(
        resolved
            .to_config_path_string(PathConvention::Posix)
            .unwrap(),
        "/depthchecker/shared/a b.txt"
    );
    assert_eq!(resolved.lexical_depth(), Some(3));
    let home_relative = PathUri::resolve_config_path_uri(
        "~/notes/../facts.txt",
        PathConvention::Posix,
        Some(&base),
        Some(&home),
    )
    .unwrap();
    assert_eq!(
        home_relative.to_string(),
        "file:///depthchecker/home/facts.txt"
    );
    assert!(
        PathUri::resolve_config_path_uri(
            "bad\0path",
            PathConvention::Posix,
            Some(&base),
            Some(&home),
        )
        .is_err()
    );
    black_box((resolved.decoded_path_bytes().len() + home_relative.encoded_path().len()) as u64)
}

/// Scenario 3: higher-level permission model -> serde -> typed path conversion.
#[unsafe(no_mangle)]
#[inline(never)]
pub extern "C" fn depthchecker_protocol_permissions_run() -> u64 {
    let read = PathUri::parse("file:///depthchecker/read%20root").unwrap();
    let write = PathUri::parse("file:///depthchecker/write").unwrap();
    let permissions =
        FileSystemPermissions::from_read_write_path_uris(Some(vec![read]), Some(vec![write]));
    let wire = serde_json::to_vec(black_box(&permissions)).unwrap();
    let restored: FileSystemPermissions = serde_json::from_slice(black_box(&wire)).unwrap();
    assert_eq!(permissions, restored);
    assert_eq!(restored.entries.len(), 2);
    let legacy = restored.legacy_read_write_roots().unwrap();
    assert_eq!(
        legacy.read.as_ref().unwrap()[0].as_path(),
        std::path::Path::new("/depthchecker/read root")
    );
    assert_eq!(
        legacy.write.as_ref().unwrap()[0].as_path(),
        std::path::Path::new("/depthchecker/write")
    );
    black_box(wire.len() as u64)
}

/// Scenario 4: real Codex HTTP preparation -> JSON -> native Zstd C implementation.
/// No request is sent; the reserved example.invalid URL is only a request field.
#[unsafe(no_mangle)]
#[inline(never)]
pub extern "C" fn depthchecker_transport_zstd_run() -> u64 {
    let payload = serde_json::json!({
        "model": "depthchecker-fixture",
        "input": "real transport compression path ".repeat(64),
    });
    let encoded = EncodedJsonBody::encode(black_box(&payload)).unwrap();
    let expected = encoded.as_bytes().to_vec();
    let mut request = Request::new(
        http::Method::POST,
        "https://example.invalid/v1/responses".into(),
    )
    .with_compression(RequestCompression::Zstd);
    request.body = Some(RequestBody::EncodedJson(encoded));
    let prepared = black_box(request).into_prepared().unwrap();
    assert_eq!(prepared.compression, RequestCompression::None);
    assert_eq!(prepared.headers[http::header::CONTENT_ENCODING], "zstd");
    assert_eq!(
        prepared.headers[http::header::CONTENT_TYPE],
        "application/json"
    );
    let Some(RequestBody::EncodedJson(body)) = prepared.body.as_ref() else {
        panic!("Codex transport must retain the prepared encoded JSON body");
    };
    assert!(body.as_bytes().len() < expected.len());
    let decoded =
        zstd::stream::decode_all(std::io::Cursor::new(black_box(body.as_bytes()))).unwrap();
    assert_eq!(decoded, expected);
    let repeated = prepared.prepare_body_for_send().unwrap();
    assert_eq!(repeated.body_bytes().as_ref(), body.as_bytes());
    black_box(body.as_bytes().len() as u64)
}

/// Scenario 5: SDK clock -> real imported TRUEOS monotonic-time C ABI leaf.
#[unsafe(no_mangle)]
#[inline(never)]
pub extern "C" fn depthchecker_cabi_clock_run() -> u64 {
    #[cfg(target_os = "trueos")]
    {
        let first = black_box(trueos::clock::Instant::now());
        let nanos_first = black_box(trueos::clock::monotonic_nanos());
        let nanos_second = black_box(trueos::clock::monotonic_nanos());
        let second = black_box(trueos::clock::Instant::now());
        assert!(nanos_second >= nanos_first);
        assert!(second.checked_duration_since(first).is_some());
        black_box(nanos_second - nanos_first)
    }
    #[cfg(not(target_os = "trueos"))]
    {
        // A host clock call would not validate the imported TRUEOS ABI path.
        0
    }
}

/// Stable worker entry for observing native thread continuation independently.
#[unsafe(no_mangle)]
#[inline(never)]
pub extern "C" fn depthchecker_native_worker_run() -> u64 {
    let mut total = 0;
    for _ in 0..black_box(32) {
        total += parse_fragmented_text();
    }
    assert_eq!(total, 32 * 23);
    black_box(total)
}

/// Scenario 6 spans spawn, worker's real parser work, and join.
#[unsafe(no_mangle)]
#[inline(never)]
pub extern "C" fn depthchecker_native_thread_run() -> u64 {
    let worker = std::thread::Builder::new()
        .name("depthchecker-native".into())
        .spawn(|| depthchecker_native_worker_run())
        .unwrap();
    let result = worker.join().unwrap();
    assert_eq!(result, 736);
    result
}

/// Scenario 7 performs real file-handle writes, sync, seek, read and deletion.
#[unsafe(no_mangle)]
#[inline(never)]
pub extern "C" fn depthchecker_filesystem_run() -> u64 {
    use std::io::{Read, Seek, SeekFrom, Write};
    #[cfg(target_os = "trueos")]
    let path = std::path::PathBuf::from("/depthchecker_factchecker.txt");
    #[cfg(not(target_os = "trueos"))]
    let path = std::env::temp_dir().join(format!(
        "depthchecker_factchecker-{}.txt",
        std::process::id()
    ));
    let contents = black_box(b"depthchecker native file roundtrip\n");
    // create_new avoids overwriting a fixture from another invocation.
    let mut file = std::fs::OpenOptions::new()
        .create_new(true)
        .read(true)
        .write(true)
        .open(&path)
        .unwrap();
    file.write_all(contents).unwrap();
    file.flush().unwrap();
    file.sync_all().unwrap();
    assert_eq!(file.metadata().unwrap().len(), contents.len() as u64);
    file.seek(SeekFrom::Start(0)).unwrap();
    let mut read = Vec::new();
    file.read_to_end(&mut read).unwrap();
    assert_eq!(read.as_slice(), contents);
    drop(file);
    let typed = AbsolutePathBuf::from_absolute_path_checked(&path).unwrap();
    assert_eq!(std::fs::read(typed.as_path()).unwrap(), contents);
    std::fs::remove_file(typed.as_path()).unwrap();
    assert_eq!(
        std::fs::metadata(typed.as_path()).unwrap_err().kind(),
        std::io::ErrorKind::NotFound
    );
    black_box(read.len() as u64)
}

fn run_one(id: u64, name: &str, operation: extern "C" fn() -> u64) {
    report(format_args!(
        "depthchecker_factchecker: BEGIN id={id} scenario={name}"
    ));
    depthchecker_begin(id);
    let checksum = black_box(operation)();
    depthchecker_end(id);
    report(format_args!(
        "depthchecker_factchecker: PASS id={id} scenario={name} checksum={checksum}"
    ));
}

fn run_scenarios() -> u64 {
    run_one(1, "text_stream", depthchecker_text_stream_run);
    run_one(2, "path_resolution", depthchecker_path_resolution_run);
    run_one(
        3,
        "protocol_permissions",
        depthchecker_protocol_permissions_run,
    );
    run_one(4, "transport_zstd", depthchecker_transport_zstd_run);
    #[cfg(target_os = "trueos")]
    run_one(5, "cabi_clock", depthchecker_cabi_clock_run);
    #[cfg(not(target_os = "trueos"))]
    {
        DEPTHCHECKER_SKIP_MASK.fetch_or(1 << 4, Ordering::SeqCst);
        report(
            "depthchecker_factchecker: SKIP id=5 scenario=cabi_clock reason=host_has_no_TRUEOS_CABI",
        );
    }
    run_one(6, "native_thread", depthchecker_native_thread_run);
    run_one(7, "filesystem", depthchecker_filesystem_run);
    let passed = DEPTHCHECKER_PASS_MASK.load(Ordering::SeqCst);
    let skipped = DEPTHCHECKER_SKIP_MASK.load(Ordering::SeqCst);
    assert_eq!(passed | skipped, 0x7f);
    assert_eq!(passed & skipped, 0);
    depthchecker_complete(passed);
    report(format_args!(
        "depthchecker_factchecker: COMPLETE pass_mask={passed:#x} skip_mask={skipped:#x}"
    ));
    passed
}

fn wait_for_debugger() -> bool {
    #[cfg(target_os = "trueos")]
    {
        report("depthchecker_factchecker: READY gate=0 set_DEPTHCHECKER_GATE_to_1");
        depthchecker_ready();
        for _ in 0..3000 {
            if DEPTHCHECKER_GATE.load(Ordering::SeqCst) != 0 {
                return true;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        report("depthchecker_factchecker: FAIL stage=debugger_gate nominal_timeout_ms=60000");
        depthchecker_complete(0);
        false
    }
    #[cfg(not(target_os = "trueos"))]
    {
        DEPTHCHECKER_GATE.store(1, Ordering::SeqCst);
        true
    }
}

fn main() {
    if wait_for_debugger() {
        run_scenarios();
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn all_portable_entry_paths_complete_with_assertions() {
        super::DEPTHCHECKER_GATE.store(1, super::Ordering::SeqCst);
        assert_eq!(super::run_scenarios(), 0x6f);
        assert_eq!(
            super::DEPTHCHECKER_COMPLETE_MASK.load(super::Ordering::SeqCst),
            0x6f
        );
        assert_eq!(
            super::DEPTHCHECKER_ACTIVE_SCENARIO.load(super::Ordering::SeqCst),
            0
        );
    }
}
