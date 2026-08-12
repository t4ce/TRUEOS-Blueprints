# HelioV

HelioV is the native TRUEOS Blueprint bring-up target for **actual Helio and
SceneDB code**. It is not another Shell2 demo and it is not a port of Stratum.

The deprecated Stratum `voxel_world` example is used only to choose a useful
product-shaped target: streamed procedural chunks, player/camera movement,
dynamic insertion/removal, coloured materials, indirect rendering, and later
textures. No Stratum runtime, ECS, `stratum-helio` bridge, or source file is
copied into this application.

## Non-negotiable boundary

HelioV may add:

- TRUEOS platform/device/presentation plumbing;
- a WGPU custom backend that translates generic WGPU operations to the VMX
  Blueprint vGPU ABI;
- build-time shader compilation and validated native pipeline packages;
- original game-specific voxel generation and input code.

HelioV may not add a second scene database, renderer, pass scheduler, material
system, or per-demo TRUEOS engine. Scene mutations must use Helio APIs and be
stored by SceneDB. Render work must originate from Helio's graph and passes.

## Proof gates

1. The Blueprint dependency closure compiles the real `helio`,
   `helio-scenedb`, and `libhelio` crates.
2. A TRUEOS WGPU `custom` backend constructs the Device and Queue consumed by
   `helio::Renderer`/`helio::Scene`.
3. HelioV inserts, updates, removes, and regrows voxel chunk meshes through
   normal Helio scene APIs.
4. The same backend runs a second Helio graph without TRUEOS renderer changes.
5. Textures are added only after the texture-free coloured voxel path renders.

The current first-stage executable generates a face-culled chunk directly as
Helio `MeshUpload`/`PackedVertex` data, compiles WGPU's real `custom` backend
contract, and probes the VMX vGPU device/buffer/render-queue/timeline ABI. It
also constructs real WGPU custom `Device`/`Queue` objects and runs a canonical
Helio `SceneObject` through SceneDB insert, mirrored edit, remove, row reuse,
despawn, and GPU flush paths. It also acquires an actual UI4 Blueprint back
buffer, maps that exact allocation into the caller's isolated VMX GPUVM, and
exposes it as a `wgpu::Texture`. A normal WGPU command encoder records a
render-pass `LoadOp::Clear`; queue submission executes and retires the mediated
Intel operation, revokes the tenant mapping, transfers the exact producer
release to UI4, publishes the frame, and waits for the physical SURFLIVE
acknowledgment. The retained dark-blue frame is therefore a command and
presentation proof, not a CPU paint or alternate renderer. Generic shader,
pipeline, binding, and indexed-draw objects remain the next boundary.

The retained proof already follows UI4's maximize/restore procedure. It stages
a private replacement generation, imports that exact new lease into VMX,
submits and publishes a complete frame, and updates the render-loop projection
aspect before UI4 commits the swap. The planned 2560x1440 bare-metal check is
therefore a real target reallocation and presentation test, not stretching the
640x360 front.

Current result: `cargo bp apps/HelioV` compiles the complete target dependency
closure and emits `dist/heliov.bp`. See [GPU_BOUNDARY.md](GPU_BOUNDARY.md) for
the measured boundary, rather than a guessed list of platform problems.

Build from the Blueprint repository root:

```sh
cargo bp apps/HelioV
```

Hosted `cargo check` is also supported. A hosted test binary cannot link the
TRUEOS CABI imports by design; tests that execute the Blueprint belong on
TRUEOS, while pure HelioV logic remains ordinary Rust testable code.
