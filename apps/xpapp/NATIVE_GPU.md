# Native OpenGL GPU path

`glClear` uses drawable-owned GPU D32 storage. A depth clear writes 1.0 through
Render0; a depth-only clear preserves color. Color-only clears preserve depth.
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

The immediate shader bridge still rejects unsupported perspective, varying
vertex color/texture combine, blending, alpha testing, lighting, fog and scissor
states. This depth change addresses the reported `glClear` frontier, not the
remaining fixed-function shader work.
