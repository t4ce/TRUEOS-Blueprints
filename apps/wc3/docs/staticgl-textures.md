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

`staticgl_textured_draw.rs` connects a restricted draw subset to the new
`staticgl_triangle::textured::TexturedRenderer`, using the real sampled vGPU
pipeline and waiting for its timeline. Texture-enabled draws cannot accidentally
fall through to the untextured triangle renderer. The accepted subset is opaque
RGBA8, nearest/repeat sampling, indexed triangle lists, in-bounds affine clip
positions, a full-surface viewport, and REPLACE, opaque DECAL or white-vertex
MODULATE. Both RGB and RGBA base images are admitted. RGB REPLACE and both
DECAL cases require opaque primary alpha because those GL operations preserve
it; RGBA REPLACE takes alpha from the texture instead.
The LOAD_COLOR submission preserves previous colour; this path does not provide
a shared GL depth buffer.

Linear/mip filtering, alpha/blend/depth behaviour, other texture environments,
perspective interpolation and clipping still need implementation. Accepted
parameter writes retain these requests; the draw rejects unsupported states
instead of quietly sampling level zero. Any fallback `NoteWrite` permanently
marks that context's textured drawing as unmodeled, even after the bounded note
journal rolls over. Thus the existing WC3 state-note path will still require
proper raster-state translation before it can use this restricted renderer.

Host tests validate texture ownership, namespace isolation, unpack addressing,
format conversion, transactional updates and draw admission. GPU ABI test seams
panic on use: host tests do not pretend to validate hardware rendering. The
existing independent triangle demo remains the prior hardware proof; this new
sampled bridge requires a separate hardware validation.

API references: [texture uploads](https://learn.microsoft.com/en-us/windows/win32/opengl/glteximage2d),
[pixel storage](https://learn.microsoft.com/en-us/windows/win32/opengl/glpixelstorei),
[texture binding](https://learn.microsoft.com/en-us/windows/win32/opengl/glbindtexture),
and [GL 1.1 section 3.8.1 / tables 3.8, 3.10, 3.11](https://registry.khronos.org/OpenGL/specs/gl/glspec11.pdf).
