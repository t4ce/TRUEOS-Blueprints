# Depthchecker factchecker

One standalone Blueprint exercises real entry points in the migrated Codex text, path, transport and protocol crates, then repeats parsing on a native logical thread and exercises filesystem handles. It uses no credentials and sends no network requests. Every successful scenario checks its outputs before emitting its completion marker.

The probe separates three facts: a Cargo package's dependency depth, the source call branches selected by an input, and the physical frames emitted by a particular optimized build. It does not equate those measurements. The seven scenarios cover their specified inputs; passing them does not prove that every Codex operation has a bounded stack or avoids additional executor turns.

## Debugger boundaries and startup gate

These symbols are exported with C calling convention and `#[inline(never)]`:

- `depthchecker_ready()` marks readiness after the initial `READY` log.
- `depthchecker_begin(u64 scenario_id)` arms a scenario interval.
- `depthchecker_end(u64 scenario_id)` marks successful assertions and cleanup.
- `depthchecker_complete(u64 pass_mask)` marks completion of all scenarios.
- `depthchecker_<scenario>_run()` preserves each scenario's entry function.
- `depthchecker_native_worker_run()` preserves the separate native worker entry.

The exported `AtomicU64` values are `DEPTHCHECKER_GATE`, `DEPTHCHECKER_ACTIVE_SCENARIO`, `DEPTHCHECKER_PASS_MASK`, `DEPTHCHECKER_SKIP_MASK`, and `DEPTHCHECKER_COMPLETE_MASK`.

On TRUEOS, the gate starts at zero. After a `READY` log, the probe waits for the debugger to set `DEPTHCHECKER_GATE` to 1. Install begin/end/leaf handlers before opening the gate. The wait performs at most 3,000 sleeps of 20 ms: a nominal 60 seconds, not a hard wall-clock deadline if execution stops in the debugger. Gate failure logs `FAIL stage=debugger_gate` and calls `depthchecker_complete(0)`; use an external wall-clock watchdog as well. Host validation bypasses the gate.

The successful TRUEOS completion mask is `0x7f`; host validation passes `0x6f` and explicitly skips the TRUEOS clock bit with `skip_mask=0x10`. Bit `scenario_id - 1` represents each result. Completion and PASS log lines follow the marker functions; atomics retain results after those calls return. A breakpoint at function entry observes the argument before that function updates its atomic result.

## Scenarios and source-backed expectations

| ID | Scenario / exported entry | Work and result assertions |
| --- | --- | --- |
| 1 | `text_stream` / `depthchecker_text_stream_run` | Four fragments split both plan and citation tags. Checks visible `Intro\nOutro`, citation `doc1`, plan start/end and extracted `- step \n`; checksum 23. |
| 2 | `path_resolution` / `depthchecker_path_resolution_run` | Resolves a POSIX relative path with a space and a home-relative path against explicit base/home facts. Checks normalized URI, native spelling, lexical depth and rejection of an embedded NUL; checksum 56. |
| 3 | `protocol_permissions` / `depthchecker_protocol_permissions_run` | Creates read/write URI permissions, serializes/deserializes the protocol model, checks equality and converts both entries to exact native roots; checksum 68. |
| 4 | `transport_zstd` / `depthchecker_transport_zstd_run` | Prepares a real Codex HTTP request body with Zstd, verifies compressed headers and native decompression roundtrip, then checks that preparing the already prepared body retains the wire bytes; checksum 84 in the validated host build. |
| 5 | `cabi_clock` / `depthchecker_cabi_clock_run` | TRUEOS SDK `Instant::now` and monotonic nanoseconds calls; checks nondecreasing values and valid elapsed duration. Host skips this ABI path rather than substituting a host clock. |
| 6 | `native_thread` / `depthchecker_native_thread_run` | Spawns a named `std::thread`, runs the same fragmented parser 32 times inside `depthchecker_native_worker_run`, then performs the filesystem roundtrip and checks its result of 35 bytes before joining. The returned parser checksum remains 736. The begin/end interval includes spawn, native parsing/file I/O and join. |
| 7 | `filesystem` / `depthchecker_filesystem_run` | Creates a new app-scoped file, writes/flushes/syncs it, checks metadata, seeks and reads through the same descriptor, reads again through a typed Codex path, removes it and checks NotFound; checksum 35. |

