# bfly

`bfly` is the on-demand TRUEOS Blueprint for the animated butterfly outlines.
Run `cargo bp bfly` from the root of `TRUEOS-Blueprints` to build and publish
it. On the rig with the matching kernel, enter `§§bfly` in Shell3.

The app uploads five four-point patches once. Three describe the SVG's cubic
curves and two describe the straight hinge closures. The hull shader selects
8 to 1024 segments per cubic from the projected size and leaves each closure
at one segment. It also generates a closed 3D path with three to seven
cubic segments for each butterfly and uses the path tangent as its heading.
The paths complete a lap every 12 seconds, with staggered starting phases.
The domain shader evaluates and projects the flapping outlines. Wings can
cross panel edges and are clipped at the UI4 window boundary. Butterflies
below four pixels in either dimension are culled. UI4 requests a 30 Hz
presentation cadence. The same draw handles flight and wing tessellation
using the window size and time already supplied to Picasso.

The active Blueprint carries the direct tessellation shader stages in
`.trueos.shaders`. The manifest and payload live in [shaders/trueos](shaders/trueos).
From this directory, verify the package and kernel trust record with
`python3 tools/package_shaders.py --blueprint ../../dist/bfly.bp`.

On a Linux EGL render node, capture one complete flight lap with
`python3 tools/render_bfly.py --direct --size 1920 1080 --frames 360`.

## Cached texture demo

The GLSL 4.50 texture demo remains available on the host. It uses
`shaders/wings.*` to bake eight R8 masks and `shaders/cached-wings.*` to
draw two textured triangles per wing. Capture it with EGL, Pillow, and FFmpeg.

```sh
python3 tools/render_bfly.py --frames 60
```

The [R8 cache contract](shaders/intel-r8-contract.md) records the coupled
host GPU run. The [Gen12 stage state](shaders/trueos/intel-stage-state.md)
describes the two texture-demo passes.
