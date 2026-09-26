# Native WC3 renderer: first-pass refactor strategy

Status: proposed implementation plan, 2026-09-27. No runtime changes in this pass.

## Decision

Keep Win32/OpenGL interpretation in the WC3 Blueprint. Reuse TRUEOS's native
primitive assembler, resource validation, shader admission and Render0/GuC
submission. Introduce an explicit state-bearing graphics contract between them.
Keep the Rust renderer as the reference and fallback.

First native scope: CPU vertex transformation and fixed-function lighting;
GPU homogeneous clipping, primitive assembly, interpolation, sampling, fog,
alpha testing, depth, blending and rasterization. Moving lighting to shaders is
a later optimization. Native strip assembly does not require that optimization.

## Existing capability versus missing contract

PotatoStamps/src/main.rs submits IndexedDrawBatchV2 with native topology. TRUEOS
src/intel/render/state.rs maps strips and fans to native VF topology. Those
pieces are reusable. The V2 draw descriptor carries immediate color, topology
and draw ranges, not WC3's complete rendering state.

The sampled single-draw route has two explicit admission gates:
`src/r/io/vgpu_cabi.rs::broker_indexed_draw_topology` and
`src/gpu/vgpu.rs::ui4_single_indexed_topology_valid`. Both currently accept
triangle/quad lists only. Its shader contract is position3 + UV2; the sampler is
nearest/repeat and the load-color route supplies no shared GL depth buffer.
Opening the topology gates alone does not implement native WC3 drawing.

## 1. Separate command capture from execution (Blueprint-only first patch)

Modify `staticgl_compat_draw.rs` so the draw provider produces an owned draw
packet before choosing a backend. Proposed new modules:

- `staticgl_commands.rs`: typed Draw, Clear, Readback, Finish and Present records.
- `staticgl_cpu_backend.rs`: adapter to the existing raster core.
- `staticgl_backend.rs`: capability classification and execution ordering.

A draw packet preserves source topology, source index order, owned decoded
vertices, immutable state and texture generation references. `gl_assemble_triangles`
moves behind the CPU adapter. Native strips must receive their original indices.

Reuse `gl_compat_vertices` for CPU transforms and lighting, retaining clip XYZW,
primary RGBA, homogeneous texture coordinates and fog coordinate. Separate its
return types from raster-private types. A proposed native vertex V1 uses four
vec4 slots (clip, color, texture coordinate, fog + reserved zeroes), 64 bytes.
Define and test offsets explicitly before freezing an ABI.

`WglContext` remains the GL state owner. Snapshot state when a draw is issued;
subsequent setters must not affect earlier queued work. Texture uploads/subimage/
deletes create or retire generations; outstanding packets retain the exact
levels they referenced. Guest pointers are never durable GPU resource identities.
Reuse guest-index deduplication only if it preserves the complete ordered index
sequence; no topology expansion, winding canonicalization or draw reordering.

Acceptance: existing 356-test baseline remains green; the same capture produces
identical CPU pixels. Strip packets retain four indices while the CPU adapter
produces two triangles. Mutation-after-capture tests establish ownership.

## 2. Define a versioned native graphics contract

Add new typed descriptors/capability negotiation in
`crates/trueos-v/src/vgpu.rs`; keep IndexedDraw/IndexedDrawBatchV2 unchanged.
Add matching validated broker operations in TRUEOS `src/r/io/vgpu_cabi.rs` and
`src/gpu/vgpu.rs`, including the guest-vmcall transport/dispatch and ABI layout
checks required by the existing routing. Proposed names below are not existing APIs.

Required objects:

- RenderTarget: generation-checked, HGLRC-owned color and persistent depth.
- Texture + sampler: owned levels, base-format semantics, filtering and wrapping.
- GraphicsPipeline: admitted vertex/fragment contract and immutable raster state.
- Draw: resource handles, topology, index range, viewport/scissor, uniform block.
- Ordered Clear/Submit/Readback/Present operations with timeline completion.

Keep GL enums/guest pointers out of the kernel. Translate them in WC3 to typed,
bounded descriptors. Negotiate supported vertex layouts, state combinations,
formats, filters and resource limits; unsupported requests remain identifiable.

Validate full command batches before resource mutation/submission. Pin resources
until retirement; quarantine ambiguous completion. No CPU retry after device
loss or uncertain execution. Preserve symbolic errors plus public errno.

A persistent target is essential: partial clears, glFinish and readback do not
start a new framebuffer. Swap exchanges/preserves buffers according to the chosen
pixel-format contract; it must not implicitly reset depth. Present the completed
target into UI4 through GPU work, without a CPU framebuffer texture upload.

