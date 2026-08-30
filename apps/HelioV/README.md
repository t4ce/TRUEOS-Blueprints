# HelioV

HelioV is the native TRUEOS Blueprint bring-up target for **actual Helio and
SceneDB code**. It is not another Shell2 demo and it is not a port of Stratum.

## Upstream attribution

Helio is third-party software by Tristan Poland (`Trident_For_U`), copyright
2026, licensed under the MIT License. The canonical upstream repository is
[Far-Beyond-Pulsar/Helio](https://github.com/Far-Beyond-Pulsar/Helio). This
Blueprint keeps its real Helio dependencies and the HelioV name; it is not
Picasso-owned code. See the shared
[renderer ownership boundary](../../docs/renderer-ownership.md).

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

The current executable generates a deterministic, face-culled 6x6 chunk world
with terrain, water, trees, houses, and a leaning tower directly as Helio
`MeshUpload`/`PackedVertex` data. It compiles WGPU's real `custom` backend
contract and probes the VMX vGPU device/buffer/render-queue/timeline ABI. Real
WGPU custom `Device`/`Queue` objects drive one canonical Helio `SceneObject`
partner per chunk through SceneDB insert, mirrored edit, remove, row reuse,
despawn, GPU flush, and repeated component-buffer growth paths. HelioV enables
SceneDB's additive CPU-shadow reallocation policy for this DirtyTracked object
authority, so the current TRUEOS backend needs only buffer creation and
`Queue::write_buffer`; normal Helio remains on GPU-copy growth and no `Once`
handoff is weakened. An actual UI4 Blueprint back buffer is mapped into the
caller's isolated VMX GPUVM and exposed as a `wgpu::Texture`. The untextured
indexed path and UI4 SURFLIVE handoff are proven. Texture experiments later
proved decode, RGBA8 upload, SceneDB residency, bind-group identity, and a fixed
mip-0 sampled presentation on a four-vertex probe. They did not prove the
sampled voxel world: full, visibility-compacted, densely reindexed, and finally
2,048-triangle / 6,144-index versions all reached the graphics submission but
failed with the same `ui4-indexed-submit rc=-32` before SURFLIVE. That result
rules out JPEG versus PNG decode and strongly rules out mesh size as the next
useful variable.

The green recovery frame established that scene submission itself is sound.
The active artifact still reads no image and creates no sampled texture,
sampler, bind group, or UV pipeline. Instead it preserves the real first-rung
Helio voxel semantics: grass, dirt, stone, water, and landmark palette identity
through Helio's `SectionedMeshUpload`, with three flat face-light levels. One
authenticated WGPU immediate-RGBA package emits the non-empty palette/face
sections as a single VMX/resident-scene batch while retaining the complete
41,784-vertex / 62,676-index topology. This is a material recovery step, not a
replacement renderer or CPU paint fallback.

The retained proof already follows UI4's maximize/restore procedure. It stages
a private replacement generation, imports that exact new lease into VMX,
submits and publishes a complete frame, and updates the render-loop projection
aspect before UI4 commits the swap. The confirmed 640x360 to 2560x1440 and
restore path is a real target reallocation and presentation test, not a stretch
of the old front.

Camera control now uses Helio's shared platform-neutral `FlyCamera` and
`NavigationState`. The local UI4 adapter samples `input_routes`, selects only
the application-focused cursor/combo and its paired keyboard, and clears held
state whenever that route changes. This preserves TRUEOS multi-mouse and
multi-keyboard isolation instead of collapsing devices into global engine
input. Primary-button drag looks; WASD moves; Space/Shift move vertically; and
Control boosts. UI4 deliberately absorbs the gesture which first selects a
frame, so activate HelioV with one click and release before starting the first
primary-button look drag or using the keyboard. Projection remains a
compatibility upload for the current position-only shader package and is
refreshed after both camera and resize changes.

Current result: `cargo bp apps/HelioV` compiles the complete target dependency
closure and emits `dist/heliov.bp`. Its default artifact is the full-world
material-palette/immediate-data rung with no bitmap asset path. See
[GPU_BOUNDARY.md](GPU_BOUNDARY.md) for the measured boundary, rather than a
guessed list of platform problems.

Build from the Blueprint repository root:

```sh
cargo bp apps/HelioV
```

Hosted `cargo check` is also supported. A hosted test binary cannot link the
TRUEOS CABI imports by design; tests that execute the Blueprint belong on
TRUEOS, while pure HelioV logic remains ordinary Rust testable code.
