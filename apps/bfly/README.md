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
full 3D motion. During ordinary flight, the body takes the shortest rotation
between half-second samples of its flight direction, capping turns at 360°/s.
The orbits have a nominal 12-second lap with staggered starting phases.
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
scans out a separate RGBA8 gradient. The top-right half and center diagonal
are transparent, and alpha rises smoothly to 33% at the bottom-left corner.
Static dithering smooths the 8-bit alpha steps in the prepared base frame.
BCS0 copies that frame once when the stage or size changes. The green stage
uses a white gradient. Stage swaps smoothstep the plane's hardware opacity
down and back up over 400 ms each, with the copy at zero opacity.
All live outlines are white. They use four segments per cubic in the smallest
outline tier and half the previous segment count in each larger tier. Line
width still follows the projected tier at 2, 3, or 4 px.

F2 follows the celestial stage. Sun uses Universe, Earth uses Twilight, and
Moon uses Sunrise. Each four-second wake crossfade begins at the background's
zero-opacity image swap. F3 combines Twilight with F5's inset fill and both
outlines, including the small interior segment. The wakes sweep the closed
F4 contours on both strokes. Sunrise uses 590 ms at 80% stretch,
Twilight uses 590 ms at 75%, and Universe uses 600 ms at 65%. Projected
flight travel takes up to 35% of the same sweep budget as the flap. The sky
pans with wing speed while the mask opens tapered gaps as the wake ages. One
retained 256×768 RGBA8 atlas holds the three 256×256 RGB tiles and the shared
R8 tear mask in the first tile's alpha. The ribbon shader blends the presets in a
single pass, and the live outline draws over it.

F1 uses the original neutral-gray line trace. It starts at 25% opacity on
both strokes and fades within eight frames.

F4 draws the separate closed SVG contours with full-size white outlines over
85%-opaque insets. At crossings the last drawn fill supplies the pixel color
and alpha. The original design uses a red-to-yellow fill biased toward yellow,
and the alternate design uses a purple-to-red fill biased toward purple. Each
butterfly's seed selects one color for both wings throughout its flight. The
shader scales each fill about its hinge to 80% of the authored area in the
wing plane, leaving a transparent margin inside the outline.
Both passes share the same motion, projection, and GPU submission.
F5 adds the original F1 outline geometry on top of F4 in that same submission.
Switching to F1 starts with a fresh history.

The display backdrop drifts through green, cyan, blue, and purple at 25–75%
HSL lightness. It starts from the current Pipe A bottom color when available.
The app picks adjacent hue/lightness swatches every three to five seconds and
fades between them using the display-background API. Green and light blue
fade in the Moon at the top left at two-thirds of its native size. Moon
opacity follows the higher-precision color sweep at up to 30 Hz. Dark blue
fades in the 512 px Earth at the top right. The remaining colors show the
native-size 512×512 Sun sprite centered in the frame. All three images stay
fixed in their prepared background frames. The wings keep their own
30 Hz schedule.
`tools/bake_sun_texture.py` prepares the grayscale premultiplied RGBA8 texture from
*Sun Emits a Solstice CME*, credited to NASA/Goddard/SDO. [NASA source](https://images.nasa.gov/details/GSFC_20171208_Archive_e001435)

A butterfly flaps fast as it grows or flies away, easing into that motion
over 0.75 seconds. As it reaches its new size, it eases back to the slow flap
over the same duration. Slow flaps use a four-second wingbeat, a 32° angle in
either direction, and a 0.135-turn offset between the sides. Fast flaps use a
synchronized 1.25-second wingbeat, a 4.5° total gap at the top, and 16
deterministic downstroke sweeps between 75° and 150° per wing.

The active Blueprint carries the direct tessellation stages, the history
compute shaders, and retained textures in `.trueos.shaders`. The manifests
and payloads live in [shaders/trueos](shaders/trueos).
From this directory, verify the package and kernel trust record with
`python3 tools/package_shaders.py --blueprint ../../dist/bfly.bp`.

On a Linux EGL render node, capture the first swap with
`python3 tools/render_bfly.py --direct --size 1280 720 --start 14 --frames 180`.

## Wake preview

Serve this directory locally and open the [Sunrise wake preview](http://127.0.0.1:8765/tools/twilight-lab/)
in a WebGL2 browser.

```sh
python3 -m http.server 8765 --bind 127.0.0.1
```

The preview uses the first closed F4 contour for both the outline and trail
of one fixed-size butterfly. The **Sky** selector offers Sunrise, Twilight, and
Universe (`tools/uni.jpg`) in their original RGB colors. A 256×256 R8 mask
drifts along the sweep and opens tapered gaps as the wake ages.
**Mask repeat** sets the size of those gaps independently of the sky texture.
**Tilt** rotates the mask relative to the sweep within the band from 175°
through 180° to −160°.
**Motion pan** controls how far each wing's sky texture slides with its flap
at the selected repeat.
Pause or scrub the flap phase while tuning texture repeat, coverage, opacity,
lifetime, falloff, motion stretch, and the tear mask. At 100% stretch the wake
follows the wing's sweep over the selected lifetime. **F2 baseline** loads
the original F2 wake settings and tint on the same contour. **Reset** restores
the tuned Sunrise settings and their starting phase. **Copy settings** copies
the parameters and flap phase as JSON.

<details>
<summary>ImageGen prompt for the R8 tear mask</summary>

The built-in ImageGen tool generated a transparent PNG. Its alpha channel was
resampled to 256×256 and stored as raw bytes in `tools/twilight-lab/wake-tear.r8`.

> Generate a new grayscale game VFX erosion texture on true transparency.
> Output one square flat 2D alpha mask for a small animated wing wake, seen
> straight on like a texture swatch. This is a material map only, no scene.
> Across the entire square, use white ink with alternating opaque islands and
> genuinely transparent, jagged missing strips. All strips run HORIZONTALLY
> left to right, with long tapered wisps and varied lengths, irregular
> soft-to-sharp torn brush edges. The strips look like narrow pieces shear off
> a fast moving ribbon, with several large clear missing slashes, each
> tapering to finer strands, plus irregular softer half-transparent streaks
> between. Aim for a network of angular elongated narrow gaps rather than
> round holes or isolated objects. White paint everywhere except the missing
> streaks and uneven translucency. Cover the square from edge to edge with
> connected white regions, including near each corner. Balance around 55%
> white and 45% see-through with transitions, keep the pattern similarly
> distributed over the full card so it can be sampled with any offset. High
> contrast and readable downsampled to 256 x 256, use roughly 10 to 18
> distinct horizontal bands, width of each varied. No outline around the
> square, no centered object, no global fade, no black or colored paint, no
> background color, no checkerboard baked in, no typography, no gradient
> lighting, no screenshot. Preserve alpha as the information. The structure is
> inspired by streaked missing sections along a curved animated sweep;
> generate only the straightened mask pattern.

</details>

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
