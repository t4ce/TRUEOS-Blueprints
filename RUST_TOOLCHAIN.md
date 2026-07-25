# TRUEOS Rust toolchain contract

TRUEOS currently uses an unmodified upstream Rust compiler:

- toolchain: `nightly-2026-07-10`
- rustc: `1.99.0-nightly`
- rust commit: `af3d95584dbddcae597890340995509a7fb47a50`
- LLVM: `22.1.8`

This is a dated compatibility baseline, not a claim that the toolchain is the
newest available nightly.

## What TRUEOS changes

The Blueprint builder adapts the pinned toolchain's `rust-src` standard-library
sources before invoking `cargo -Z build-std`. It does not patch or replace the
`rustc` compiler binary.

The active adaptations are the `ensure_rust_std_trueos_*` functions in
`src/main.rs`. They cover the TRUEOS cfg aliases, platform clock, random source,
filesystem/path behavior, thread lifecycle, current-thread binding, and
worker-local storage/TLS. The patched `libc` source remains repository-owned at
`vendor/libc-0.2.186`.

At startup the Blueprint builder verifies the full rustc commit hash before it
may modify `rust-src`. A missing or different toolchain fails closed.

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

## Future native compiler port

A native TRUEOS rustc port should begin from the complete upstream Rust source
tree at the pinned commit. Its compiler/bootstrap changes belong in that source
tree as an explicit port series. The standard-library build adaptations here
must not be described or reused as a patch to rustc itself.