Memory admission must include color, depth, texture residency, staging and any
separate imported UI4 surface. At 1440p, three separate four-byte-per-pixel images
already exceed 32 MiB before textures. Negotiate a measured, bounded device budget
or reject native admission and retain CPU rendering. Do not bypass accounting.
Alias charges may be avoided only when the physical allocation really is shared.

## 3. Extend resident state and shader contracts

Reuse the primitive mapping and submission infrastructure. Extend or add an
explicit native pipeline path in `src/intel/render/primary.rs`, `state.rs`,
`pipeline.rs`, `resources.rs` and `submit.rs`; do not change defaults of the
existing PotatoStamps/Cubes/UI4 pipelines.

Implement viewport/scissor, front-face/cull selection, depth comparison/write/
range, polygon offset and blend state. Native WC3 must preserve winding rather
than use the current triangle-list canonicalizer. Keep depth across submissions;
only explicit clears reset it. Check alpha discard ordering against depth writes.

Add authenticated vertex/fragment packages through the existing shader artifact
pipeline. Preserve XYZW into clipping. Specify GL clip-depth to native clip-depth
conversion and framebuffer/texture orientation once, with tests. Interpolate
color, texture coordinates and fog with the required semantics; handle texture
Q independently from clip W. Fragment work provides base-format texture-env
rules, fog and alpha test; hardware state handles depth/blend where supported.

Implement complete mip-chain residency and min/mag/wrap selection for the captured
workload. Cache by immutable generation and sampler/pipeline key, not pointers.
Retire staging after upload completion. Do not retain redundant full-size staging
and sampled copies indefinitely. RF01 remains a separate later consumer; changing
the game's lighting model is not a dependency for this renderer migration.

## 4. Validation without repeated game startup

Capture one bounded real WC3 render interval plus all referenced resources, with
its initial color/depth or a proven initializing clear. Stop capture at an explicit
swap/readback/finish boundary; label which boundary was observed. A partial
frontier capture is still useful, but is not a completed game frame.

Replay the immutable command stream to CPU and native offscreen targets. Compare
coverage, color and depth with documented format/rounding tolerances. Fixtures:
four-index strip with culling; perspective checkerboard crossing near plane;
minified mip chain; overlapping depth-tested draws; partial clear; alpha discard;
blending/fog; offset viewport/scissor; subimage/delete after an earlier draw;
readback pack/row orientation; resize and context lifetime.

Reuse PotatoStamps topology fixtures and rerun its primitive tests for regressions.
Report source topology/index count, native topology/index count, backend,
resource generation, uploaded bytes, memory peaks and actual timeline retirement.
CPU presentation through a GPU quad must never count as native game geometry.

## 5. Rollout and fallback policy

Initially keep CPU rendering authoritative and replay captured commands on a
separate native target for comparison. This also provides a coherent CPU shadow
while native coverage is incomplete. Validate the captured workload end to end
before native-only mode is enabled.

Do not alternate arbitrary CPU and GPU draws onto unrelated buffers. A backend
transition requires coherent color AND depth. Initially switch only at a verified
full color/depth initialization boundary. For unsupported state encountered later,
use the already-current CPU shadow for visible output. Without a coherent shadow,
stop before unsupported execution; do not silently switch. A later explicit
GPU-to-CPU synchronization path can support general transitions/readbacks.

Preserve guest clear/draw/readback/finish/swap order. Chunked submissions retain
target contents and do not publish partial frames unless explicitly in diagnostic
preview mode. Resource-budget rejection happens before selecting the native path.

## Implementation batches and gates

1. Owned command packets + CPU adapter + bounded capture. No rendering change.
2. Versioned target/resource/pipeline ABI + native strip probe, with capability
   negotiation and lifetime/quota tests. Requires coordinated kernel/Blueprint.
3. Clip4/color/UV/fog shader contract + sampler/mips + raster/depth/blend state;
   offscreen replay equivalence for the captured workload.
4. Native WC3 backend with CPU shadow, UI4 publication and readback/lifetime tests;
   enable native-only execution only after capture and hardware results justify it.
5. Remove routine shadow rendering; optimize caching/batching, then move CPU
   lighting/transforms into shaders if profiling identifies them as material costs.

The immediate next code patch is batch 1. The first native milestone is a real
WC3 strip retaining four indices through VF assembly with validated render state,
not merely a different topology for the framebuffer publication quad.
