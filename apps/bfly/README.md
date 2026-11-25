# bfly

`bfly` is the on-demand TRUEOS Blueprint for the animated butterfly outlines.
Run `cargo bp bfly` from the root of `TRUEOS-Blueprints` to build and publish
it. On the rig with the matching kernel, enter `§§bfly` in Shell3.

The app uploads both wing designs together. The original has three open cubic
curves and the alternate has five. Each has a straight hinge connector shared
by both mirrored sides. The hull shader selects the design by seed. It uses
four segments per cubic below a 16-pixel projected span, then doubles the
segment count from 16 up to 1024 as the span grows. It
emits one segment for the connector. The shader generates each stage-plane
orbit from three to seven cubic segments and turns the butterfly along its
full 3D motion. The orbits have a nominal 12-second lap with staggered
starting phases.
Each butterfly has its own seeded flight rate from 85% to 115%.
The orbit anchors can land anywhere in the assigned stage rectangle. Each
butterfly also drifts smoothly up to half a stage closer to or farther from
the camera, varying its projected size as it flies.
The domain shader evaluates and projects the flapping outlines. Wings can
cross panel edges and are clipped at the UI4 window boundary. The outlines
are 4 px wide for the 1024-segment tier, 3 px for the 512-segment tier, and
2 px for the remaining tiers. A butterfly below four pixels in either dimension
keeps only its shared hinge, drawn as a two-pixel-long line. UI4 requests a 30 Hz
presentation cadence. Three draws in one submission select the raster widths.
The GPU handles flight and wing tessellation
using the window size, clocks, and flock records supplied to Picasso.

Left-click in the scene to spawn one butterfly at the farthest size. More
butterflies at smaller sizes encourage independent growth after staggered
dwell times. A growth step can end at an intermediate size and location,
typically moving half to one of the original steps. A butterfly's rate sets
its growth transition duration, from about 3.8 to 5.2 seconds. The live flock
starts with four butterflies of each design and can grow to 80. Arrivals keep
the counts balanced, and each butterfly keeps its design throughout its flight.
Both designs share the same submission.
At capacity, a click queues a newcomer while an existing butterfly flies out.

At 15 seconds, the two halves of the first split trade places over four
seconds, carrying all their nested regions and butterflies with them. The
next split trades its halves 30 seconds later. The sequence repeats through
the seven splits. The wing surface clears to transparent. Below it, UI4
scans out a separate black RGBA8 gradient. The top-right half and center
diagonal are transparent, and alpha rises smoothly to 33% at the bottom-left
corner. The GPU initializes the gradient once per background buffer and
rebuilds it when the window changes size.
Each tessellated outline segment uses one opaque primary color, cycling red,
green, and blue.
Downstrokes leave smoke-textured traces as the wing outlines move away from
pixels. The GPU selects a vertical strip by the outline's projected size
tier, starting with the central strips for the largest butterflies. It uses
the 64×128 R8 texture baked from the dark left half of `tools/vfx.jpg`, rotated
90° counterclockwise, with the sample anchored to screen coordinates. Smoke
density sets the initial trace opacity up to 25%. The previous frame supplies
coverage, which fades within eight frames. Current RGB outlines draw above
the traces on both strokes.

The focused window accepts F1 for the original untextured gray trail, F2 for
the default smoke texture, and F3 for the 512×512 R8 cloud. In F1, outlines
leave a neutral-gray trace at 25% opacity on both strokes and fade within
eight frames. The cloud brightens low-density details and leaves slate-blue
traces that fade within 16 frames. Each wing deposits new cloud traces on
downstrokes at up to 25% opacity. Larger projected outlines sample wider
parts of the cloud. The iGPU uploads the 256 KiB mask on the first cloud
render after boot and shares the resident copy across later frames and runs.
Existing traces keep fading when you switch.

F4 draws the separate closed SVG contours with full-size RGB outlines over
85%-opaque insets. At crossings the last drawn fill supplies the pixel color
and alpha. The original design uses a red-to-yellow fill biased toward yellow,
and the alternate design uses a purple-to-red fill biased toward purple. Each
butterfly's seed selects one color for both wings throughout its flight. The
shader scales each fill about its hinge to 80% of the authored area in the
wing plane, leaving a transparent margin inside the outline.
Both passes share the same motion, projection, and GPU submission.
F5 adds the original F1 outline geometry on top of F4 in that same submission.
Returning from F4 or F5 to a trail mode starts with a fresh history.

The display backdrop drifts through green, cyan, blue, and purple at 25–75%
HSL lightness. It starts from the current Pipe A bottom color when available.
The app picks adjacent hue/lightness swatches every three to five seconds and
fades between them using the display-background API. A green or light-blue
backdrop fades in the Moon in the top-right corner. Dark blue fades in Earth
at the same position. Each sprite uses 512 native pixels when it fits. The
two grayscale R8 masks keep the transparency from the source PNGs and share
one 512 KiB GPU upload. The background samples the backdrop at 8 Hz and
publishes only when the sprite or window size changes. The GPU updates just
the sprite rectangle after the buffer's first draw. The wings keep their own
30 Hz schedule.

A butterfly flaps fast as it grows or flies away, easing into that motion
over 0.75 seconds. As it reaches its new size, it eases back to the slow flap
over the same duration. Slow flaps use a four-second wingbeat, a 32° angle in
either direction, and a 0.135-turn offset between the sides. Fast flaps use a
synchronized 1.25-second wingbeat, a 4.5° total gap at the top, and 16
deterministic downstroke sweeps between 75° and 150° per wing.

The active Blueprint carries the direct tessellation stages, the history
compute shaders, and the R8 textures in `.trueos.shaders`. The manifests
and payloads live in [shaders/trueos](shaders/trueos).
From this directory, verify the package and kernel trust record with
`python3 tools/package_shaders.py --blueprint ../../dist/bfly.bp`.

On a Linux EGL render node, capture the first swap with
`python3 tools/render_bfly.py --direct --size 1280 720 --start 14 --frames 180`.

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
