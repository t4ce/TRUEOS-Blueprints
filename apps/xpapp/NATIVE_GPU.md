# Native OpenGL GPU path

`glClear` uses drawable-owned GPU D32 storage. A depth clear writes 1.0 through
Render0; a depth-only clear preserves color. Clears use two GPU triangles
clipped to the drawable and the lower-left GL scissor rectangle, independently
of viewport. Empty scissors submit nothing. Pixels outside the rectangle retain
both color and depth. Color-only clears preserve depth.
`glDepthMask(false)` suppresses depth clears and writes, and `glDepthFunc` is
translated to Intel comparison state. Depth survives separate draw submissions.
The kernel charges each device for its own drawable depth allocation and retains
it until a resize or device teardown; it is independent of Picasso scene depth.

Deploy the matching TRUEOS kernel and `xpapp.bp` together. The added
`IndexedDraw.texture_reserved` flags preserve the existing wire struct size;
old kernels reject the new flags. CPU rasterization remains test-only.

Validation: `cargo test -p xpapp --lib --offline`,
`python3 tools/test_drawable_depth.py` and
`python3 tools/test_clip_position3_uv_texture.py` in TRUEOS, plus kernel and
Blueprint builds. These checks do not establish hardware image correctness or FPS.

Indexed draws now use the native fixed-function GPU package. The GPU applies
modelview/projection/normal/texture transforms, all eight lights, material and
primary colors, perspective interpolation, RGB/RGBA texture combination and
linear/exp/exp2 fog. The kernel programs draw scissoring and back-face culling.
Guest vertices are uploaded without CPU transformation or lighting.

Minification supports nearest, linear, and all four mipmapped filters;
magnification supports nearest and linear. The complete authored mip chain is
uploaded as an atlas. Level selection and filtering run in the fragment shader.
Missing or inconsistent mip levels are rejected explicitly.

The coordinator acquires the UI4 producer lease before the first `glClear` or
`glDrawElements`, retains it across subsequent calls, and publishes it at WGL
swap (or the explicit first-draw preview feature). Acquisition retries only UI4
Busy. A missing lease is not a busy condition. `glClear` is excluded from the
nolog state-only dispatch path so it cannot bypass frame coordination.
