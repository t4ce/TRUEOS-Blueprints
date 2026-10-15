# GBOI

Game Boy emulator Blueprint. The app and emulator core share this package;
`emulator/` provides the `trueos_gboi` library without a sibling TRUEOS checkout dependency for the emulator core.

Restored from TRUEOS-Blueprints commit `344f71953e0d2785910e9725c3051f0075c7b512`
and the `crates/trueos-gboi` tree immediately before TRUEOS commit `3b81f4b56`.

The default ROM is `common/gboi.gb`. In the app shell, use `list`,
`list common`, `load <path>`, `status`, or `help`.
Controls: arrow keys or WASD for direction, X/Space for A, Z for B,
C for Select, and Enter for Start.

Check from the repository root with the Blueprint runtime features:

```sh
cargo check -p gboi --offline --target apps/target.json -Zjson-target-spec -Zbuild-std=core,compiler_builtins,alloc --features trueos/default-global-allocator,trueos/default-panic-handler
```

Run the emulator core tests independently of the Blueprint runtime:

```sh
rustc --edition 2024 --test apps/gboi/emulator/lib.rs -o /tmp/gboi-core-tests
/tmp/gboi-core-tests
```
