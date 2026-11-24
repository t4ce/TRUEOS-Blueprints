# bfly

`bfly` is the on-demand TRUEOS Blueprint for the animated butterfly outlines.
Run `cargo bp bfly` from the root of `TRUEOS-Blueprints` to build and publish
it. On the rig with the matching kernel, enter `§§bfly` in Shell3.

The app uploads four four-point patches once. Three describe the SVG's open
cubic curves and one describes the straight hinge connector shared by both
sides. The hull shader selects 8 to 1024 segments per cubic from the projected
size and emits one segment for the connector. It also generates a closed 3D
path with three to seven cubic segments for each butterfly and uses the path
tangent as its heading.
The paths complete a lap every 12 seconds, with staggered starting phases.
The domain shader evaluates and projects the flapping outlines. Wings can
cross panel edges and are clipped at the UI4 window boundary. Butterflies
below four pixels in either dimension are culled. UI4 requests a 30 Hz
presentation cadence. The same draw handles flight and wing tessellation
using the window size and time already supplied to Picasso.

The two flap modes alternate in 30-second blocks with short blends at the
switches. The slow mode uses a four-second wingbeat, a 32° angle in either
direction, and a 0.135-turn offset between the sides. The synchronized mode
uses a 1.25-second wingbeat and a 4.5° total gap at the top. It cycles through
16 deterministic downstroke sweeps between 75° and 150° per wing.

The active Blueprint carries the direct tessellation shader stages in
`.trueos.shaders`. The manifest and payload live in [shaders/trueos](shaders/trueos).
From this directory, verify the package and kernel trust record with
`python3 tools/package_shaders.py --blueprint ../../dist/bfly.bp`.

On a Linux EGL render node, capture the switch between flap modes with
`python3 tools/render_bfly.py --direct --size 1280 720 --start 28.5 --frames 150`.

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
