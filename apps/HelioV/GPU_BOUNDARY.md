# HelioV GPU boundary

This file records the measured boundary after compiling HelioV as a real
`x86_64-unknown-trueos` Blueprint. It is deliberately not a list of speculative
porting tasks.

## Active material-recovery boundary

The constant-RGBA indexed world and UI4 SURFLIVE path were physically proven
before texture bring-up. Subsequent texture work proved PNG/JPEG decode to
RGBA8, SceneDB texture residency, the canonical texture/sampler binding, and
fixed mip-0 fragment sampling on the four-vertex diagnostic draw.

The sampled world did not retire. The complete 41,784-vertex / 62,676-index
draw, the 31,203-visible-index draw, its 20,807-vertex dense remap, and finally
the nearest 2,048 triangles / 4,122 vertices / 6,144 indices all reached
`ui4-indexed-submit` and failed with `rc=-32` before SURFLIVE. Decode and
sampled mesh size are therefore parked variables, not active prerequisites.

The green baseline is now physically established. Inspection of Helio's real
voxel example showed that its first visual identity is a material palette and
lighting, not a decoded bitmap tiled over the mesh. HelioV therefore keeps
image assets parked and preserves grass, dirt, stone, water, and landmark
identity as Helio `SectionedMeshUpload` sections. Three bounded face-light
levels make at most fifteen material sections without changing the authoritative
41,784-vertex / 62,676-index topology.

The active WGPU package accepts one clip-space `Float32x3` attribute and a
`vec4<f32>` immediate fragment color. Repeated `draw_indexed` calls are encoded
as one fixed-capacity VMX batch and consume the imported UI4 lease once. The
broker receives only generic ranges and RGBA bytes, then reuses TRUEOS
Render's resident-scene batching already proven by `helio 2`; it receives no
voxel or material names. The physical success gate is the
`Helio material-palette ... indexed batch retired and SURFLIVE confirmed`
line followed by the live palette-world and camera logs.

## Proven now

- The Blueprint builder compiles the actual `helio`, `helio-scenedb`,
  `libhelio`, patched WGPU 30, WGPU Core/HAL, Naga, and Helio pass crates into
  one ET_REL Blueprint artifact.
- Hosted-only OpenXR loading is an optional Helio feature and is absent from
  the TRUEOS dependency closure. Hosted Helio keeps it in the default profile.
- WGPU's public `custom` backend interfaces compile for TRUEOS. HelioV names
  the active contract `wgpu-30-custom/vmx-vgpu-v7-material-immediates`.
- The Blueprint imports and exercises the generic VMX vGPU device, buffer,
  read/write, render-queue, submission-timeline, and wait operations.
- A buffer-first WGPU custom `Device`/`Queue` maps WGPU buffer creation and
  `Queue::write_buffer` onto those VMX handles. Its unsupported object classes
  fail explicitly rather than falling through to a hosted backend.
- The executable uses that WGPU pair to construct `helio-scenedb`'s real
  `SceneAuthority`, register Helio's canonical object partner, and run one
  object per voxel chunk through insert/edit/remove/reinsert/despawn plus mirror
  flushes and component-local row growth. HelioV opts this authority into
  SceneDB's additive `RewriteFromCpuShadow` policy: DirtyTracked value,
  presence, and generation columns rebuild replacement allocations from their
  complete CPU shadows using `Queue::write_buffer`. Default Helio/SceneDB
  authorities retain GPU-copy growth, `Once` columns are ineligible, and
  TRUEOS gains no demo-specific kernel path. The complete `helio::Renderer`
  graph remains outside this compatibility rung.
- The voxel source produces a deterministic face-culled 6x6 chunk world as
  authoritative Helio `MeshUpload` data and a topology-equivalent
  `SectionedMeshUpload`, retaining per-chunk bounds and index ranges. The
  section abstraction is Helio's existing shared multi-material model.
