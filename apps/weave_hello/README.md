# Weave hello_medium Blueprint

The original Weave hello Blueprint, bumped in place into a medium Windows CLI
compatibility specimen. There is deliberately still only one app/package.

`src/pe_bytes.rs` contains the exact bytes of the freestanding PE built from
`Weave/tools/windows-cli-hello`. The Blueprint contains a deliberately narrow
TRUEOS Weave slice:

- validates and maps PE32+ x86-64 sections;
- accepts only `kernel32.dll`;
- resolves fourteen console/process/time/environment/atomic `kernel32` calls;
- enters the Windows executable with the Microsoft x64 calling convention;
- preserves Windows standard-output success semantics without routing bytes
  during boot isolation.

Build from the TRUEOS-Blueprints root:

```sh
cargo bp weave_hello
```

OS Log contains loader receipts for validation, mapping, binding, entry, and
return, followed by per-contract API receipts and a final result:

```text
weave-boot-probe: IMPORTANT stage=loader.enter action=begin abi=win64
weave-boot-probe: IMPORTANT stage=kernel32.GetStdHandle action=noop-contract
weave-boot-probe: IMPORTANT stage=launch action=return exit_code=0
```

## Boot-isolation mode

This specimen currently keeps its fourteen `kernel32` exports contract-shaped
but replaces the TRUEOS-backed console, clock, and thread behavior with canned
successful results. Every loader stage and Windows API call emits a structured
OS Log record under `weave-boot-probe`, with `IMPORTANT` and
`action=noop-contract` in the message.

This is deliberate diagnostic scaffolding. The packed Blueprint has no POSIX
filesystem imports. Its remaining generic host shims are `memcpy` and `memset`,
plus `trueos_cabi_log` for the stage receipts. Reintroduce real helpers one at a
time only after a boot run establishes which side of that boundary disturbs
unrelated kernel state.
