# HelioV GPU boundary

This file records the measured boundary after compiling HelioV as a real
`x86_64-unknown-trueos` Blueprint. It is deliberately not a list of speculative
porting tasks.

## Texture bring-up status correction

Texture allocation, CPU upload, view/sampler objects, bind-group validation and
sampled-surface construction reach the kernel. They are not yet an end-to-end
success: the first fragment sampler submission on physical ADL-S does not
retire and no sampled frame reaches SURFLIVE. Older standalone JPG/PNG/video
paths do not close this gap because they never sampled an image from a mesh
fragment shader.

The current default Blueprint selects the authenticated fixed mip-0
`textureLoad` probe and draws one four-vertex Helio quad. This is a diagnostic
package, never a fallback renderer. It distinguishes sampled-surface/message
failure from implicit-derivative/filter failure and emits sampler/row/fault plus
stage-frontier diagnostics on timeout. Building HelioV with
`--no-default-features` restores the canonical filtered voxel material package.

Its pixels now come from `kernel:logo`: the Blueprint reads encoded JPEG bytes,
decodes them through `vmedia`, downsamples to the package's bounded 16x16 mip-0
footprint, inserts the upload through the real Helio `Scene`, and binds the
view/sampler at `TextureResidency::slot_for`. The public `TextureId` entity slot
is logged separately and is never treated as a renderer array index.

## Proven now

- The Blueprint builder compiles the actual `helio`, `helio-scenedb`,
  `libhelio`, patched WGPU 30, WGPU Core/HAL, Naga, and Helio pass crates into
  one ET_REL Blueprint artifact.
- Hosted-only OpenXR loading is an optional Helio feature and is absent from
  the TRUEOS dependency closure. Hosted Helio keeps it in the default profile.
- WGPU's public `custom` backend interfaces compile for TRUEOS. HelioV names
  the contract `wgpu-30-custom/vmx-vgpu-v6-sampled-texture-surflive` so loss of that feature
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
  TRUEOS gains no demo-specific kernel path. The high-level `helio::Scene` is
  now constructed for the bounded texture/material witness; the complete
  `helio::Renderer` graph remains outside this compatibility rung.
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
  and UI4 presentation are proven for render-pass clear and the untextured
  indexed graphics package. HelioV also creates a real Helio/SceneDB texture
  and material from a kernel JPEG decoded by `vmedia`, resolves the canonical
  non-compacting residency slot, and creates the WGPU RGBA8
  texture/view/sampler, bind group, sampled WGSL module, immutable pipeline and
  indexed draw. That sampled submission reaches Render0 but currently does not
  retire; it is the active bring-up frontier.
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
| HelioV Blueprint | Builds the deterministic chunked world as `helio::MeshUpload`, adapts the focused UI4 input route into shared Helio navigation, and runs the WGPU event/presentation loop. |
| `helio-controls` | Owns platform-neutral navigation actions, held/delta reduction, fly-camera behavior, and lens/pose handoff; it owns no UI4 or WGPU policy. |
| WGPU custom backend | Validates the WGSL/package and position+UV pipeline interface, maps Helio texture residency to a generic opaque buffer, retains the texture/view/sampler bind group, and records render-pass bindings plus `draw_indexed`. |
| VMX vGPU broker | Owns generation-tagged resources, validates every vertex/index/texture byte range and the admitted sampled-package interface, and never receives Helio/voxel concepts. |
| TRUEOS Render | Copies the bounded position+UV/index/texture snapshot into resident Render0 storage and installs the generic RGBA8 surface and sampler table. Constant-colour indexed work mints an exact release fence; the sampled PS currently does not retire. |
| UI4 | Retains the old front across resize, accepts only a release matching its private write lease, commits/publishes that generation, and reports physical SURFLIVE. |

The current AOT package accepts clip-space `Float32x3` positions, `Float32x2`
UVs, and one nearest/repeat RGBA8 sampled texture. Camera projection is still performed in the
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
| Textures/views/samplers | One linear RGBA8 D2 allocation/view plus nearest-repeat sampler reaches authenticated state construction; the first physical sampler read still fails to retire | Retire the fixed mip-0 probe, then filtered sampling; afterward generalize mip/layer/view ranges, formats, clamp/linear sampler encoding, and GPU-native copy/clear operations. |
| Shader modules | One exact WGSL source is matched to an authenticated AOT package digest and represented by an opaque vGPU object | Generalize the build-produced package catalog and reflect compilation diagnostics/features for the Helio shader set. |
| Bind groups/layouts | One fragment texture+sampler group is live and validated end-to-end | Generalize typed descriptor tables, arrays, buffers, stage visibility, and dynamic offsets. |
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
2. The sampled shader/pipeline/position+UV/texture/sampler/bind-group/direct-draw
   package reaches Render0 but does not yet retire. Close the fixed-load then
   filtered-sample probe before claiming visibility. Afterward add a
   camera-uniform package and the buffer/binding variants needed to stop the
   remaining CPU projection compatibility upload.
3. The SceneDB partner-row lifecycle now covers every world chunk plus one
   transform/remove/regrow proof. Extend that identity into mesh residency,
   compaction, visibility, and indexed-indirect drawing.
4. Run a second Helio graph through the unchanged WGPU/VMX interface.
5. Texture decode, residency, upload, and exact slot binding now exist in the
   bounded probe. Extend that authority to the descriptor arrays and material
   buffers consumed by the full G-buffer after the sampler retirement gate.

Step 2's visible indexed frame establishes the first real graphics territory.
Passing steps 2–4 in their complete form establishes that the adapter is an
engine backend rather than a manually ported demo.
