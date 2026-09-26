# Radiance Forge: renderer-owned light transport from WC3 hints

This document describes the proposed engine beyond the delivered RF01 kernels. Do not confuse proposals with implemented code.

## 1. Authority

Warcraft remains authoritative for simulation, animation timing, gameplay visibility, input, silhouettes and the original draw transcript. The new renderer is authoritative for inferred/authored materials, extra lights, indirect transport, reflections, water, atmosphere and display composition. There is no eight-light ceiling and `GL_LIGHTING == false` is not a veto on renderer-owned lighting.

Keep an optional original-style debug path to isolate import mistakes. Do not make full fixed-function visual conformance a prerequisite for developing the new engine. A synthetic scene and a captured scene should enter the SAME lighting pipeline.

## 2. Retained scene rather than replaying GL one draw at a time

Input GL calls are import events. Decode state and geometry into a retained scene plus per-frame updates. Stable topology/UV/material IDs can be cached; position/transform changes are versioned separately. Do not hash all dynamic vertices into a new permanent mesh each animation frame or use guest pointer equality as identity.

Asset mining supplies the valuable prior information: authored topology, materials, texture identities and attachment candidates. These are absent from a pure color buffer and should not be inferred from screen brightness when source records exist. Correlate imported texture content hashes and mesh topology with the asset database. Missing provenance selects an explicit inferred material, not a halt to all enhancement.

A hint carries value, coordinate domain, source/provenance, confidence, timestamp and generation. It is never silently promoted to a verified game semantic.

Modelview = view×model cannot be factored uniquely from the matrix alone. Build world-space identities from validated camera/object evidence; otherwise use common per-view geometry with frame-local caches. History is invalidated on unknown identity, camera cuts, geometry generation changes and disocclusion.

## 3. Geometry: Nanite-style principles, not a logo claim

Build bounded mesh clusters offline for stable assets (start around 64 vertices/128 triangles as a candidate, not a proven optimal size). Store cluster bounds, material spans, LOD/error estimates, immutable compressed topology and a ray proxy. Stream/map only needed pages into retained GPU storage. Use compute frustum/Hi-Z cluster culling and compact indirect draw records when the native ABI admits them.

For original low-detail meshes, do not create millions of microtriangles merely to resemble Nanite. Spend early compute on lighting, material detail, water and visibility. Replacement high-detail assets can use the same cluster stream later. Geometry virtualization and light transport share scene identities, but are distinct subsystems.

Ray representation: static terrain/building BLAS with instance TLAS; dynamic actor topology has a refit/update path; alpha foliage requires cutout-aware traversal or explicitly authored occlusion proxies. Never ray-trace every billboard quad as an opaque wall.

RF01 implements only host-built threaded BVH + compute traversal, not those streaming/culling/refit subsystems.

## 4. Light engine

Renderer-owned lights come from asset attachments, rule-driven effects/emissives, authored lights and GL/environment hints. Capacity is a renderer resource policy (RF01 reference 4096), not the number of GL slots. Deduplicate temporally stable sources using emitter/instance IDs; do not turn every bright texel or every duplicate draw into a new light.

RF01 direct uses conservative surface-cluster lists, exact point-light shadow rays and one disk-sampled sun shadow direction per surface/frame. Overflow is correct but expensive: evaluate every input light. Production replaces pathological overflow with a compacted spill list or a reviewed sampling estimator; never silently truncate it.

Target indirect engine: camera-local irradiance probes + surface radiance cache. An initial proposed grid is 24×24×8 = 4,608 probes. Updating 1/8 at 16 rays each means 9,216 probe rays per frame, before hit shading/shadow rays. This is a work budget, not a measured runtime or a promise of 60 FPS. Store at least directional irradiance (SH or small octahedral map) and distance moments for visibility weighting. Sky-only misses are sampled from the environment. Tag probe validity and lighting generations; move/update bricks rather than rebuilding everything.

Inject selected emissive patches as stable light records, or add explicit importance sampling with the correct PDFs later. Uniform cosine rays alone are noisy for small bright emitters. Do not claim a reservoir method or unbiased estimator without implementing its weights and validating it.

Primary shading combines direct + visibility-aware cached indirect + emission. RF01 has diffuse-only transport; modernity here is independent, geometry-aware light transport, not forcing every hand-painted asset through a metallic BRDF.

## 5. Materials and hints

Separate pigment/team tint/visibility/emission from legacy prelighting where evidence exists. No algorithm can uniquely recover albedo and baked illumination from their product alone. Use authored de-lit replacements when available; otherwise preserve an art residual and limit additional contrast rather than multiplying two full light solutions.

