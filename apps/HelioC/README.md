# HelioC

HelioC is the TRUEOS Cloud Engine sidekick to `apps/HelioV`.

## Upstream attribution

Helio is third-party software by Tristan Poland (`Trident_For_U`), copyright
2026, licensed under the MIT License. The canonical upstream repository is
[Far-Beyond-Pulsar/Helio](https://github.com/Far-Beyond-Pulsar/Helio). This
Blueprint keeps its real Helio dependency and the HelioC name; it is not
Picasso-owned code. See the shared
[renderer ownership boundary](../../docs/renderer-ownership.md).

It is not the hosted `heliov_flycam` example and it does not rename the Linux
Cloud Engine. The hosted `Helio-Examples/cloud_engine.rs` and its original WGSL
remain the workload source and visual oracle. This Blueprint owns the TRUEOS
platform, VMX and UI4 integration needed to execute that workload.

The first checked-in rung establishes:

- a real Blueprint identity and real `helio` dependency;
- pinned SHA-256 provenance for the exact simulation and raymarch WGSL;
- the exact 112-byte `SimParams` and 272-byte `RenderParams` ABI;
- two retained 96 x 48 x 96 RGBA16F VMX allocations;
- the original 4 x 4 x 4 simulation workgroup and 24 x 12 x 24 dispatch;
- the compute and fragment resource binding contract;
- fail-cold behavior with no C++ cloud or screen-space fallback.

It deliberately does not publish a fake cloud frame. Presentation becomes live
only when TRUEOS can admit the baked Cloud Engine stages, materialize their 3D
sampled/storage views and sampler, dispatch and synchronize the pass pair, and
release the fullscreen result through UI4.

Build it from the Blueprint repository root:

```sh
cargo bp apps/HelioC
```
