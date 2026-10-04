Maxpix

10,000 native Intel POINT_LIST particles, seeded and uploaded once by the CPU.
A baked native vertex shader animates a rising, widening tornado. The fragment
shader gives every point the same cyan color. There is no mouse/keyboard control.
Picasso's camera stays at [-10,-10,-10], looking at [0,0,0]. UI4 resize events
update the viewport and camera aspect. The native rasterizer stamps four-pixel
squares, using the same point primitive as Potato Stamps; no HS/DS/GS expansion.

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
Startup and the first four/every 120 frames emit Important maxpix markers with
particle count, time, extent, and completion timeline. Capture/host tests pass;
physical output needs a run on the updated kernel.
