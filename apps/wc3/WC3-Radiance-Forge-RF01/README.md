# Radiance Forge RF01 — WC3 hints → an independent compute light engine

This replaces the direction of the earlier compatibility-only proposals. The guest owns simulation; the renderer owns the picture. GL state is one input to an independent scene/material/light system, not its feature ceiling.

## What is actually supplied

Four C++-for-OpenCL kernels, a shared scalar arithmetic/traversal core, a C++ reference BVH builder and scene validator, 21 executable host tests, and a tiny synthetic scene with 65 lights. The kernels implement spatial light lists with correct overflow, geometry-tested direct lighting/shadows, one-bounce diffuse transport, and a separate water appearance pass (waves, Fresnel, attenuation and scene-ray reflections).

This is candidate engine source, NOT a completed renderer, a native shader publication, a Warcraft frame capture, a Nanite implementation, or a tested bare-metal executable. Geometry/material capture, native dispatch admission and final graphics composition are specified in `docs/INTEGRATION.md`; those changes have NOT been made to your repositories. No existing compatibility handlers are replaced.

## Checked source baselines

* TRUEOS-Blueprints: `ac98aba6641f3e77f22982abb7161a36be84f8c1`.
* TRUEOS: `bf7c0002be092f2118b2e3f256a5a13581e00619`.
* Bakery: `tools/intel-gpu-bakery/bake.py`, its strict `adls-4680-r0c-cpp.json` profile and `toolchains/adls-cpp-proof.lock.json`.
* Runtime reference: `src/intel/gpgpu/rcs/shadertoy.rs`. Its payload writer is an example to study, NOT a compatible dispatcher for these kernels.

The current Blueprint has ambient/diffuse/position state, `glDisable(GL_LIGHT0)`, and a bounded GL write journal. This package does not roll that work backwards.

## 1. Run the supplied reference now

```sh
python3 tools/check_local.py
```

From this package's root. Requires a host C++ compiler and a Clang supporting C++ for OpenCL. Produces `validation/report.json`, synthetic scene/reference buffers and frontend-only LLVM bitcode under `validation/`.

The delivered report records the actual checks performed in this conversation: 21 host tests, 5,000 BVH-versus-brute-force rays, and all four kernels compiled to SPIR64 LLVM bitcode with Clang 17. This is NOT the repository's pinned Clang 21 and NOT an IGC run. AddressSanitizer and UndefinedBehaviorSanitizer also passed on the host reference; see `validation/sanitizers.txt`.

## 2. Bake with your existing C++ bakery

Keep this supplied package in place. The wrapper passes its source paths to the
TRUEOS bakery, which records that external provenance while writing only
no-publish candidates below the TRUEOS build root. Do not create a second
source copy or revert the current worktree.

From the TRUEOS root, with your established CLANG, LLVM_SPIRV, OCLOC and OCLOC_LD_LIBRARY_PATH environment:

```sh
python3 /path/to/WC3-Radiance-Forge-RF01/tools/bake.py --trueos-root . --dry-run
python3 /path/to/WC3-Radiance-Forge-RF01/tools/bake.py --trueos-root .
```

The wrapper invokes the real upstream bakery CLI. It keeps the existing target, revision, SIMD16, zero-scratch/zero-SLM and reproducibility gates. It does not publish, auto-admit new packages, modify the lock or request relaxed math. Candidate outputs remain in `bld/wc3-radiance-forge/<kernel>/`.

Success must include all four separate expected entries. The compiler's `.ze_info` and generated `.contract.rs` decide actual GRF/BTI/payload/entry offsets. None are guessed here. A bake failure is actionable compiler/ABI evidence, not a reason to convert the engine into eight fixed-function lights.

## 3. First bare-metal milestone

Implement the narrowly scoped RF01 native fixture probe in `docs/INTEGRATION.md`, separate from WC3 startup. Upload the supplied 12,336-byte fixture scene and dispatch light-list build → direct → indirect → water to private buffers, fully retiring each before reuse. Read back to files and compare:

```sh
python3 tools/wc3-radiance-forge/tools/compare_readback.py \
  tools/wc3-radiance-forge/validation/fixture.direct.f32 /path/to/direct.readback.f32
```

Repeat for indirect and water. Light-list u32 outputs must match exactly (including overflow metadata). Float tolerances are diagnostic starting values, not permission to mask wrong hits. Inspect mismatches and preserve native compiler options.

Only after that fixture works connect the same scene/light core to draw capture. This development track does not depend on completing the guest API surface or reaching the first game frame.

## Important limits of RF01

* Threaded BVH construction is currently on the host; traversal is compute code. No GPU geometry virtualizer, BVH refitter, temporal denoiser, radiance cache or streaming system is claimed implemented.
* RF01 indirect is one cosine-sampled diffuse bounce plus next-event direct lighting at the hit. No recursive transport, reservoir resampling, specular GI or importance-sampled emissive triangles.
* Ray geometry is opaque, two-sided proxy triangles with per-triangle constant materials. Do NOT include alpha-cutout foliage as opaque rectangles in a production ray scene. Explicit proxy/material classification is required.
* Finite-range local lights use a smooth cutoff and point-source visibility, not area-light penumbrae. Sun soft-shadow directions use a small-angle disk approximation.
* RF01 light lists have 32 inline indices per 64-surface cluster. Overflow deliberately evaluates ALL input lights; it never drops lights. The reference admission profile allows 4,096 input lights. Neither value comes from the GL slots.
* Water currently uses two analytic wave slopes, Schlick Fresnel, Beer–Lambert attenuation, and a scene reflection ray. Below-water radiance/path length are supplied inputs; a depth-based refraction/shoreline pass is future work. No fluid simulation, full microfacet BSDF or energy-complete multiple scattering is claimed.
* RF01 outputs linear floating-point radiance, not display pixels. World up is +Z. Inputs in the fixture are authored synthetic data.

See `docs/ARCHITECTURE.md` for the actual target: GPU-driven retained geometry, independent many-light records, multiscale radiance caches, selective software rays, stylized material replacement and water, with UI/gameplay visibility protected.
