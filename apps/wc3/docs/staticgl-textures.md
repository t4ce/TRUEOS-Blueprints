# Static GL texture batch

This batch implements the texture preparation sequence together, so the next
hardware replay need not stop separately at name generation, binding, each
parameter setter and the first upload. It does not claim a complete WC3 renderer.

`staticgl_texture.rs` models per-context names and bindings, deletion, the default
texture object, unpack state, texture parameters/environment, texture-coordinate
arrays, texture enable/disable, and the corresponding integer queries. Texture
generation reserves names without allocating pixel storage. Binding materializes
an object; deleting a bound object restores binding zero.

Uploads own their pixels rather than retaining guest pointers. `glTexImage2D`
stores separate explicit mip levels; `glTexSubImage2D` updates a bounded rectangle
only after its complete source was read successfully. Unsigned-byte RGB/RGBA,
BGR/BGRA, channel and luminance formats are converted into RGBA8. Alignment,
row length and row/pixel skips are respected. The model bounds names at 65,536,
an edge at 4,096 pixels, and owned pixels at 256 MiB per context. Unsupported
source formats/types and borders remain explicit frontiers. All standard GL 1.1
sized internal formats (including RGB5, RGBA4 and RGB5_A1) use the corresponding
base format with an invariant eight-bit component allocation, as permitted by
section 3.8.1. RGB discards source alpha on both image and subimage uploads; RGBA
retains it. No manual quantization to the requested sized format is performed.

Binary evidence: Game.dll's upload calls around `0x6f0cfd1e` and `0x6f0cfd74`
use TEXTURE_2D and RGBA/UNSIGNED_BYTE; `0x6f0cfcd3` sets UNPACK_ROW_LENGTH from
the source pitch. The upload loop supplies individual mip levels. Its client
texture coordinates use two floats at byte 28 of a 36-byte vertex. This is why
row pitch, mip storage and interleaved client arrays are included now.

The live `glDrawElements` provider now routes through
`staticgl_compat_draw.rs`, `staticgl_vertex.rs` and `staticgl_raster.rs`. These own
a per-HGLRC RGBA8 color buffer and floating-point depth buffer. Client indices
and interleaved position/normal/color/UV arrays are read and validated before
framebuffer writes. The path retains homogeneous clip W, clips triangles, shades
vertices using the enabled lights/materials, and rasterizes with perspective
texture coordinates, explicit mip levels, linear/nearest filtering, scissor,
culling, depth comparison/writes, alpha testing, blending, polygon offset and fog.
Texture environments distinguish RGB, RGBA, alpha, luminance, luminance-alpha and
intensity base formats. The old restricted direct-draw helper is not the live
provider route.

Fixed-function setters have real per-context state in `staticgl_fixed.rs`.
There is no persistent first-setter blocker: disabled lighting, for example,
does not invalidate a later draw just because light parameters were set earlier.
Unimplemented imported calls are explicit frontiers; the signature table no
longer admits writes as diagnostic-only success. Unsupported active two-sided
lighting, non-triangle primitives, non-float position/normal arrays, front/aux
buffers and unmodeled extensions remain explicit boundaries.

This is a CPU compatibility rasterizer with real GPU publication, not native
hardware fixed-function shading. It deliberately uses the already-proven sampled
vGPU pipeline for a fullscreen quad, waits for the timeline and then publishes
the UI4 frame. The first draw publishes a preview; subsequent draws accumulate
until swap or finish. Color clears also publish. Presentation copies bottom-up
GL rows into top-down texture storage and uses opaque window alpha, while the
owned GL buffer retains its alpha for blending and readback. `glReadPixels`
reads that owned buffer with pack row alignment/skips. See
[first-frame validation](staticgl-first-frame.md) for receipt interpretation and
remaining limits.

Host tests validate texture ownership, namespace isolation, unpack addressing,
format conversion, transactional updates and complete guest-array-to-pixel
rendering. GPU ABI test seams panic on use: host tests do not pretend to validate
hardware rendering. The independent triangle demo remains the prior hardware
proof; this compatibility path needs its own next packed hardware run.

API references: [texture uploads](https://learn.microsoft.com/en-us/windows/win32/opengl/glteximage2d),
[pixel storage](https://learn.microsoft.com/en-us/windows/win32/opengl/glpixelstorei),
[texture binding](https://learn.microsoft.com/en-us/windows/win32/opengl/glbindtexture),
and [GL 1.1 section 3.8.1 / tables 3.8, 3.10, 3.11](https://registry.khronos.org/OpenGL/specs/gl/glspec11.pdf).
