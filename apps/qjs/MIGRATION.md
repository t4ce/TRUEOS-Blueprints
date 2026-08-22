# QJS Blueprint runtime topology

The ownership migration is complete. The `qjs` Blueprint owns the QuickJS
runtime source under `crates/trueos-qjs`; the TRUEOS kernel has no
`trueos-qjs` dependency or QJS workbench state. Solara remains independent and
continues to own its separate `vendor/RustQJSDom` runtime.

## Runtime ownership

- Shell2's `qjs` command only launches `qjs.bp`.
- The visible Blueprint owns one persistent
  `trueos_qjs::workbench::Workbench`. Evaluation, script/module selection,
  output, timers, async operations, reset, and close happen in that Hull.
- The runtime uses the Blueprint SDK copy of `v`. Its remaining C ABI adapters
  are ordinary bounded Blueprint services, not kernel-owned QuickJS state.
- Async filesystem requests are advanced by the Blueprint VM pump. No QJS
  executor task is started by the kernel.

## Worker topology

`new Worker(source)` starts another instance of the same archive through the
generic `blueprint_child_{spawn,send,receive,status,terminate}_v1` ABI. The
kernel schedules that hidden child through the existing `vm_task` pool on a
reserved AP2+ VMX Hull lane.

The child starts with `--trueos-child-worker`, never acquires a terminal lease,
receives its source as the first frame on parent handle zero, and exclusively
owns a separate QuickJS runtime/context. Parent and child exchange bounded byte
frames only; executor handles, Rust futures, guest pointers, and JavaScript
values never cross the Hull boundary. The app enforces a 32-concurrent-worker
limit, while lane placement and VM lifecycle remain generic kernel policy.

Parent teardown terminates its children. A child also observes parent liveness
through handle zero and exits when the parent disappears. Final child-to-parent
messages remain drainable after child exit.

## Kernel boundary

The kernel retains only generic services used by other Blueprints as well:

- child Hull spawn, message, status, terminate, and generation checking;
- `vm_task` pooling and VMX lane leasing;
- bounded filesystem, network, clock, allocation, and loader ABI services;
- generic libc/math shims needed by Blueprint loading.

The old `shell2/qjs_workbench.rs`, QJS workbench CABI/VMCalls, QJS async-FS
startup task, QJS worker-spawner exports, and the `trueos-qjs` Cargo edge have
been removed.

## Verification

- `cargo fmt --manifest-path apps/qjs/Cargo.toml -- --check`
- `cargo check --manifest-path apps/qjs/Cargo.toml`
- `cargo check` in the TRUEOS kernel repository
- package `qjs.bp`

Source/build checks cover ownership and linkage. Persistent evaluation,
modules, timers, filesystem promises, worker message round-trips, reset, and
close still require an on-system VMX smoke test for runtime confirmation.
