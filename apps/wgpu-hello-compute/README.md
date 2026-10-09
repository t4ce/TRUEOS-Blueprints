# wgpu hello compute for TRUEOS

Adapted from `wgpu/examples/standalone/01_hello_compute` in the sibling
`../../../wgpu` checkout (MIT OR Apache-2.0). The dependency uses that local
checkout so future TRUEOS backend changes are picked up by this app.

The app keeps the example's WGSL shader, storage buffers, compute dispatch,
GPU-to-CPU copy and mapped readback. With no arguments it doubles
`[1, 2, 3, 4]`; successful execution prints `[2, 4, 6, 8]` and a PASS line.
Explicit command-line arguments are parsed as floats. Mapping failures and
incorrect results fail the run.

## Current status

The sibling `wgpu` checkout currently has no TRUEOS backend. The example is
packaged so its host compute path can be validated, and it reports that the
backend is unavailable when run on TRUEOS. It does not use a CPU substitute or
the wgpu noop backend.

Linux host builds enable Vulkan to validate the example independently.

## Build locally

From the TRUEOS-Blueprints repository root:

```sh
TRUEOS_BLUEPRINT_SKIP_APPS_PUBLISH=1 cargo bp wgpu-hello-compute
```

Output: `dist/wgpu-hello-compute.bp`. This command skips remote publication.
The app is registered in `apps.json` for the normal Blueprint tooling.

## Host validation

From the TRUEOS-Blueprints repository root:

```sh
cargo fmt --manifest-path apps/wgpu-hello-compute/Cargo.toml --check
cargo clippy --manifest-path apps/wgpu-hello-compute/Cargo.toml --tests
cargo run --manifest-path apps/wgpu-hello-compute/Cargo.toml
cargo run --manifest-path apps/wgpu-hello-compute/Cargo.toml -- 0 -1 2.5
```

A Vulkan adapter must be available for the host runs.
