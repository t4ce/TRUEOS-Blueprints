# Resident tracing on TRUEOS

Blueprint materialization permanently selects the six sources listed in
[`tracing-resident-versions.json`](../vendor/tracing-resident-versions.json).
These are the latest stable crates.io releases verified on 2026-10-08; each
upstream archive was checked against its crates.io SHA-256. Original licenses,
source, and Cargo.toml.orig are retained. Linux and other targets retain upstream
behavior. Native branches use `target_os = "trueos"`; `trueos_tracing_test` is
only a host-test switch.

## Transport and ownership

`tracing-core` supplies the resident default collector before initialization.
Every public Dispatch selects this collector. Blueprint API trace helpers delegate
to it, including the resident event counter, without installing another subscriber. Tracing macros and attributes,
subscriber initializers, and tracing-log's legacy log bridge converge on it.
Subscriber layers, RUST_LOG, and tracing's compile-time max-level features do
not filter native events; log_os owns the policy. Native subscriber initialization
also installs the legacy log bridge when available. `log-always` cannot duplicate
tracing events through that bridge. Explicit application calls to log crate's
`set_max_level` or another logger remain outside tracing's control.

ERROR/WARN/INFO/DEBUG/TRACE map to log_os levels 1/2/3/4/5. Records use the existing
`trueos_cabi_log` boundary, Apps area, and preserve the original target as a label.
Messages and typed fields become bounded, single-line UTF-8 text (1024 bytes,
256-byte target). Oversized records end in `...`; control characters become spaces.
There is no tracing-owned file, queue, drain thread, or stdout sink.

Span IDs, parent references, enter/exit context, clones, and recorded fields are
retained. Lifecycle/follows-from markers are TRACE; events include up to eight
parent contexts. Stored fields are bounded to 256 bytes per span; later records
append updates, with the latest value last, until truncation. Formatting occurs
outside the span-state lock. Recursive events caused by Debug formatting are
suppressed on that carrier. Thread context uses the kernel carrier ID.

The enabled query is flag bit 31 on the existing logging ABI, not a new symbol.
The updated kernel reports the Apps policy before field expressions/formatting.
Old kernels reject this optional query; records still pass through their normal
host filter. Therefore old kernels work, but do not gain early formatting avoidance.
The kernel changes require deployment to obtain that optimization.

Native rolling and nonblocking appender APIs remain callable but never open files
or spawn workers. Direct Write calls lack metadata and map to INFO under target
`tracing-appender`. Flush/WorkerGuard are no-ops and the dropped-lines counter is
zero (no queue). A caller-provided writer is dropped without writing; arbitrary
application filesystem calls and writer destructors are outside this replacement.
Voxy's shared frontend skips its own legacy file setup on TRUEOS as well.

## Validation and updates

From the Blueprint repository:

```sh
RUSTFLAGS='--cfg trueos_tracing_test' cargo run --release --manifest-path tests/tracing-resident/Cargo.toml
RUSTFLAGS='--cfg trueos_tracing_test' cargo check --manifest-path tests/tracing-resident/core-no-std/Cargo.toml
cargo test --bin trueos-blueprint resident_tracing_pins_replace_application_aliases
cargo bp voxy
```

The transport test supplies a fake host ABI to verify levels/targets, early native
filtering, initialization independence, compile-time-filter override in release,
no log-always duplicates, span retention, updates, thread isolation, recursive
formatting, UTF-8 bounds, legacy log bridging, and absence of file/worker writes.
It is not a physical-rig test. The kernel is separately built with `cargo build`.

For updates, verify latest stable releases and archive checksums, vendor the full
upstream sources, carry forward native branches, update `TRACING_VENDOR_PATCHES`
and the version manifest, and rerun these checks. Do not replace upstream code
for desktop targets or remove the native sink during version alignment.

Desktop core validation: 19 upstream unit tests pass. The remaining
`level_filter_reprs` test also fails with the unmodified upstream metadata source
on the installed nightly-2026-07-10 toolchain (OFF niche representation changed).
Native current-level handling avoids that representation shortcut; upstream
non-TRUEOS code and its test are retained rather than silently changing them.
