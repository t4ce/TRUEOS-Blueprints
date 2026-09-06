# Radial sampling for Protean Clouds

F6 now defaults to automatic radial sampling. **Space** switches between that
mode and native resolution; the performance log identifies the selected mode.
The cloud density, marching steps, lighting, mouse controls and displayed image
size remain the same. Sample positions change, so this is a spatial quality
tradeoff. No previous-frame history or motion estimation is used.

At or below 1280×720 pixels the renderer stays native. Above that area it
smoothly reduces the sample dimensions, up to 2× per axis. A 2560×1440 window
uses a 1280×720 sample image: 921,600 expensive cloud evaluations instead of
3,686,400. A second, inexpensive pass reconstructs the full image from four
neighboring RGBA8 samples. It runs on the GPU, with no frame readback.

Near the focus, sample spacing approaches one output pixel. Away from it,
spacing increases smoothly. The focus is the projected tunnel centerline eight
world units ahead, including the shader's camera rotation and mouse offset.
This geometric estimate follows the tunnel path; it does not detect the
brightest opening or track the viewer's gaze. Near an image edge the focus disk
shrinks to stay inside the viewport.

## Coordinate mapping

All positions here are in full-image pixels, with the origin at the top left.
For focus `c`, radius `R`, boost `b` between 1 and 2, and output position `p`:

```text
d = p - c
r = length(d)
t = clamp(1 - r/R, 0, 1)
sample_position = c + d * (1 + (b - 1)*t*t)
```

Multiply that position by `sample_dimensions / output_dimensions` to address
the sample image. Subtract half a pixel before the bilinear lookup. The cloud
pass applies the inverse mapping before evaluating the original `mainImage`.
Five fixed Newton iterations solve that inverse only on the smaller image.
The full-size reconstruction uses the direct formula, with no iterative solve.

The mapping and its first derivative meet identity at the disk boundary.
Inside the disk its radial derivative is
`1 + (b - 1)*(1 - r/R)*(1 - 3*r/R)`, bounded below by 2/3 at the maximum boost.
It cannot fold over or leave holes. At the center the derivative is `b`, giving
1:1 sample pitch when both the reduction and boost are 2. Redistributing the
fixed sample budget makes the transition annulus coarser: up to about three
output pixels per sample radially, returning to two outside the disk. This
costs peripheral detail; it does not create additional true samples for free.

## Runtime and reuse

The unchanged 64-byte Blueprint request selects the reviewed shader and a
native-resolution flag. TRUEOS constructs the extra 32 GPU-uniform bytes and
owns the scratch allocation and its addresses. The reviewed F6 artifact has
three pointer bindings; F1–F5 keep their previous layouts and executables.
Package, executable and SPIR-V authentication, target checks, SIMD16 and zero
scratch/SLM admission remain enforced.

The cached sample image uses ordinary PAT0/WB storage; the display output uses
the existing PAT3/UC policy. One mutex owns the sample image through both passes.
Every bounded row batch must retire before the next starts. At 1440p there
are eight cloud batches and four larger batches for the cheap reconstruction. A partial frame
cannot be published, and an accepted submission that fails to retire retains
its backing and quarantines reuse. Resizing reuses capacity or retires the old
allocation before freeing it. The shared cache retains its largest allocation
for subsequent frames/windows (about 3.52 MiB after a 1440p frame).

The coordinate functions live separately in TRUEOS's
`tools/shadertoy-cpp-offline/foveated_coordinates.clcpp`; the four-tap buffer
lookup is in `foveated.clcpp`. The adapter inlines both into authenticated source.
The coordinate formula is reusable for a Picasso fullscreen material sampling
a similarly warped effect texture. Geometry rendering would also need a
matching sample projection. This change integrates ShaderToy's compute path;
it does not add Picasso material support or enable hardware variable-rate
pixel shading.

## Local verification

The proof uses the production SPIR-V, the pinned Intel OpenCL driver and
`-cl-fast-relaxed-math` on the local UHD 770. It measures dispatch plus completion,
including both passes and row boundaries; it excludes image readback and UI
publication. The final 2560×1440 medians across 14 warm frames were:

| Mode | Cloud pass | Reconstruction | Total |
| --- | ---: | ---: | ---: |
| Native | 216.87 ms | — | 216.88 ms |
| Uniform reduced | 56.11 ms | 1.72 ms | 57.91 ms |
| Radial focus | 55.96 ms | 1.68 ms | 57.65 ms |

That is about 3.8× faster than native, with essentially the same measured cost
as uniform upscaling. An odd 1441×2561 portrait case improved from 247.85 to
66.39 ms. Native output matched the preceding production kernel byte-for-byte
across all 16 frames; automatic mode at 641×361 also matched native exactly. These are host-driver
measurements, not bare-metal frame times or a claim about matching GPU clocks.

Thirteen time/mouse samples through 120 seconds had full-image mean absolute
RGB errors below 0.12/255 against native. In a square extending ±20% of the
focus radius, radial sampling reduced mean error by 36.5% compared with
uniform half-resolution sampling. The production coordinate functions are also
host-tested for monotonicity, viewport bounds, boundary continuity and inverse
accuracy (less than 0.002 output pixels). This is not a guarantee against
shimmering or detail loss in every possible animation state.

Reproduce from TRUEOS (output directories must exist):

```sh
make -C tools/shadertoy-cpp-offline benchmark-protean-clouds
FOVEATED_MODE=radial SHADERTOY_BUILD_OPTIONS=-cl-fast-relaxed-math \
  bld/tools/benchmark_protean_clouds \
  ../TRUEOS-Blueprints/apps/shadertoy/assets/protean_clouds/kernel.spv \
  /tmp/clouds-radial 2560 1440
```

Use `FOVEATED_MODE=full` or `uniform` for comparisons. Select the local Intel ICD
as described in TRUEOS's offline tool README. Proof frames and timings from this
change are under `TRUEOS/bld/shadertoy-foveated/final/`.

Run `python3 tools/test_shadertoy_dispatch.py`,
`python3 tools/shadertoy-cpp-offline/test_foveated_mapping.py`, and
`python3 tools/shadertoy-cpp-offline/test_blueprint_packages.py` for the CPU,
coordinate and admission checks. Rebuild the kernel and Blueprint together;
the F6 artifact and trusted hashes changed.
