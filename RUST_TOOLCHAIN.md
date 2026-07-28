# TRUEOS Rust toolchain contract

TRUEOS pins one exact upstream Rust identity:

- toolchain: `nightly-2026-07-10`
- rustc: `1.99.0-nightly`
- rust commit: `af3d95584dbddcae597890340995509a7fb47a50`
- LLVM: `22.1.8`

This is a dated compatibility baseline, not a claim that the toolchain is the
newest available nightly. The installed host `rustc` is retained as the
bootstrap and identity oracle; the native Blueprint compiler driver is built
from the matching compiler sources in the archived toolchain.

## What TRUEOS changes

The Blueprint builder adapts the pinned toolchain's `rust-src` standard-library
sources before invoking `cargo -Z build-std`. The archived compiler sources also
carry narrow `target_os = "trueos"` adaptations for deterministic sysroot
discovery, no-mmap metadata reads, fixed-stack operation, Ctrl-C exclusion, and
static Cranelift selection. The installed host `rustc` binary is not modified.

The active adaptations are the `ensure_rust_std_trueos_*` functions in
`src/main.rs`. They cover the TRUEOS cfg aliases, platform clock, random source,
filesystem/path behavior, thread lifecycle, current-thread binding, and
worker-local storage/TLS. The patched `libc` source remains repository-owned at
`vendor/libc-0.2.186`.

At startup the Blueprint builder verifies the full rustc commit hash before it
may modify `rust-src`. A missing or different toolchain fails closed.

## Native rustc Blueprint

The exact toolchain checkout contains one native compiler appliance,
`rustc-min`. It pins Tokio `=1.52.3` with default features disabled and keeps
rustc at one query worker.

The former med and med+ bring-up packages have been folded into min. Min now
embeds the authenticated target JSON, the current build-std metadata closure,
and the selected TrueOS API payload; selects the static Cranelift backend;
compiles `/common/localcompile/main.rs` to an x86-64 relocatable object;
validates the object and its loader contract; and publishes
`/common/localcompile/local.bp`.

The managed default source is a self-contained Gridpaper one-shot. It constructs
a valid 39×55 page in static zero-filled storage, submits it through the kernel
Gridpaper snapshot ABI, and parks cooperatively on the Blueprint poll/sleep
calls. Static storage avoids making Cranelift compile a 27,885-byte automatic
stack initialization on the target. Keeping this seed monolithic is deliberate:
Cranelift does not support full-graph LTO, and a general source using non-inline
functions from embedded `rmeta` still needs the future in-process
relocatable-link stage.

It uses a current-thread Tokio runtime and places the compiler invocation on a
blocking lane. `-Zthreads=1` keeps rustc's query engine single-worker, while the
Tokio blocking carrier remains a separate parked supervisor. This is not a
general process or IPC model.

The package carries the target JSON both at the explicit `--target` path and at
rustc's host-target sysroot fallback. The builder collects only authenticated
metadata from the current build-std invocation and writes it with an
exact-toolchain manifest into deterministic `.trueos.assets` data. The kernel
validates the bundle, hashes every entry, rejects unsafe paths, and materializes
it under the Blueprint's filesystem root before calling `_start`.

Native compiler packages set the `argv-entry-v1` Blueprint capability. The
kernel calls only those entries as `_start(argc, argv) -> i32`; unflagged
packages retain the legacy no-argument entry ABI. Each compiler wrapper copies
the loader-owned argument strings before it leaves the Hull carrier, and a
nonzero normal return is reported after image cleanup.

The packer treats the root crate's `.rlink` as the authoritative native link
closure. Tokio/std packages must select exactly one `panic_abort` archive and
must not select `panic_unwind`; generic `libunwind` support remains available
for backtraces.

Pack from this repository:

```text
TRUEOS_RUST_TOOLCHAIN_ROOT=/home/t4ce/REPOS/TRUEOS-Rust-Toolchain-nightly-2026-07-10 \
TRUEOS_BLUEPRINT_SKIP_APPS_PUBLISH=1 \
cargo bp /home/t4ce/REPOS/TRUEOS-Rust-Toolchain-nightly-2026-07-10/blueprints/rustc-min
```

The resulting file is `dist/rustc-min.bp`.

## Deliberate boundary

This is a compiler appliance over the no_std TrueOS substrate, not a
conventional hosted OS:

- Compiler filesystem discovery and mutation use the TrueOS async filesystem
  contract. The current filesystem has no symlinks or hard links; compiler
  rename is a single-writer copy-and-remove operation.
- The compiler path does not depend on host `mmap`, stack growth, dynamic
  loading, signals, `fork`, or `exec`. Process-shaped compatibility names that
  can still occur on diagnostic or dormant paths fail explicitly with
  `ENOSYS`/`EPERM` instead of pretending success.
- Min performs parsing, expansion, resolution, typechecking, and relocatable
  x86-64 object emission with statically linked Cranelift. It does not embed the
  LLVM backend or a C++ runtime.
- Final executable linking, Cargo's subprocess-oriented orchestration, a
  general process model, and arbitrary third-party `std` filesystem semantics
  remain outside this first native compiler contract.
- The successful default compiler path returns through `argv-entry-v1`.
  Compiler-fatal diagnostics still use the current `panic=abort` boundary and
  do not yet unwind back into that status path.

## Exact archived toolchain

Set `TRUEOS_RUST_TOOLCHAIN_ROOT` to use an unpacked, exact toolchain archive:

```text
TRUEOS_RUST_TOOLCHAIN_ROOT=/home/t4ce/REPOS/TRUEOS-Rust-Toolchain-nightly-2026-07-10
```

The builder then invokes `bin/cargo` and `bin/rustc` from that root directly;
Cargo is explicitly bound to the same rustc. When the Blueprint checkout and
the dated toolchain checkout are siblings, the builder selects that archive
automatically after confirming that both binaries exist. Otherwise it retains
the pinned rustup fallback.

An explicitly configured archive fails closed: the builder does not fall back
to rustup when the path is missing or incomplete. The selected rustc must report
the archive itself as its sysroot and the full commit hash above.

## Native compiler source

The exact archive carries matching rustc-dev sources at:

```text
lib/rustlib/rustc-src/rust/compiler
```

The builder verifies the compiler identity before exposing this directory and
checks the `rustc_driver_impl`, `rustc_interface`, `rustc_session`, and
`rustc_codegen_cranelift` manifests. It also supplies the release, commit,
date, build-host, requested compiler-host, and bootstrap environment captured
by the upstream compiler build.

This rustc-dev extraction is the compiler crate graph needed by the native
Blueprint path. It is not a complete upstream bootstrap checkout: it has no
top-level Rust workspace or `x.py` bootstrap sources.

## Updating Rust

Treat a nightly update as an explicit port:

1. Install the candidate as a separate dated toolchain with `rust-src`.
2. Keep the existing pinned toolchain installed and working.
3. Validate every source marker used by the standard-library adaptations
   against the candidate's pristine `rust-src`.
4. Build and run the Tokio runtime, filesystem, networking, thread, condvar,
   multi-runtime, and WLS probes.
5. Update `rust-toolchain.toml`, `src/toolchain.rs`, and the identity above in
   one reviewed change.

Do not introduce the floating `nightly` alias into the Blueprint build path.

## Full bootstrap ports

A full upstream bootstrap or compiler-distribution port still requires the
complete Rust source tree at the pinned commit. Keep such work distinct from
the bounded compiler-appliance contract above.
