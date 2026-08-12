# HelioV GPU boundary

This file records the measured boundary after compiling HelioV as a real
`x86_64-unknown-trueos` Blueprint. It is deliberately not a list of speculative
porting tasks.

## Proven now

- The Blueprint builder compiles the actual `helio`, `helio-scenedb`,
  `libhelio`, patched WGPU 30, WGPU Core/HAL, Naga, and Helio pass crates into
  one ET_REL Blueprint artifact.
- Hosted-only OpenXR loading is an optional Helio feature and is absent from
  the TRUEOS dependency closure. Hosted Helio keeps it in the default profile.
- WGPU's public `custom` backend interfaces compile for TRUEOS. HelioV names
  the contract `wgpu-30-custom/vmx-vgpu-v5-aot-indexed-resize-surflive` so loss of that feature
  is a build failure rather than a runtime surprise.
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
  TRUEOS gains no demo-specific kernel path. The high-level
  `helio::Scene` is intentionally not constructed until texture/sampler
  support satisfies its real constructor contract.
- The voxel source produces a deterministic face-culled 6x6 chunk world as
  Helio `MeshUpload` data, retaining per-chunk bounds and index ranges. No
  Stratum runtime and no TRUEOS-specific renderer, scene store, or material
  implementation exists.
- UI4 presentation storage is no longer disconnected from vGPU. HelioV opens
  a normal streaming Blueprint frame, acquires its real write lease, imports
  that page-aligned allocation into its own PPGTT, and receives a custom WGPU
  texture/view. Dropping the texture revokes the mapping before cancelling the
  lease; device close is rejected while an imported target is live.
- WGPU command encoding, command-buffer submission, exact Intel retirement,
  and UI4 presentation are proven for both render-pass clear and one indexed
  graphics package. HelioV creates a real WGPU WGSL shader module, immutable
  render pipeline, vertex buffer, and index buffer, binds them in a render
  pass, and executes `draw_indexed` over the Helio-authored voxel mesh. The
  surface mapping is consumed only after Render0's resident-scene release
  packet retires, and HelioV waits for UI4's physical SURFLIVE event.
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
| HelioV Blueprint | Builds the deterministic chunked world as `helio::MeshUpload`, adapts the focused UI4 input route into shared Helio navigation, and runs the WGPU event/presentation loop. |
| `helio-controls` | Owns platform-neutral navigation actions, held/delta reduction, fly-camera behavior, and lens/pose handoff; it owns no UI4 or WGPU policy. |
| WGPU custom backend | Validates the WGSL/package match and pipeline interface, retains command resources, and records render-pass clear plus pipeline/vertex/index bindings and `draw_indexed`. |
| VMX vGPU broker | Owns generation-tagged shader and pipeline objects, validates buffer usages and every byte/index range, admits only the authenticated AOT package digest, and never receives Helio/voxel concepts. |
| TRUEOS Render | Uploads the bounded projected position/index snapshot into resident Render0 storage, executes the authenticated VS/PS and indexed 3D draw in one GuC scene schedule, and mints the exact resident-scene release fence. |
| UI4 | Retains the old front across resize, accepts only a release matching its private write lease, commits/publishes that generation, and reports physical SURFLIVE. |

The current AOT package accepts clip-space `Float32x3` positions and a fixed
fragment output. Camera projection is intentionally still performed in the
Blueprint adapter before `Queue::write_buffer`; the source pose and movement
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
| Textures/views/samplers | Absent | Add opaque texture allocation, view ranges, formats/usages, sampler descriptors, and copy/clear operations. |
| Shader modules | One exact WGSL source is matched to an authenticated AOT package digest and represented by an opaque vGPU object | Generalize the build-produced package catalog and reflect compilation diagnostics/features for the Helio shader set. |
| Bind groups/layouts | Absent | Add descriptor tables referencing opaque buffer/texture/sampler handles with validated ranges and dynamic offsets. |
| Render pipelines | Opaque immutable shader-package plus position-layout pipeline objects; one color target/topology is admitted | Add the raster/depth/blend/target variants requested by Helio graphs and package metadata. |
| Command encoder and passes | Clear, pipeline bind, vertex bind, Uint32 index bind, and one direct indexed draw execute through WGPU | Add generic copy, compute dispatch, bind groups, multiple draws, and indexed-indirect packets. |
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
2. The first shader/pipeline/vertex/index/direct-draw package is now visible.
   Add texture, sampler, bind-group, uniform-camera, and pipeline variants
   sufficient for one texture-free high-level Helio graph. Assign voxel colour
   using a normal Helio material; `PackedVertex` has no hidden colour channel.
   Texture/sampler objects are still required for Helio's 1x1 placeholder even
   though the voxel material itself is texture-free.
3. The SceneDB partner-row lifecycle now covers every world chunk plus one
   transform/remove/regrow proof. Extend that identity into mesh residency,
   compaction, visibility, and indexed-indirect drawing.
4. Run a second Helio graph through the unchanged WGPU/VMX interface.
5. Add texture residency/upload as a separate milestone.

Step 2's visible indexed frame establishes the first real graphics territory.
Passing steps 2–4 in their complete form establishes that the adapter is an
engine backend rather than a manually ported demo.