Missing normals do not forbid lighting. Generate geometric normals from triangles, reconstruct depth normals for a screen-space fallback, or supply authored replacement normals. Each has a confidence tag and known artifacts. Generated tangent detail is an artistic addition, not recovered original detail.

Suggested rule roles: terrain, stone/building, organic unit, foliage cutout, emissive attachment, water, UI, decal/effect, visibility mask. Keep rules versioned and visualizable; use verified asset hashes when available. No actual Warcraft hashes/IDs have been invented in this package.

## 6. Day/night and water

Environment hints drive a renderer-owned sun/moon/sky/exposure profile with smooth temporal filtering. Use a verified simulation-time/day-phase hint when available; otherwise treat color/direction changes as environmental observations, not an exact clock. An independently authored day/night mode remains possible but must be labeled independent. Do not advance a supposed game day using wall time while simulation is paused.

Water replacement shares scene lighting and visibility. Target chain: compute slope/normal field → reflection ray or hierarchical screen trace with scene fallback → thickness/refraction from opaque depth → Beer-Lambert transmittance → Fresnel reflection → shoreline foam from validated distance/thickness → optional low-resolution caustic contribution. Preserve gameplay silhouette and surface location initially.

RF01 provides only wave slopes, scene-ray reflection, Fresnel and absorption. Water throughput is not a simulator. Full depth refraction, shore foam, caustics and surface radiance history remain separate passes.

## 7. Presentation and protection of gameplay meaning

Keep UI and appropriate particle/decal/selection passes outside world relighting. Retain fog-of-war/visibility as a final authority on which sources and geometry may affect visible pixels. A hidden unit must not reveal itself through a new shadow, reflection, emissive spill or history ghost. This is not a limitation on appearance; it preserves the simulation's information rules.

For uncertain visibility, use only current permitted draw/capture geometry and expire hidden dynamic sources immediately. Offline static map geometry used for shadowing may also reveal unexplored content; gate its contribution by the game visibility policy.

Use private linear radiance resources, an explicit tone-map/display transform for the world, then original UI composition. Do not add a whole-frame exposure filter over already-composited UI. Swap publishes the completed composite once; readback/finish remain explicit flush points.

## 8. Native execution and performance

C++ for OpenCL → pinned Clang spir64 bitcode → metadata-preserving llvm-spirv OpenCL Kernel SPIR-V → ocloc/IGC Zebin → generated contract. Graphics stages retain the existing graphics compiler route. These are different SPIR-V environments, not interchangeable packages.

Respect the current SIMD16/zero-scratch/zero-SLM constraints. Split tracing, cache updates and shading into small kernels where register pressure demands it; do not solve spills by falsely marking the package zero-scratch. A future explicitly supported SLM path can enable group-shared reductions later.

RCS work is scheduled in bounded, independently retired batches. Begin the fixture with 129 surfaces; expand through 320×180 / 640×360 sample grids after native timing. Do not start at 1440p × all lights × all bounces. Full-resolution raster/GUI can coexist with lower-resolution lighting caches and edge-aware upsampling.

The documented physical bakery target is ADL-S 0x4680 rev 0x0c. Intel's published UHD770 specification is 32 EUs; 96 EUs describes a full Xe-LP slice configuration, not this SKU. Runtime fuse/topology evidence should drive dispatch budgeting. Software BVH rays do not need DXR/Vulkan ray-query support.

Required telemetry: input draws/triangles, cache hits, uploaded bytes, cluster list occupancy/overflow, generated lights, primary/secondary/shadow rays, visited BVH nodes, valid/history-rejected probes, dirty bricks, stage wall/GPU times (distinguish them), peak memory, retired/published frames. RF01 source currently tests results rather than collecting all these GPU counters.

## Reviewed upstream references

* TRUEOS `bf7c0002...`: tools/intel-gpu-bakery/README.md; tools/intel-gpu-bakery/bake.py; profiles/adls-4680-r0c-cpp.json; kernels/CPP_FOR_OPENCL_ARCHITECTURE.md.
* TRUEOS `bf7c0002...`: src/intel/gpgpu/rcs/shadertoy.rs; tools/shadertoy-cpp-offline/RUNTIME_PERFORMANCE.md. Existing ShaderToy work demonstrates why native math profiles and bounded retirement matter; its timings are NOT RF01 timings.
* TRUEOS-Blueprints `ac98aba6...`: apps/wc3/src/staticgl.rs. The recent bounded state journal is evidence of progress, not a substitute for geometry/material capture.
* Intel: Product Specification Comparison for UHD 770/0x4680 (32 EUs); Intel Xe GPU Architecture (full Xe-LP slice 96 EUs).
* Epic official documentation: Nanite Virtualized Geometry; Lumen Technical Details. Their geometry-streaming and software-ray/radiance-cache ideas inform the proposal, but RF01 does not implement Unreal's systems.
