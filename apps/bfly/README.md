# bfly

`bfly` is the on-demand TRUEOS Blueprint for the butterfly renderer. The two
SVG wing paths in `src/geometry.rs` supply the native outline and the shader
prototype.

## TRUEOS reference view

From the root of `TRUEOS-Blueprints`, run `cargo bp bfly` to build and publish
the Blueprint for a kernel that carries the matching shader hashes. In Shell3,
enter `§§bfly` to fetch and launch it. Set `TRUEOS_BLUEPRINT_SKIP_APPS_PUBLISH=1`
when building a local candidate before the matching kernel is deployed.

The native view draws four closed loops on the iGPU using the current
`IndexedDrawBatchV2` API. The build samples each cubic into 64 segments. The
window uploads the outline at startup and reprojects it after a resize.

## Cached wing renderer

`shaders/wings.*` contains the GLSL 4.50 isoline tessellation pass for baking
eight R8 masks. `shaders/cached-wings.*` contains the display pass that draws
two textured triangles per wing, rotates each wing around its fixed edge,
and selects a cached sampling level from its projected size. The eight panels
split along their longer axis and show the active sampling level.

From this directory, capture the host Intel iGPU preview with EGL, Pillow,
and FFmpeg. The capture uses 30 Hz sample times.

```sh
python3 tools/render_bfly.py --frames 60
```

The native Intel stage packages live in `shaders/trueos/` and travel in the
Blueprint's `.trueos.shaders` section. TRUEOS authenticates them against the
kernel's package hashes. The stages from the Intel RPL-S GPU (`8086:a780`)
match the stages from an offline compile for the Intel ADL-S rig (`8086:4680`).
The native bake and sampled-strip pipelines are pending. The TRUEOS window
still uses the outline view. See the [R8 cache contract](shaders/intel-r8-contract.md)
for the coupled host GPU run and the [Gen12 stage state](shaders/trueos/intel-stage-state.md)
for the register and resource bindings.

From this directory, verify the sources, packages, packaged Blueprint, and
kernel trust records with `python3 tools/package_shaders.py --blueprint ../../dist/bfly.bp`.