Scenario 4 stores a reserved `example.invalid` URL as a request field; it never constructs or sends a network request. Its payload compresses well, and the assertions compare the exact decompressed original JSON bytes rather than only comparing compressed length.

Scenario 7 uses `/depthchecker_factchecker.txt` in the Blueprint's app filesystem scope. `create_new` fails rather than overwriting an existing fixture. Successful execution removes the fixture. An interrupted or failing run can leave that file; inspect and remove only that known fixture before repeating the probe. Host execution uses a PID-suffixed file in `std::env::temp_dir()`.

The same filesystem function also runs on the native worker during scenario 6, and removes its fixture before the worker returns and the Hull's scenario 7 begins. There are no nested scenario markers. This makes native continuation parking observable under the real worker's module ancestry. The ordinary Hull filesystem path uses its existing VMcall bridge; a native `trueos_thread_context_swap` is not promised for that Hull path. File synchronization remains an assertion in both lanes; an error is reported as a failure, never skipped to obtain capture coverage.

### 1 and 6: text parser and physical standard-library leaves

Entry: [`AssistantTextStreamParser::push_str`](../../../codex/codex-rs/utils/text/src/stream_parser/assistant_text.rs). Citation processing invokes `CitationStreamParser` and `InlineHiddenTagParser::push_str`; plan processing invokes `ProposedPlanParser` and `TaggedLineParser::parse`.

[`InlineHiddenTagParser::find_next_open`](../../../codex/codex-rs/utils/text/src/stream_parser/inline_hidden_tag.rs) searches the pending string with `str::find(spec.open)`. A split open tag takes its suffix/prefix buffering branch; the final citation fragment takes the active-close `pending.find(close)` branch. Those inputs select real string searching, byte slicing and buffer growth. The actual native ELF retains the text crate's `str::find` instantiation and its physical call to `StrSearcher::new`; the subsequent `next_match` search algorithm is inlined in that `find` body. Separate `next_match` symbols elsewhere in the ELF are not proof that this fixture calls them.

`codex-utils-text` has **zero ordinary non-standard-library dependencies**. It is therefore a terminal source package in an ordinary Cargo dependency graph. Physical calls continue into `core`, `alloc`, `std`, and the selected allocator. With the Blueprint SDK's default allocator, buffer growth can call `trueos_cabi_alloc` or `trueos_cabi_realloc`; aligned growth can call `sys_alloc_aligned`. Such an allocation may be the first kernel leaf captured, before the deeper parsing branch. It is a genuine selected branch, but it is not evidence that a different string-search terminal was reached.

Scenario 6 adds real `std::thread` spawn/join edges around the same parser and filesystem work. The archived TRUEOS standard library's [`sys/thread/trueos.rs`](../../../TRUEOS-Rust-Toolchain-nightly-2026-07-10/lib/rustlib/src/rust/library/std/src/sys/thread/trueos.rs) calls `trueos_cabi_thread_spawn` and `trueos_cabi_thread_join`. Its thread entry initializes logical-thread state before invoking the worker closure. The parent and worker have distinct physical stacks; report the observed native continuation separately from the caller's Hull stack. The worker's filesystem calls can select the existing native parking/context-swap branch; runtime captures confirm whether they do. No artificial leaf callback is inserted into the parser or filesystem code.

### 2: paths and URL parsing, without an IDNA or DNS branch

Entry: [`PathUri::resolve_config_path_uri`](../../../codex/codex-rs/utils/paths/src/path_uri/config_path.rs). It validates input text and supplied path facts, resolves `LegacyAppPathString` against the base/home, validates the result and normalizes it with `PathUri::join(".")`.

