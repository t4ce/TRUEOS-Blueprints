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
indexed path and UI4 SURFLIVE handoff are proven. The authenticated fixed mip-0
shader sampler read is now also proven through exact Render0 retirement and
SURFLIVE on the four-vertex diagnostic rung.
The current probe no longer synthesizes a checkerboard: it reads the kernel's
encoded JPEG logo, decodes it through `vmedia`, bounds it to the authenticated
16x16 mip-0 footprint, inserts it through `Scene::insert_texture`, resolves the
exact non-compacting SceneDB residency slot, and binds that slot's canonical
view/sampler pair. This closes asset decode, SceneDB-to-bind-group identity, and
the first physical sampled presentation.

The next physical log proved that VMX accepts and uploads the actual
41,784-vertex / 62,676-index Helio voxel world and launches its frame. The
unculled sampled draw then missed Render0's bounded two-second /
five-million-poll release window, producing `rc=-32` before SURFLIVE. The
default build therefore retains the complete Helio `MeshUpload` but submits a
camera-dependent index snapshot: closed-voxel backfaces and triangles wholly
outside one frustum plane are removed. The initial view measures 31,203
submitted indices, and camera/resize changes rebuild that snapshot. It keeps
the ordinary WGPU texture/view/sampler, bind-group, pipeline, indexed-draw and
UI4 path while still excluding implicit derivatives and filtering. The normal
`textureSample` package remains intact behind `--no-default-features`. There
is no CPU paint or alternate renderer fallback.

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
closure and emits `dist/heliov.bp`. Its default artifact is the fixed-texel
visibility-compacted world retirement rung. See
[GPU_BOUNDARY.md](GPU_BOUNDARY.md) for the measured boundary, rather than a
guessed list of platform problems.

Build from the Blueprint repository root:

```sh
cargo bp apps/HelioV
```

Hosted `cargo check` is also supported. A hosted test binary cannot link the
TRUEOS CABI imports by design; tests that execute the Blueprint belong on
TRUEOS, while pure HelioV logic remains ordinary Rust testable code.
