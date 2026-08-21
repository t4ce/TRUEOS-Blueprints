# QJS runtime ownership migration

The `qjs` Blueprint owns the QuickJS runtime source under
`crates/trueos-qjs`. Solara is outside this migration: it owns and uses its
separate `vendor/RustQJSDom` runtime.

## Current transition state

The complete `trueos-qjs` source tree has moved from the TRUEOS kernel
repository into this application. The kernel temporarily consumes the crate
through a sibling path dependency so the existing `qjs.bp` workbench keeps its
runtime, worker behavior, timers, async filesystem operations, and module
loader unchanged.

The Blueprint UI still calls the kernel workbench through
`trueos::vshell::{qjs_workbench_eval, qjs_workbench_poll,
qjs_workbench_close}`. This is the compatibility bridge, not the final owner
boundary.

## Remaining runtime cut

1. Split the crate's current `trueos` feature into a Blueprint-safe runtime
   core and explicit host-service adapters. The runtime core owns QuickJS,
   evaluation, module loading, timers, diagnostics, and JS-facing Node APIs.
2. Keep using the explicit `kernel-code-model` feature only for the temporary
   kernel consumer. Blueprint C objects can now follow the packager's
   PIC/code-model flags.
3. Replace direct source includes of `TRUEOS/src/r/cabi_codes.rs` with an ABI
   definition owned by `trueos-v` or by this application.
4. Move `shell2/qjs_workbench.rs` state and evaluation into the Blueprint and
   make the UI call it directly. Then remove the qjs-workbench eval/poll/close
   CABI and the kernel module.
5. Preserve workers through a narrow host boundary while remastering them.
   The current `extern "Rust"` spawner hooks are kernel-internal and cannot be
   Blueprint imports. Prefer Blueprint-local tasks when the packaged Tokio
   runtime can provide the required isolation; otherwise expose only generic
   CABI scheduling and message-queue operations from the kernel.
6. Once worker and async services no longer require kernel-owned QuickJS state,
   remove the temporary TRUEOS dependency on `trueos-qjs`.

## Verification gates

- `cargo check --manifest-path apps/qjs/Cargo.toml`
- `cargo check -p trueos-qjs` from the TRUEOS repository during the bridge phase
- `cargo check --bin TRUEOS` during the bridge phase
- package `qjs.bp` after the runtime core becomes a direct app dependency
- exercise persistent evaluations, ESM imports, timers, filesystem promises,
  worker message round-trips, reset, and close before deleting the compatibility
  bridge
