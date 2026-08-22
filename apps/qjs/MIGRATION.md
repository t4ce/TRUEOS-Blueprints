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

`qjs.bp` now creates and owns a persistent `trueos_qjs::workbench::Workbench`
inside its application process. The terminal UI calls that object directly:
evaluation, result formatting, console output, module-mode detection, timers,
async pumping, and VM reset no longer use the qjs-workbench eval/poll/close
CABI.

The runtime is now a direct `qjs.bp` dependency and uses the Blueprint SDK
copy of `v`. It still has a few transition adapters (notably locale, time, and
legacy synchronous filesystem CABI), but those no longer put the VM back in
the kernel.

## Remaining runtime cut

1. Split the crate's current `trueos` feature into a Blueprint-safe runtime
   core and explicit host-service adapters. The runtime core owns QuickJS,
   evaluation, module loading, timers, diagnostics, and JS-facing Node APIs.
2. Keep using the explicit `kernel-code-model` feature only for the temporary
   kernel consumer. Blueprint C objects can now follow the packager's
   PIC/code-model flags.
3. Replace direct source includes of `TRUEOS/src/r/cabi_codes.rs` with an ABI
   definition owned by `trueos-v` or by this application.
4. Remove `shell2/qjs_workbench.rs` and the qjs-workbench eval/poll/close CABI.
5. Workers now spawn a child instance of the same archive via the generic
   `blueprint_child_{spawn,send,receive,status,terminate}_v1` CABI. The kernel
   selects the VMX lane; the child starts with `--trueos-child-worker`, receives
   its startup source as its first frame on handle zero, and owns its own
   QuickJS runtime/context. Parent and child exchange byte frames only—no
   `Spawner`, Rust future, or JS value crosses the Hull boundary.
6. Replace remaining legacy synchronous filesystem CABI declarations with the
   Blueprint async FS service, then remove the temporary TRUEOS dependencies
   still used by runtime adapters.
7. Once worker and async services no longer require kernel-owned QuickJS state,
   remove the temporary TRUEOS dependency on `trueos-qjs`.

## Verification gates

- `cargo check --manifest-path apps/qjs/Cargo.toml`
- `cargo check -p trueos-qjs` from the TRUEOS repository during the bridge phase
- `cargo check --bin TRUEOS` during the bridge phase
- package `qjs.bp` after the runtime core becomes a direct app dependency
- exercise persistent evaluations, ESM imports, timers, filesystem promises,
  VMX child worker message round-trips, reset, and close
