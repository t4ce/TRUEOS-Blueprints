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

The shader/state contract and remaining admission limits are documented in
TRUEOS `tools/wc3-fixed-bake/README.md`. Alpha testing, blending, two-sided
lighting, texgen, polygon offset, nondefault viewport/depth range, and sampling
beyond nearest/repeat remain explicit frontiers. The reported `0x300f04f` mask
is covered by a regression test; hardware correctness and FPS are unverified.
