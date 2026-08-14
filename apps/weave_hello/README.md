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
- routes Windows standard output to the Blueprint console.

Build from the TRUEOS-Blueprints root:

```sh
cargo bp weave_hello
```

The serial output contains loader receipts for validation, mapping, binding,
entry, and return, followed by per-contract phase receipts and a final result:

```text
hello_medium: result=PASS checks=7
hello_medium: Windows PE returned exit_code=0
```
