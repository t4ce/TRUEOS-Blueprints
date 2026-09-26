# RF01 native integration — patch instructions, not an already-applied patch

## Scope and success condition

Implement a fixture compute probe first, independent of guest startup. The deliverable is native radiance buffers with reproducible readbacks from 65 authored lights, shadows, a diffuse bounce and water optics. No glLightfv/glEnable/provider/thunk changes are part of this milestone. The following new Rust files and functions are proposed implementation targets; they do not exist merely because they are named here.

## A. Keep the existing compiler architecture

Place this source tree in `tools/wc3-radiance-forge/`. Bake the four candidate kernels through the wrapper. Preserve the `8086:4680`, revision `0x0c`, SIMD16, zero scratch and zero SLM profile. Do not load OpenCL Kernel SPIR-V through Picasso's graphics shader path. It goes through llvm-spirv → ocloc/IGC → validated Zebin. The four native entries are:

| Kernel | Arguments, in exact source order | Output |
|---|---|---|
| rf_build_light_lists | scene_words: const u32*, light_lists: u32*, first_cluster: u32, cluster_count: u32 | 34 u32 per cluster |
| rf_direct | scene_words: const u32*, output: float4*, light_lists: const u32*, first_surface: u32, surface_count: u32 | 16 bytes per surface |
| rf_indirect | scene_words: const u32*, output: float4*, first_surface: u32, surface_count: u32 | 16 bytes per surface |
| rf_water | same as rf_indirect | 16 bytes per surface |

Exact BTIs, native pointer offsets, entry ranges, payload lengths and GRF counts come ONLY from the new baked contracts. The SIMD16 attribute is a request; contract validation must prove it survived compilation.

Publish only after reviewing artifact quartets and adding them to the repository's explicit admission/source-publication inventory. Do not overwrite a ShaderToy entry or recycle its package ID. Do not weaken the existing production verifier to accept unnamed shaders.

## B. Native operation and encoder

Add `src/intel/gpgpu/operations/radiance_forge.rs` and `src/intel/gpgpu/rcs/radiance_forge.rs`, registering them alongside the repository's existing operation/encoder modules. Add a private native fixture entry (proposed name `run_radiance_forge_fixture`) callable through the existing developer diagnostic routing; no production public opcode is assigned in this proposal.

Use `rcs/shadertoy.rs` for owner/retirement/interface-descriptor patterns, not for hardcoded payload offsets or destination format assumptions. That path's 2/3-binding and RGBA8 surface checks do not automatically admit our raw float4 outputs.

The new encoder must:

1. Check the target and package digest against its generated contract; bind only this owner’s mapped resource ranges. Clear the entire indirect payload before writing fields.
2. Fill explicit arguments by their generated contract offsets; fill all admitted implicit records and local-ID payloads exactly as the generated contract requires. Preserve zero-only fields. Do not assume 48/56/etc. from the copy/ShaderToy kernels.
3. Use 16×1×1 workgroups for the initial profile. For N work items launch ceil(N/16) groups; set implicit global offsets to zero because this ABI uses explicit first_surface/first_cluster. Never offset both.
4. Respect RAW buffer descriptor sizes. Destination float4 records are 16 bytes, not RGBA8's 4 bytes. Scene and list descriptors are read-only where required.
5. Fully retire and make list writes visible before direct lighting reads them. Queue order alone is not a memory/cache visibility guarantee. Use the existing native completion/flush/invalidate routines; verify the RAW UAV/data-cache producer-to-consumer path rather than copying an image-sampler barrier.
6. Retain all mapped scene/list/output/batch allocations until accepted submissions retire. On timeout, quarantine; do not free, overwrite or retry into the same storage.
7. CPU readback must use the existing GPU-to-CPU cache visibility path after retirement. Compare all bytes in list outputs, all float4 records in radiance outputs, and unchanged guard regions.

## C. Fixture data and dispatches

`validation/fixture.scene.u32`: 3,084 little-endian u32 words = 12,336 bytes, containing 129 surfaces, 65 local lights, 18 triangles and 11 BVH nodes.

Use independent private allocations (sizes exclude chosen guard padding):

* scene: 12,336 bytes, immutable until all four stages retire.
* lists: ceil(129/64) × 34 × 4 = 408 bytes.
* direct, indirect and water: 129 × 16 = 2,064 bytes each.

Dispatch list build with first_cluster=0, cluster_count=3. Dispatch the three radiance stages separately with first_surface=0, surface_count=129. First pass uses one workgroup, radiance stages each use nine. There is no UI4 acquisition during this raw-buffer proof.

Then repeat direct lighting in split ranges [0,64), [64,128), [128,129), keeping the same immutable scene and list resources. Output must match the one-dispatch reference. Partial ranges leave every other output record unchanged. Initialize output and surrounding guard padding to a recognizable bit pattern before each probe.

Expected list overflow = 65 in the first two cluster overflow words. This is intentional: each holds only 32 inline indices but the shader evaluates all 65. Do not 'fix' the fixture by reducing the light count.

Required probe receipt (values must be measured, not copied from this document):

`RF01 target=... package_hashes=... surfaces=129 lights=65 nodes=11`
`RF01 stage=... launched=... retired=... elapsed_us=... guards_ok=...`
`RF01 comparison=... mismatched_components=... max_abs_error=...`

Call elapsed_us a wall-time submit/retirement measurement unless genuine GPU timestamps are programmed. No FPS or EU-utilization claim follows from it.

## D. Connect to WC3 only at the capture boundary

In the current `apps/wc3/src/staticgl.rs`, capture the referenced data in `gl_draw_elements_static` BEFORE converting positions to position3/NDC or submitting through staticgl_triangle. Preserve raw homogeneous positions, matrices, indices, normals where present, UVs, colors, texture generations, draw order and render-state classifications. Do not use the last 256 `observed_writes` as a complete retained scene.

Introduce a new scene record layer under `apps/wc3/src/radiance_forge/` (capture, scene, classify, environment, materials). Make generation-owned copies of guest data before returning from the draw provider. Preserve clear/readback/finish/swap dependencies in the command transcript.

Known object-local geometry can be cached with immutable topology and an instance transform. A modelview alone does NOT uniquely identify the model and view matrices. For unknown poses work in common per-view coordinates; use inverse projection for clip-space reconstruction where valid. Retire frame-local geometry on the next frame; do not manufacture persistent world transforms.

Run the new lighting on surface records generated from the raster/depth path. The synthetic packet ABI can initially be populated by CPU for diagnosis, but production must not read back a full G-buffer merely to reupload it to compute. Add a validated GPU producer/storage-buffer contract. Support original UI/effect passes separately and publish only the completed composite at swap.

## E. Scene validation boundary

`host/scene.hpp::validate` is the host reference for a tightly packed immutable fixture. Port its checks into Rust before accepting external/guest-controlled scene buffers. Strengthen production validation with explicit finite coordinate/radiance bounds, unit scaling, nonaliasing, per-owner capacity budgets, validated primitive/material generations and ABI layout tests. Validate conservativeness of BVH bounds, forward escape links, child structure and leaf coverage, not just array lengths.

RF01 device code assumes admitted buffers. Its bounded traversal loop is NOT a substitute for buffer admission. GPU-generated geometry and refitted BVHs require equivalent producer guarantees.

## Not required for RF01

Completing every staticgl setter; implementing a Vulkan or OpenCL runtime inside TRUEOS; a screen-sized G-buffer; GPU texture sampling; a new hardware ray-tracing opcode; a full Nanite implementation; or a changed game executable. None is a dependency for this independent native lighting probe.