- UI4 presentation storage is no longer disconnected from vGPU. HelioV opens
  a normal streaming Blueprint frame, acquires its real write lease, imports
  that page-aligned allocation into its own PPGTT, and receives a custom WGPU
  texture/view. Dropping the texture revokes the mapping before cancelling the
  lease; device close is rejected while an imported target is live.
- WGPU command encoding, command-buffer submission, exact Intel retirement,
  and UI4 presentation are proven for render-pass clear and the untextured
  indexed graphics package. An earlier run also created a real Helio/SceneDB
  texture and material from a kernel JPEG decoded by `vmedia`, resolved the
  canonical non-compacting residency slot, and created the WGPU RGBA8
  texture/view/sampler, bind group, sampled WGSL module, immutable pipeline and
  indexed draw. The fixed mip-0 sampled submission retires and reaches
  SURFLIVE on the diagnostic quad. The complete world upload and launch are
  also proven, but its unculled single draw exceeded the exact-retirement
  window. Even the 2,048-triangle sampled snapshot failed identically. Those
  sampled-asset path remains parked; the full material-section batch is the
  active frontier.
- A separate build-time TRUEOS capture now compiles Helio's unmodified
  G-buffer WGSL to Intel SIMD8 VS/FS ISA with the real 40-byte vertex layout,
  two bind groups, 256-wide texture/sampler arrays, eight color targets, and
  depth. This proves the native compiler ABI; it does not imply that the
  current VMX adapter can replay the full pass.
- Maximize/restore consumes UI4's resize event and stages its private
  replacement generation. HelioV begins and imports that exact new lease,
  updates the camera projection and vertex upload from the new aspect, submits
  and publishes the complete indexed frame, and only then lets UI4 replace the
  previous SURFLIVE front.
- UI4's routed n-pointer/n-keyboard state now drives Helio's shared semantic
  navigation and `FlyCamera`. One application-focused cursor/combo route owns
  the camera at a time; route changes clear held keys and pending deltas. A
  changed pose rewrites the compatibility projection and submits through the
  same WGPU/VMX/UI4 path, with busy streaming leases retained for retry.

## Implemented vertical slice

| Layer | Ownership in the indexed voxel frame |
| --- | --- |
| HelioV Blueprint | Builds the deterministic chunked world as authoritative `MeshUpload` plus `SectionedMeshUpload`, owns the palette, projects its complete position stream for the live camera, and runs the WGPU event/presentation loop. |
| `helio-controls` | Owns platform-neutral navigation actions, held/delta reduction, fly-camera behavior, and lens/pose handoff; it owns no UI4 or WGPU policy. |
| WGPU custom backend | Validates the exact immediate-RGBA WGSL package and position-only pipeline interface, rejects bind groups, and records one bounded batch of sectioned `draw_indexed` calls. |
| VMX vGPU broker | Owns generation-tagged resources, validates every vertex/index range plus the admitted package, converts no Helio concepts, and consumes one UI4 lease for the batch. |
| TRUEOS Render | Reuses resident-scene multi-draw with one RGBA value per dense section and mints the exact release fence. No sampled surface or sampler table participates. |
| UI4 | Retains the old front across resize, accepts only a release matching its private write lease, commits/publishes that generation, and reports physical SURFLIVE. |

The current AOT package accepts only clip-space `Float32x3` positions and emits
fragment RGBA from standard WGPU immediate data. Camera projection is still performed in the Blueprint
adapter before `Queue::write_buffer`; the source pose and movement
now come from the same shared Helio controller used by hosted examples. This is
the narrow compatibility step that keeps the first voxel frame interactive
without teaching the kernel about cameras or materials. A camera-uniform shader
package replaces that projection upload later while leaving controller, object,
draw, lease, and release architecture unchanged.

## Exact runtime gap

The present VMX vGPU ABI is a safe resource-and-scheduling substrate. WGPU's
custom backend needs the following semantic operations before it can create a
complete high-level `helio::Scene`/`Renderer` and execute its graph:

| WGPU family | VMX vGPU today | Required adapter/ABI work |
| --- | --- | --- |
| Device, buffers, queue, timeline | Present and probed by HelioV; copy, vertex, and index usages map to opaque buffers. CPU-shadowed SceneDB partners can grow without encoder copies. | Complete public mapping semantics, generic GPU-native copies for `Once`/GPU-only storage and other WGPU clients, callbacks, and asynchronous device-loss rules. |
| Limits/features/errors | Only coarse capability bits and device info | Publish WebGPU limits, format features, error scopes, and device-loss callbacks. |
| Textures/views/samplers | The adapter capability and diagnostic sampled proof exist, but the active material artifact creates none of these objects. Every tested sampled-world size currently fails before SURFLIVE. | Keep bitmap assets separate from voxel palette recovery; next diagnose the structural sampled-package failure independently of decode. |
| Shader modules | One exact WGSL source is matched to an authenticated AOT package digest and represented by an opaque vGPU object | Generalize the build-produced package catalog and reflect compilation diagnostics/features for the Helio shader set. |
| Bind groups/layouts | The active package has an explicit 16-byte immediate layout and no bind groups. A one-texture diagnostic group was previously proven. | Later generalize typed descriptor tables, arrays, buffers, stage visibility, and dynamic offsets for full Helio passes. |
| Render pipelines | Opaque immutable shader-package plus position-layout pipeline objects; one color target/topology is admitted | Add the raster/depth/blend/target variants requested by Helio graphs and package metadata. |
| Command encoder and passes | Clear, pipeline bind, vertex bind, Uint32 index bind, immediates, and up to sixteen direct indexed draws execute as one submission | Add generic copy, compute dispatch, bind groups, heterogeneous batches, and indexed-indirect packets. |
| Presentation | Exact UI4 lease import, resident-Render release, publish, SURFLIVE acknowledgment, transactional maximize/restore, and demand-driven camera redraw all run on the indexed path; frame-begin backpressure retains the dirty camera pose | Add brokered continuous cadence and complete recovery/device-loss rules around a full Helio graph. |
| Queries/timestamps | Timeline completion only | Add the query subset actually requested by Helio; optional features remain disabled until advertised. |

The key distinction is that these operations describe generic GPU objects and
commands. The TRUEOS kernel must not learn what a voxel, Helio pass, SceneDB
row, or material means.

VMX vGPU v1's bounded CPU transfer calls require `MAP_WRITE`/`MAP_READ`. The
buffer-first adapter therefore grants those internal broker capabilities for
WGPU `COPY_DST`/`COPY_SRC` buffers respectively. WGPU mapping remains
unsupported and unexposed; this is the explicit staging policy behind
`Queue::write_buffer`, not a public usage-bit lie.

## Vertical proof order

1. The buffer-first WGPU custom `Device`/`Queue` and Helio object-partner
   lifecycle now exist in HelioV. DirtyTracked partner growth is complete on
   the current backend through the explicit CPU-shadow rewrite policy. Extract
   the adapter as a reusable crate and add generic GPU-native buffer copies for
   storage that has no reconstructible CPU authority.
2. Physically prove the complete material-palette batch, camera, resize, and
   SURFLIVE path. Decoded asset format and triangle-count tuning are already
   excluded as useful variables. Afterward add a camera-uniform package and the
   buffer/binding variants needed to stop the remaining CPU projection upload.
3. The SceneDB partner-row lifecycle now covers every world chunk plus one
   transform/remove/regrow proof. Extend that identity into mesh residency,
   compaction, visibility, and indexed-indirect drawing.
4. Run a second Helio graph through the unchanged WGPU/VMX interface.
5. Texture decode, residency, upload, exact slot binding, and fixed-load sampled
   presentation were proven on the diagnostic draw. They are intentionally
   absent from the material-palette artifact. Reintroduce them behind a separate gate
   after the scene baseline, then extend that authority to the descriptor
   arrays and material buffers consumed by the full G-buffer.

Step 2's visible indexed frame establishes the first real graphics territory.
Passing steps 2–4 in their complete form establishes that the adapter is an
engine backend rather than a manually ported demo.