The fixture `../shared/a b.txt` takes the relative-resolution branch; `~/notes/../facts.txt` takes the home-relative branch. URI construction and parsing use `url::Url`; `decoded_path_bytes` performs percent decoding. The native ELF retains the file-URI branch `Parser::parse_url` → `parse_file` → regular `parse_path` → `push_pending`, with real relocations between the latter functions. These supply deeper physical capture candidates than the public URL entry. Percent-decoding helpers may be inlined, so an absent standalone symbol must be reported as such.

The supplied `file:///...` URIs have no hostname. These paths do not select URL host IDNA processing, DNS, certificate lookup, or HTTP I/O, even when packages supporting those operations exist in the full dependency graph. Filesystem canonicalization is not used in this scenario.

### 3: protocol permissions and serde parsing

Entry: [`FileSystemPermissions::from_read_write_path_uris`](../../../codex/codex-rs/protocol/src/models.rs), followed by `serde_json::to_vec` and `serde_json::from_slice` of that actual public model. Generated serde code restores the two permission entries and their typed `PathUri` fields. The path deserializer parses file URIs; `legacy_read_write_roots` invokes `as_legacy_permissions`, `PathUri::to_abs_path`, and the typed absolute-path constructor.

The native ELF retains `serde_json::read::SliceRead::parse_str` and its real call to the emitted `skip_to_escape` clone. The quoted permission keys and values make these string-parsing functions useful physical candidates. `FileSystemPermissions::as_legacy_permissions`, the URI parser below a path field, and standard-library buffer allocation are additional selected branches. Which branch is deepest requires observed calls. Protocol dependencies on network policy, image handling and other models are not a claim that those modules execute in this permission fixture.

### 4: Codex transport into actual native Zstd

Entry: [`Request::into_prepared`](../../../codex/codex-rs/transport/src/http/request.rs). The probe supplies `EncodedJsonBody`, so it selects `prepare_body_for_send` → `prepare_encoded_json` → `zstd::stream::encode_all(..., 3)`. The encoder proceeds through `zstd-safe` to `zstd-sys`'s real C implementation. Physical native leaf candidates are `ZSTD_compressStream2` and, during verification, `ZSTD_decompressStream`. Both were present as local text symbols in the host ELF. No new callback is introduced into the C codec.

The compression path also records a standard-library `Instant`, so native monotonic time may appear here independently of scenario 5. Provider URL building, HTTP sockets, TLS negotiation and transport retries are not selected by this fixture. After `into_prepared`, the second preparation checks the cached-body branch instead of compressing again.

### 5: direct TRUEOS SDK ABI import

Entry: [`trueos::clock::Instant::now` / `monotonic_nanos`](../../crates/trueos-v/src/vclock.rs), which invoke the existing `trueos_time_monotonic_nanos` import. The kernel implementation is in [`std_abi_shim.rs`](../../../TRUEOS/src/std_abi_shim.rs). This boundary is expected to be explicit in the target import table even if the SDK wrapper is inlined. Its host skip is part of the recorded result, not a passing substitute.

### 7: standard-library handles into native services

`std::fs::OpenOptions` and descriptor operations use the archived standard library's [`sys/fs/unix.rs`](../../../TRUEOS-Rust-Toolchain-nightly-2026-07-10/lib/rustlib/src/rust/library/std/src/sys/fs/unix.rs) and [`sys/fd/unix.rs`](../../../TRUEOS-Rust-Toolchain-nightly-2026-07-10/lib/rustlib/src/rust/library/std/src/sys/fd/unix.rs). Source-backed service candidates are `open`/`open64`, `write`, `read`, `fsync`, `fstat`/`fstat64`, and `lseek`. They resolve to the kernel's POSIX compatibility functions, not a host Linux syscall implementation.

TRUEOS removal uses `trueos_cabi_async_fs_remove_start`, status/discard and, while pending, `trueos_cabi_poll_once`. The fixture exercises that existing asynchronous-backed standard-library deletion path. A physical backtrace at a service entry can identify the current caller and kernel continuation; it cannot reconstruct all earlier or future executor turns from one stack. Keep physical frames, inline DWARF frames and asynchronous boundaries distinct in the capture report.

