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

The Blueprint enables the local fork's experimental `trueos` HAL feature.
Startup now reaches the native mediated vGPU probe (open, device info, close)
and prints its reported facts through the HAL instance. The HAL now enumerates
one TRUEOS adapter, but advertises no compute capability or usable resources;
Adapter::open still rejects requests. This app cannot run compute on TRUEOS yet.

This is a backend bring-up app, not yet a working TRUEOS GPU compute demo.
It does not use a CPU substitute or the wgpu noop backend. The implementation
and next integration boundary are documented in
`../../../wgpu/wgpu-hal/src/trueos/README.md`.

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
