# Weave hello Blueprint

The first Windows executable launched by a TRUEOS Blueprint.

`src/pe_bytes.rs` contains the exact bytes of the freestanding PE built from
`Weave/tools/windows-cli-hello`. The Blueprint contains a deliberately narrow
TRUEOS Weave slice:

- validates and maps PE32+ x86-64 sections;
- accepts only `kernel32.dll`;
- resolves only `GetStdHandle`, `WriteFile`, and `ExitProcess`;
- enters the Windows executable with the Microsoft x64 calling convention;
- routes Windows standard output to the Blueprint console.

Build from the TRUEOS-Blueprints root:

```sh
cargo bp weave_hello
```

Expected output when the Blueprint runs:

```text
Hello, world from Weave!
weave_hello: Windows PE returned exit_code=0
```