## Build and validation scope

The manifest has its own `[workspace]`, path dependencies on the current migrated Codex crates, and the same required implementation pins for HTTP dates, rustls-rustcrypto and Rama that Codex's workspace uses. The Blueprint packer supplies its remaining audited target overlays. The probe does not require a new member in the main Blueprints Cargo workspace.

`[package.metadata.trueos-blueprint] stack-capture = true` opts into the diagnostic packer's separate Cargo cache, full DWARF, Rust frame pointers, retained local symbols and coalesced sections. Native C/C++ receives `-g -fno-omit-frame-pointer` after the existing platform flags. The release profile uses `opt-level=1`, `debug=2`, `lto=false`, `codegen-units=8`, and no debug stripping in the unstripped sidecar. Normal Blueprint packages keep their existing debug stripping; this probe changes no production profile. These settings retain scenario boundaries; they do not guarantee that every dependency helper has a separate physical frame.

The actual diagnostic pack succeeded with publication disabled. Its sidecars are `dist/depthchecker_factchecker.debug.o`, `dist/depthchecker_factchecker.packed.o`, and `dist/depthchecker_factchecker.debug.json`. The JSON binds the Blueprint and both ELF files with SHA-256 hashes. `native-unit-graph.json` and `native-link-inputs.json` are retained under `probes/depthchecker_factchecker/target/trueos-blueprint/work/depthchecker_factchecker/`.

### Actual emitted native symbols

[`validation/native-elf-symbols.json`](validation/native-elf-symbols.json) records exact mangled names, sizes, bindings and section offsets from the completed native diagnostic ELF, tied to the sidecar hashes. It also records the undefined service imports for scenarios 5–7. It is a static inventory, not proof of runtime hits. [`validation/refresh-native-inventory.py`](validation/refresh-native-inventory.py) verifies those hashes, confirms staged `main.rs` differs only by the packer's `no_main`/`_start` entry augmentation, and refreshes every curated symbol from `nm -S`; a missing named symbol fails the refresh.

| Selected physical candidate | Defined `.text` offset |
| --- | ---: |
| Text crate's `<str>::find::<&str>` instantiation | `0x164b90` |
| `StrSearcher::new` | `0x4c42a0` |
| URL `Parser::parse_file` | `0x414980` |
| URL regular `Parser::parse_path` | `0x4161c0` |
| URL `parse_path::push_pending` | `0x41a530` |
| Serde `SliceRead::parse_str` | `0x171a40` |
| Serde `SliceRead::skip_to_escape` emitted LLVM clone | `0x171640` |
| Permission model `as_legacy_permissions` | `0xbc960` |
| Native `ZSTD_compressStream2` | `0x64b1c` |
| Native `ZSTD_decompressStream` | `0xdc83` |

The text parser's emitted `push_str` has three real relocations to its own `str::find` instantiation. That `find` body physically calls `StrSearcher::new` and **inlines the `next_match` search algorithm**. Two separate `StrSearcher::next_match` symbols also exist elsewhere in this ELF, but they are not direct callees of this text `find` body. Selecting those unrelated instantiations by symbol name alone can therefore produce zero hits. Prefer the actual `StrSearcher::new`/text `find` branch and report its inline search work separately.

The retained URL `parse_file` body has a real relocation to regular `parse_path`; `parse_path` has real relocations to `push_pending`. `SliceRead::parse_str` has a real relocation to the retained `skip_to_escape` LLVM clone. These static call edges substantiate useful capture candidates; a runtime hit confirms which fixture branch reaches them.

All offsets above belong to `.text` in a **relocatable** ELF, and require the actual loader's section mapping. Undefined imports such as `trueos_time_monotonic_nanos`, `trueos_cabi_thread_spawn`, `fsync`, and `read` use addresses in the matching kernel ELF. They must not be interpreted as offset zero from the Blueprint's text base.

Host validation completed with Rust 1.95.0: one integration-style test ran all six portable scenarios successfully, the native worker joined, the file was removed, and the completion atomics held `pass_mask=0x6f`, `skip_mask=0x10`, `active=0`. See [`validation/host-result.md`](validation/host-result.md).

