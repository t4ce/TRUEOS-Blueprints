# CPU geometry / GPU raster experiment

`gpu-raster` keeps XPApp's current CPU array decoding, transforms, lighting,
texture generation and homogeneous clipping. It replaces the scalar pixel loop
with the existing Intel GLSL raster pipeline, using a prepared-vertex branch.
The feature is opt-in. Ordinary builds retain scalar rendering and the carrier
wake-up correction.

## Frame path

1. `Frame::prepare_indexed` validates and clips into retained CPU storage.
2. `staticgl_prepared` records the resulting triangles and an immutable snapshot
   of draw state and texture atlas. Vertex attributes already contain lit color,
   projective UV and eye-space fog distance. The shader does not repeat geometry
   work.
3. `prepared::Renderer` packs one vertex/state buffer and one index buffer for
   the batch. Texture buffers persist across frames; only changed textures are
   uploaded. Recorded draws retain their original texture if the game edits or
   deletes it before submission.
4. The new prepared-raster ABI decodes the batch, retains textures and drawable
   depth, and submits ordered draws/clears together into one UI4 surface.
5. `glFinish`, capacity flushes and `SwapBuffers` complete pending GPU work.
   Completing a render lease does not publish the window; the existing swap
   flow remains responsible for publication.

A batch holds 600 draw/clear commands. Larger frames flush and continue on the
same unpublished drawable. Color/depth attachments are not copied through CPU
framebuffers between draws. CPU framebuffer storage currently remains allocated
as preparation-owner storage; removing that allocation is not necessary for this
first experiment.

## Evidence and limits

Host tests cover unchanged preparation output, culling/scissors, clear masks,
viewport/depth/blend packing and prepared attributes. They do not establish GPU
image parity. Native shader source/binary/metadata checks and a custom-target
kernel build are also required.

`XPAPP GPU RASTER` records acquire time, upload/submit time, explicit wait,
geometry bytes and changed texture bytes. Submission is synchronous, so GPU
completion time is included in upload/submit until separated by kernel timing.
Do not interpret the explicit wait alone as GPU raster time.

The experimental feature explicitly rejects `glReadPixels`; GPU attachment
readback is not wired. Scalar builds retain their existing readback support.
Hardware output, depth-edge behavior and frame timings need a matched-kernel
live run before this becomes the default. A few-millisecond raster time is a
measurement target, not an established result.

## Building

Host checks:

```sh
cargo test -p xpapp --lib --features gpu-raster --offline
cargo check -p xpapp --features gpu-raster --offline
```

The current Blueprint packager reads feature directives from the entry source.
For an opt-in packaged build, temporarily prepend this line to `src/main.rs`:

```rust
// trueos-blueprint: features = ["gpu-raster"]
```

Then, from TRUEOS-Blueprints, run:

```sh
TRUEOS_BLUEPRINT_SKIP_APPS_PUBLISH=1 cargo bp apps/xpapp
```

Remove the directive afterward. The staged artifact in TRUEOS
`bld/xpapp-usability/prepared-raster/xpapp-gpu-raster.bp` is built this way.
It requires the matching kernel and shader package digest; do not launch it on
an older kernel. The previously verified scalar artifact is saved alongside it
as `previous-xpapp.bp`.
