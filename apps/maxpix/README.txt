Maxpix

230,400 native Intel POINT_LIST particles, seeded and uploaded once by the CPU.
A baked native vertex shader animates a rising, widening tornado.
Its local bottom/top radii are 0.75/5.5, and its local vertical extent is
-4.4 to 4.4. The instance scale is 1.0.
The frame clears to fully transparent black (RGBA 0,0,0,0).
The fragment shader gives every point the same opaque cyan color. Four pointer sources are assigned stable slots in first-seen order (extra sources
are ignored). The first mouse alone controls the entire tornado: horizontal
position selects 0x..2x motion speed (center = 1x), vertical position selects
-PI..PI angular offset (center = zero). Multiple active pointers contribute the
average of their integrated motion phases and angular offsets. Without input,
the original animation runs at 1x. Slots retain their assignment until app exit.
Held buttons, press/release edges and wheel deltas are wired for all four slots;
this demo does not assign effects to those controls. Edges accumulate until a
frame is published, preserving clicks between frames. This is a current-state
snapshot, not a historical event ring.

Layout 11 reserves two otherwise unused camera matrices for pointer transport:
inverse_view_projection columns = [normalized x, normalized y, motion seconds,
active]; previous_view_projection columns = [held, pressed, released, wheel].
The shader uses these as four vec4 records each, not as matrices. The shared
camera ABI and once-only particle upload are unchanged. Motion time is integrated
by the app so speed changes do not retime the whole animation.
Picasso's camera stays at [-10,-10,-10], looking at [0,0,0]. UI4 resize events
update the viewport and camera aspect. Maxpix initially renders 1x1 native points into
a full-resolution streaming frame, presented at 1:1 without upscaling.

Change PARTICLE_COUNT near the top of scene.rs to adjust the workload and rebuild.
Color/motion live in tornado.wgsl alongside this app; changing them
requires rebaking the shader and rebuilding the kernel. Seeds occupy 24 bytes
plus a 4-byte index per point. Only one retained instance is allocated.

Build from TRUEOS-Blueprints:
  target/x86_64-unknown-linux-gnu/debug/trueos-blueprint apps/maxpix
Host checks:
  cargo test --offline --target x86_64-unknown-linux-gnu \
    --manifest-path apps/maxpix/Cargo.toml --lib

Requires the accompanying TRUEOS kernel with retained layout MAXPIX_TORNADO (11).
After loading the updated OS, use Apps mode: online maxpix
Startup and the first four frames emit Important maxpix markers with
particle count, time, extent, and completion timeline. Every four seconds an
Important ShellIO log reports average FPS from GPU-completed, published frames.
This includes frame pacing (the loop sleeps 16 ms) and surface backpressure. Capture/host tests pass;
physical output needs a run on the updated kernel.

Select the frame with the RDP/physical pointer, release the selection click,
then move within its content to steer. Dragging the frame keeps the pointer's
local position approximately unchanged; that is not an app steering gesture.
Press 1 with the frame focused to cycle native point widths in pixels:
1, 2, 4, 8, 16, 32, 64, 128, 255.875, 0.125, 0.25, 0.5, then 1 again.
The kernel validates the full nonzero 11-bit U8.3 domain. No vertex uploads or
shader rebakes occur when changing size. camera.jitter_frame[3] carries raw width.

Rebuild and boot the matching kernel as well as updating maxpix.bp. Layout 11
prevents an older kernel silently ignoring pointer/raster input. The first
kernel submission logs maxpix-native: input-contract=pointer-v1 and the width.
The app logs each new source and every 120th pointer report, plus a cumulative
pointer-reports count in the FPS line. These distinguish input routing from a
shader/kernel mismatch.