After adding native-worker filesystem work, the same host test passed again with both sequential filesystem roundtrips, without changing the parser checksum or completion mask. Its raw output is retained in [`validation/host-native-filesystem.log`](validation/host-native-filesystem.log). The updated native diagnostic pack also succeeded with jobs 2 and publication disabled; its log is `target/depthchecker-pack-native-fs.log`. The symbol inventory and offsets above bind that updated artifact and its staged source.

The initial native run reached real `fsync` but failed with `NotFound` after a successful write-begin. Diagnosis found that the standard ABI metadata helper queried private Hull filesystem mounts after the host service had committed the file; those mounts were empty. The kernel helper was corrected to use the existing metadata-v2 CABI in Hulls. The probe preserves the synchronization assertion in both lanes. The completed guest runs below verify those fixture operations against the corrected kernel.

### Observed guest runtime and native stack fold

Both the baseline `run-final` and follow-up `run-stack-fold` completed all seven scenarios with mask `0x7f` and all 14 selected first-hit captures. Every capture reached its recognized physical root, had entry `RSP % 16 == 8`, and retained its full requested raw stack interval with zero unreadable bytes. Hull captures ended at `trueos_vmx_guest_entry`; the three native-worker captures ended at `trueos_thread_context_start`. See [`validation/runtime-result.md`](validation/runtime-result.md), the sibling TRUEOS [experiment notes](../../../TRUEOS/tools/docs/depthchecker-factchecker-2026-10-14.md), and [stack-fold overview](../../../TRUEOS/bld/depthchecker_factchecker/report-stack-fold/overview.md).

The native parking capture exposed the guest-only 160 KiB file buffer inlined into the shared read ABI, reserving stack space even on its native host branch. Preventing inlining of `guest_fs_read_file` in the kernel reduced the captured native `RSP`-to-stack-end interval from **166,824 to 3,048 bytes**, a decrease of 163,776 bytes. The largest adjacent frame-pointer gap fell from 163,936 to 480 bytes; the shared read ABI's particular gap fell to 160 bytes. The physical chain remained 19 frames. Both runs used the same Blueprint payload and capture plan; only the matching kernel image changed. This is measured stack-byte reduction on that native branch, not a crate-count or call-depth reduction.

The launch reports RAM root 2 and VM 0. Kernel ELF ownership of a function is separate from where it executed: `scoped_identity.context` distinguishes the L2 Hull snapshot from a host-carried native continuation. The worker was observed as logical thread 1 on carrier/AP slot 2 with its active stack contained in the recorded native bounds. The debugger captured selected function entries before their prologues, preserving the immediate caller at `[RSP]`; it did not discard caller evidence by beginning only at the current `RBP`. Each target is captured on its first accepted hit inside its scenario interval, with unrelated kernel hits excluded by the probe-ancestry check. These snapshots establish those selected paths and physical roots, not an exhaustive trace, maximum stack use across every branch, or a complete history of asynchronous turns.

To avoid rebuilding the large dependency graph during a concurrent native diagnostic build, that test was compiled directly with `rustc --test` against existing Codex Cargo host `.rlib` artifacts, selected by their most recent modification times and used read-only. Rust accepted and linked those artifacts. Separately, the standalone manifest's `cargo metadata --offline` resolution succeeded with its own lockfile. **This was not a fresh Cargo host dependency build**, and it does not validate the TRUEOS clock, target overlays, loader or kernel service behavior. The native diagnostic pack and runtime captures provide those additional checks.

For a fresh host Cargo validation, run from outside the repositories so no parent TRUEOS target config is inherited:

```sh
cd /tmp
CARGO_TARGET_DIR=/tmp/depthchecker-factchecker-host-target \
  cargo +1.95.0 test --offline \
  --manifest-path /home/t4ce/Repos/TRUEOS-Blueprints/probes/depthchecker_factchecker/Cargo.toml \
  -- --nocapture
```
