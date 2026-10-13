Maxpix

230,400 native Intel POINT_LIST particles, seeded and uploaded once by the CPU.
A baked native vertex shader animates a rising, widening tornado.
Its local bottom/top radii are 0.75/5.5, and its local vertical extent is
-4.4 to 4.4. The instance scale is 1.0.
The frame clears to fully transparent black (RGBA 0,0,0,0).
The fragment shader gives every point the same opaque cyan color. There is no mouse/keyboard control.
Picasso's camera stays at [-10,-10,-10], looking at [0,0,0]. UI4 resize events
update the viewport and camera aspect. Maxpix renders 1x1 native points into
a half-resolution scene backing. UI4 uses the display engine plane scaler for
2x presentation (roughly 2x2 output pixels per point), with a transparent
full-resolution foreground. No BCS0 upscale or HS/DS/GS expansion is used.

Change PARTICLE_COUNT near the top of scene.rs to adjust the workload and rebuild.
Color/motion live in TRUEOS/tools/maxpix-bake/shaders/tornado.wgsl; changing them
requires rebaking the shader and rebuilding the kernel. Seeds occupy 24 bytes
plus a 4-byte index per point. Only one retained instance is allocated.

Build from TRUEOS-Blueprints:
  target/x86_64-unknown-linux-gnu/debug/trueos-blueprint apps/maxpix
Host checks:
  cargo test --offline --target x86_64-unknown-linux-gnu \
    --manifest-path apps/maxpix/Cargo.toml --lib

Requires the accompanying TRUEOS kernel with retained layout MAXPIX_TORNADO (10).
After loading the updated OS, use Apps mode: online maxpix
Startup and the first four frames emit Important maxpix markers with
particle count, time, extent, and completion timeline. Every four seconds an
Important ShellIO log reports average FPS from GPU-completed, published frames.
This includes frame pacing (the loop sleeps 16 ms) and surface backpressure. Capture/host tests pass;
physical output needs a run on the updated kernel.
