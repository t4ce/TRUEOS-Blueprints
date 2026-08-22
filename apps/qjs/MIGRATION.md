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

The relocated crate remains a nested workspace during this bridge phase. It
cannot join the `qjs` app workspace until its private TRUEOS `v`, locale, and
executor dependencies are replaced: Cargo cannot lock those alongside the
Blueprint SDK packages with the same names and versions but different source
paths.

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
5. Remaster each parallel QJS Worker as a child `qjs` Hull. The kernel already
   owns a 64-entry `vm_task` pool and leases AP2+ `VmHull` lanes; that is the
   executable boundary a Blueprint can safely use. Keep worker QuickJS state,
   startup evaluation, timers, and message callbacks inside the child Hull.
   Add a generic child-Blueprint spawn/message/terminate CABI so the parent can
   request a child instance of its own archive without receiving a kernel
   `Spawner` or passing a Rust future/function pointer across the VM boundary.
   A Blueprint-local current-thread Tokio task remains useful for cooperative
   pumps, but it is not a replacement for the existing parallel lane behavior.
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
