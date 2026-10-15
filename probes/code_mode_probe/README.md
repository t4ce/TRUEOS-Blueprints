# Native Code Mode probe

This optional probe runs the production QuickJS provider and Code Mode session
protocol. Its default cases exercise ES modules and top-level await, nested
delegated tools with computed JSON arguments, session store/load, and cooperative
interruption and cleanup of an infinite loop. It does not use a model or remote
JavaScript host.

Host baseline:

```sh
cargo run --manifest-path probes/code_mode_probe/Cargo.toml
```

Native build and publish to the existing probe Apps destination:

```sh
probes/code_mode_probe/build_publish.sh
```

On TRUEOS, launching `code_mode_probe` additionally authorizes one unchanged
`gri` script through the production Shell3 `KernelBackend`, as required for this
native vertical slice. Ensure the owned test instance has the `gri` alias mapped
to `gridpaper`; the embedded default alias is named `grid`. Shell3 can launch the
probe directly with `§§code_mode_probe`.

The native delegate accepts only that exact script. It captures output and the
actual numeric exit status for up to 30 seconds. A persistent session reports
`running_at_deadline` with a null exit code; this is not an asserted successful
command exit. Confirming Gridpaper UI4 appeared requires observing the native
display. The host default performs only the controlled protocol cases; host
`--gri` explicitly refuses native execution.

The probe is isolated from the main Blueprint Cargo workspace because Codex's
protocol build dependency and unrelated Blueprint probes pin incompatible patch
versions. Its patches mirror the protocol workspace contracts. It is registered
only in `probes.json`, never in default applications or startup.
