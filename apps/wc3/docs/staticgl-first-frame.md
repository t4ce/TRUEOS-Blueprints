# First game-frame compatibility path

The captured draw contains 8,103 unsigned-short indices and a complete RGB5 mip
chain. Its live state enables lighting, fog, culling, scissor and depth. Removing
the old persistent `glLightf` blocker alone would not render those semantics.

The provider now consumes that state through a Rust fixed-function rasterizer.
Color/depth live with each HGLRC; the final image is submitted through the existing
sampled vGPU shader and UI4 presentation path. No vGPU ABI extension or RF01 kernel
deployment is part of this patch. Software shading prioritizes compatibility;
this is not yet a GPU-speed game renderer.

## Independent viewport prerequisite

`_ftol` previously addressed FXSAVE register data using physical TOP. FXSAVE data
slots are already in logical ST0..ST7 order; TOP only indexes the physical tag
bits. The fix reads logical slot zero and shifts logical data on pop while
updating the physical tag/TOP. A native save/restore regression checks both the
integer result and the remaining x87 stack with nonzero TOP. This addresses the
observed zero right/bottom edges and negative scissor sizes without overriding
the guest's viewport or scissor.

## Replay receipts

- `WC3 GL RASTER DRAW ... renderer=rust-fixed`: guest indexed geometry reached
  the rasterizer. `triangles` is the post-clip triangle count; `pixels` counts
  fragments written after scissor, cull, depth and alpha tests. Zero is a valid
  clipped/occluded draw and does not prove a renderer failure.
- `WC3 GL FRAME PRESENT ... reason=first-draw-preview ... gpu=completed`:
  the first draw's current buffer completed the sampled GPU submission. This is
  a preview, not proof the guest completed its frame.
- The same receipt with `reason=swap` follows the guest's swap call. `reason=finish`
  follows `glFinish`; `reason=clear` publishes a color clear. `draws` is cumulative
  for the context, and `nonblack_pixels` describes the submitted image.
- UI4 publication occurs after successful provider return. A GPU completion
  receipt alone does not prove physical scanout; check the visible window and
  any subsequent UI4 error as well.

## Bounds and known boundaries

The raster frame permits 4,147,200 pixels (including 2560x1440). Indexed triangle
lists permit up to one million indices. Complete mip chains are required when
selected by the minification filter. Invalid guest reads or incomplete texture
state fail before drawing. An initial color/depth allocation is deterministic
black/1.0; normal subsequent clears obey the guest's scissor and depth mask.

The renderer implements the current single-texture GL 1.1 import path, not all of
OpenGL. Two-sided lighting, front/auxiliary buffers and unsupported array types
remain frontiers. The window preview/clear/finish publication policy is deliberate
bring-up visibility; only the swap receipt signifies a guest swap. Internal
color/depth remain owned across publications.

## Host validation

From the blueprint repository:

```sh
cargo test -p wc3 --lib --offline
cargo check -p wc3 --bin wc3 --offline
```

The integration fixture uses the game's 36-byte interleaved vertex shape and
checks transformed/lit/fogged mipmapped pixels, depth retention across draws,
alpha rejection, blending, duplicate-index remapping, and rejection without
partial framebuffer writes. Raster unit tests additionally cover homogeneous
clipping, perspective UVs, shared triangle edges, texture environments, mip
sampling, scissor and offset/empty viewports. These tests do not call the GPU ABI.
