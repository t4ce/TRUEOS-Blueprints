# ShaderToy visual Blueprint

See [compiler settings](COMPILER_FLAGS.md) for the backend flag that produced the
large math-performance win and the precision tradeoffs we checked.

The Blueprint owns nine authenticated programs with **15 selectable views** in `assets/`.
The six GLSL ports contain raw GLSL (`input.glsl`), generated C++ (`kernel.clcpp`),
Zebin, SPIR-V, a bake manifest, and an ABI contract. The three native C++ programs
use `input.sources.json` to archive their entry source and any headers by baked
source path. The corresponding `.stpkg` includes all six files and is embedded
in the Blueprint. `build.rs` rejects stale packages.

The app registers these packages through bounded, window-owned transfers before
rendering. TRUEOS checks the package layout and authenticates the Zebin and
SPIR-V against kernel-owned SHA-256 hashes. It validates the PCI device and
revision, ABI and ELF layout before copying the executable into kernel-owned
DMA memory and mapping it into the dispatch PPGTT. GPU addresses, surfaces and
dispatch geometry remain kernel-owned. The build checks the source and
provenance packaged alongside each executable.

The app opens a UI4 visual frame at 30 Hz. Use Left/Right to cycle all 15 views, or F1–F12 for the first twelve; Escape closes the app. For F6, **Space** toggles radial sampling and
native resolution. Per-frame calls still carry the existing 64-byte request. Registering an incomplete, modified or unknown package cannot
make a new shader executable. There is no embedded/filesystem shader fallback.

Every five seconds the app logs actual throughput (`fps_x100`, where 3000 means
30 FPS) and average/maximum render-and-publish wall time in microseconds. Resize
and shader changes reset the interval. These timings exclude the frame-begin
cadence wait, so slow rendering can be distinguished from the requested 30 Hz.

F6 uses a smaller render target with smoothly concentrated samples around the
tunnel focus. At 1440p it evaluates 1280×720 cloud samples and reconstructs the
full image on the GPU; small windows remain native. See [radial sampling](FOVEATED_RENDERING.md)
for the math, quality tradeoff, local measurements and Picasso reuse boundary.

Nguyen, Palette Grid, Cosmic Strands and Protean now use a separately pinned
relaxed-math backend profile; Mandelbrot and cube-field executables are unchanged.
The kernel renders bounded row batches and publishes only the completed image.
This preserves resolution and mouse coordinates while avoiding one enormous
fullscreen dispatch. Native-intrinsic comparisons, 1440p measurements and
long-running precision tradeoffs are recorded in TRUEOS's
`tools/shadertoy-cpp-offline/RUNTIME_PERFORMANCE.md`.

For a local package build from TRUEOS-Blueprints:

```sh
TRUEOS_BLUEPRINT_SKIP_APPS_PUBLISH=1 cargo bp shadertoy
```

Rebuild the kernel and Blueprint together so the kernel can admit the new F7
shader package.

From TRUEOS, regenerate and reproducibly bake the six shaders with the existing
locked compiler toolchain:

```sh
make intel-gpu-bake-shadertoy-cpp
python3 tools/shadertoy-cpp-offline/test_blueprint_packages.py
```

`TRUEOS_BLUEPRINTS_ROOT` can select a different Blueprint checkout. The bake
script writes payloads into these assets and keeps only contracts and package
hash/length metadata in TRUEOS. `package_blueprint.py --check` verifies packages
without modifying them; `--update-trust` refreshes the kernel package hashes
following review. A new candidate still needs explicit catalog admission.

## Gallery and sine wave visualizer

The artistic views formerly reached through Shell2 `cpp` are now here. Shell2
`cpp` is retired; **`win`** opens only the 30 retained UI4 windows. `win status`
and `win stop` inspect and close that window demo; focused Escape also closes it.

| Selection | View | Registered program |
| --- | --- | ---: |
| F1–F6 | Existing six GLSL ports | 1–6 |
| F7 | Sine wave visualizer | 7 |
| F8 | Four-panel gallery | 8 |
| F9 | Aurora | 8 |
| F10 | Julia | 8 |
| F11 | SDF | 8 |
| F12 | Voronoi | 8 |
| 13, arrows | Retro Sun | 8 |
| 14, arrows | High Wisps | 8 |
| 15, arrows | ParticleCraft | 15 |

The four panels occupy quadrants of one dispatch. The six standalone gallery
views share that same program and its original mode uniforms. High Wisps keeps
primary-button painting with an interpolated 32-point brush history per window;
leaving the view clears its strokes. Secondary-button window movement stays UI4-owned.

F7 uses the shader's frame time to generate a sine wave on the GPU. The wave
animates the visualizer's ribbons and circular prism even when the system is
silent.

ParticleCraft retains its three GPU stages and per-window particle state. It
reuses the shader's existing 1/2/4-pixel block output to keep the former gallery's
sample count at each window size, including 1280×720 samples at 1440p. This keeps
the shared ShaderToy surface full-sized with no extra upscale pass. Switching
away releases the particle allocation; re-entering starts a new simulation.

ShaderToy registers its authenticated packages for each window. The sine wave
visualizer uses the same frame parameters as the GLSL ports.

After rebaking Gallery or ParticleCraft with its existing bakery target, update
its Blueprint copy from the TRUEOS checkout:

```sh
python3 tools/shadertoy-cpp-offline/import_native.py
python3 tools/shadertoy-cpp-offline/package_blueprint.py --update-trust
python3 tools/shadertoy-cpp-offline/test_blueprint_packages.py
python3 tools/test_shadertoy_catalog.py
python3 tools/test_win_command.py
```

The importer checks every source/header against the bake manifest. Review the
new artifacts before refreshing the kernel trust hashes.

## New candidates and cube-field update

F6 selects **Protean Clouds** (`assets/protean_clouds/input.glsl`), now admitted
with its reproducibly baked, zero-scratch contract. Its lighting probes now use
four-step interpolation, retaining the original density, ray steps, fog and
full-resolution shading function. That lighting change measured about 1.6× faster rendering. The newer
backend-math and row-dispatch update measures another 4.48× at 1440p locally
(972 to 217 ms); it still needs a bare-metal performance check. Relaxed math
has tiny differences in the short tests and pattern drift at large time values.
`assets/protean_clouds/original.glsl` preserves the previous source. See
TRUEOS's `tools/shadertoy-cpp-offline/PROTEAN_CLOUDS_PERFORMANCE.md` for timings,
quality comparisons and the benchmark. Rebuild the kernel and Blueprint together
to install the updated shader package and its trusted hashes.

The other two sources are retained in `assets/candidates/`:

- `hex_array_pulse/input.glsl`: generated artifacts are retained too, but the
  best tested version still requires 2048 bytes of scratch per hardware thread.
  It has no executable catalog ID while the dispatcher supports zero scratch.
- `aiekick_sphere/input.glsl`: needs a cubemap, a mipmapped 2D image, and the
  corresponding compute-side channel/sampler bindings. Representative textures
  are acceptable; this is a remaining runtime feature, not an original-asset match.

F2 keeps the same cube-field scene using analytic sphere intersection, traversal
of only the affected grid cells, and analytic face normals. It preserves the
column heights, camera and lighting. `assets/cube_field/original.glsl` retains the
old 128-step SDF source for comparison. See the TRUEOS toolchain's
`tools/shadertoy-cpp-offline/CUBE_FIELD_PERFORMANCE.md` for measured differences.
