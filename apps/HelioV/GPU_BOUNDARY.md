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
  the contract `wgpu-30-custom/vmx-vgpu-v4-clear-resize-surflive` so loss of that feature
  is a build failure rather than a runtime surprise.
- The Blueprint imports and exercises the generic VMX vGPU device, buffer,
  read/write, render-queue, submission-timeline, and wait operations.
- A buffer-first WGPU custom `Device`/`Queue` maps WGPU buffer creation and
  `Queue::write_buffer` onto those VMX handles. Its unsupported object classes
  fail explicitly rather than falling through to a hosted backend.
- The executable uses that WGPU pair to construct `helio-scenedb`'s real
  `SceneAuthority`, register Helio's canonical object partner, and run object
  insert/edit/remove/reinsert/despawn plus mirror flushes. The high-level
  `helio::Scene` is intentionally not constructed until texture/sampler
  support satisfies its real constructor contract.
- The voxel source produces Helio `MeshUpload` data. No Stratum runtime and no
  TRUEOS-specific renderer, scene store, or material implementation exists.
- UI4 presentation storage is no longer disconnected from vGPU. HelioV opens
  a normal streaming Blueprint frame, acquires its real write lease, imports
  that page-aligned allocation into its own PPGTT, and receives a custom WGPU
  texture/view. Dropping the texture revokes the mapping before cancelling the
  lease; device close is rejected while an imported target is live.
- WGPU command encoding, command-buffer submission, exact Intel retirement,
  and UI4 presentation are now proven for the generic render-pass clear
  operation. The surface mapping is consumed only after its cache-draining
  release packet retires, and HelioV waits for UI4's physical SURFLIVE event.
- Maximize/restore consumes UI4's resize event and stages its private
  replacement generation. HelioV begins and imports that exact new lease,
  submits and publishes its complete extent, and updates the render-loop
  projection aspect before UI4 replaces the previous SURFLIVE front.

## Exact runtime gap

The present VMX vGPU ABI is a safe resource-and-scheduling substrate. WGPU's
custom backend needs the following semantic operations before it can create a
real Helio `Device` and execute a graph:

| WGPU family | VMX vGPU today | Required adapter/ABI work |
| --- | --- | --- |
| Device, buffers, queue, timeline | Present and probed by HelioV | Map WGPU usage/mapping/lifetime and device-loss rules onto the existing opaque handles. |
| Limits/features/errors | Only coarse capability bits and device info | Publish WebGPU limits, format features, error scopes, and device-loss callbacks. |
| Textures/views/samplers | Absent | Add opaque texture allocation, view ranges, formats/usages, sampler descriptors, and copy/clear operations. |
| Shader modules | Absent | Validate Helio WGSL with Naga and introduce a build/runtime pipeline package that TRUEOS can execute on Intel. No fixed demo shader IDs. |
| Bind groups/layouts | Absent | Add descriptor tables referencing opaque buffer/texture/sampler handles with validated ranges and dynamic offsets. |
| Render/compute pipelines | Absent | Add immutable pipeline objects carrying shader, vertex, raster, depth, blend, target, and compute state. |
| Command encoder and passes | Render-pass clear records, submits, and retires through WGPU | Add generic copy, compute dispatch, render draw, indexed/indirect draw, and state binding packets. |
| Presentation | Exact UI4 lease import, submission release, publish, SURFLIVE acknowledgment, and transactional maximize/restore are implemented | Generalize the proven surface-consumption path from clear to shader-driven command buffers and continuously rendered frames. |
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
   lifecycle now exist in HelioV. Extract the adapter as a reusable crate while
   adding GPU-native buffer-copy commands so partner growth preserves live rows
   without CPU readback.
2. Add texture, sampler, pipeline, bind-group, and command-packet support sufficient for
   one texture-free Helio graph. Assign voxel colour using a normal Helio
   material; `PackedVertex` has no hidden colour channel. Texture/sampler
   objects are still required for Helio's 1x1 placeholder even though the
   voxel material itself is texture-free.
3. UI4 submission/present retirement is now proven with WGPU clear. Run the
   dynamic chunk lifecycle through shader pipelines: insertion, transform,
   removal, regrowth, and indirect drawing.
4. Run a second Helio graph through the unchanged WGPU/VMX interface.
5. Add texture residency/upload as a separate milestone.

Passing step 1 without drawing is useful engine territory: SceneDB and Helio
own the live scene and GPU-buffer lifecycle. Passing steps 2–4 establishes that
the adapter is an engine backend rather than another manually ported demo.
