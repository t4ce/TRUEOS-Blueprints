Maxpix

230,400 native Intel POINT_LIST particles, seeded and uploaded at startup and
when changing scenes or circuit stages. The original baked native vertex shader
is unchanged.
Maxpix embeds the same PrismQ simulator library used by apps/prismq; the web
designer/server does not need to run.

With the frame focused:
  0: original tornado (also the startup scene)
  1: cycle native point size (existing control)
  2: quantum-weighted (2,3) trefoil knot
  3: quantum-weighted Hopf fibers, linked circles projected from S^3 into R^3
  4: Fourier bloom (8-qubit QFT, coherent spectral petals)
  5: Grover beacon (8-qubit search, marked state 0xA5)
  6: quantum walk (7 position qubits plus a coin)
  7: Bell weave (20 qubits, two correlated 10-bit registers)
  8 / 9: previous / next circuit stage in scenes 4..7
  Wheel up/down: zoom in/out by changing vertical FoV (20..100 degrees)

FoV starts at 60 degrees and changes by 3 degrees per wheel step. Zoom works
in every scene and persists across scene changes and window resizing. The
camera stays at [-10,-10,-10], aimed at the origin; only its projection changes.
Each wheel delta is applied once when received, including while frames are busy.

Circuit stages (8/9 clamp at the endpoints; they do not wrap):
  4: stages 0..15 change input rotations/phases before a genuine QFT. Four
     coherent residue combs interfere into 16 frequency peaks. Each peak gets
     an entire curled petal; its Born weight sets the particle population and
     its relative complex phase shapes the curl. Starts at stage 0.
  5: stages 0..12 select the number of Grover oracle/diffusion iterations.
     Starts at 6. Three central orbits represent the marked state 0xA5; outer
     rails represent all other states. Go back to 0, then advance to 12 to
     watch probability concentrate in the beacon (over 99% at iteration 12).
  6: stages 0..32 select steps of a symmetric coined Hadamard walk on a cycle
     of 128 sites. Starts at 16. Each occupied site/coin outcome gets a loop;
     probability sets its population. Alternating dark sites are genuine walk
     parity, not missing geometry. The site axis scales to the current walk's
     reach so each stage stays framed; this is not a fixed spatial ruler.
  7: stages 0..15 change local rotations and phases while preserving exact
     A=B correlations between two 10-bit registers. Starts at 0. The actual
     20-qubit state has 1,048,576 complex amplitudes (16 MiB working state),
     with only 1024 supported outcomes exported. Their matching low-bit
     addresses project onto 32 braided channels. This is a view of register
     correlations; it does not draw a physical connection between qubits.

Changing to a different scene selects its default stage. Repeating the same
scene key keeps its stage. Circuit stages are logged alongside preparation
time and qubit count. The 20-qubit scene can take longer to prepare; it has
the same 230,400-point GPU workload as every other scene once ready.
Only relative phases affect geometry; a common global phase has no effect.

Scenes 2/3 simulate an eight-qubit entangled interferometer on the CPU once per
selection. Its 256 Born probabilities determine point populations along the
knot or across the fibers, with different circuit phases for each scene.
These are mathematical artworks with quantum-weighted density, not physical
electron orbitals. Empty bins stay empty; total particle count stays fixed.
The new scenes gently ripple and rotate; mouse X controls speed and mouse Y
steers their rotation. Four pointers continue to contribute equally. Point
size is preserved across scene changes; 1px or 2px gives the clearest detail.

Geometry uses only the existing phase/height/radius + position-offset payload.
Offsets cancel the tornado at time zero; bounded shader time then adds small
native motion without wrapping height. The existing instance quaternion turns
the whole shape. No shader source, baked binary, ABI, or kernel changes, and
no shader compiler/bakery setup is required. Cyan is still shader-constant.
Retained GPU snapshots are released and recreated on scene/stage selection;
there are no per-frame vertex uploads, simulations, or accumulating scene
allocations.
PrismQ parallelism is disabled; simulations run on the calling thread only
when selecting a scene/stage, never during animation. Because it uses std,
Maxpix now builds with the existing TRUEOS std backend.

The startup tornado is a rising, widening cloud.
Its local bottom/top radii are 0.75/5.5, and its local vertical extent is
-4.4 to 4.4. The instance scale is 1.0.
The frame clears to fully transparent black (RGBA 0,0,0,0).
The fragment shader gives every point the same opaque cyan color. Four pointer
sources are assigned stable slots in first-seen order (extra sources are
ignored). The first mouse alone controls the entire tornado: horizontal
position selects 0x..2x motion speed (center = 1x), vertical position selects
-PI..PI angular offset (center = zero). Multiple active pointers contribute the
average of their integrated motion phases and angular offsets. Without input,
the original animation runs at 1x. Slots retain their assignment until app exit.
Held buttons, press/release edges and wheel deltas are wired for all four slots;
wheel deltas also control the app's camera FoV. Buttons have no assigned effect.
Edges accumulate until a
frame is published, preserving clicks between frames. This is a current-state
snapshot, not a historical event ring.

Layout 11 reserves two otherwise unused camera matrices for pointer transport:
inverse_view_projection columns = [normalized x, normalized y, motion seconds,
active]; previous_view_projection columns = [held, pressed, released, wheel].
The shader uses these as four vec4 records each, not as matrices. The shared
camera ABI is unchanged. Motion time is integrated by the app so speed changes
do not retime the whole animation.
Picasso's camera stays at [-10,-10,-10], looking at [0,0,0]. UI4 resize events
update the viewport and camera aspect. Maxpix initially renders 1x1 native points into
a full-resolution streaming frame, presented at 1:1 without upscaling.

Change PARTICLE_COUNT near the top of scene.rs to adjust the workload and rebuild.
Math geometry lives in quantum.rs and circuit preparation in circuits.rs;
tornado.wgsl remains unchanged.
Seeds occupy 24 bytes plus a 4-byte index per point. Only one retained instance
is allocated.

Build from TRUEOS-Blueprints:
  target/x86_64-unknown-linux-gnu/debug/trueos-blueprint apps/maxpix
Host checks:
  cargo test --offline --target x86_64-unknown-linux-gnu \
    --manifest-path apps/maxpix/Cargo.toml --lib

Requires the accompanying TRUEOS kernel with retained layout MAXPIX_TORNADO (11).
On the existing matching OS, update the Blueprint and use Apps mode: online maxpix
Startup and the first four frames emit Important maxpix markers with
particle count, time, extent, and completion timeline. Every four seconds an
Important ShellIO log reports average FPS from GPU-completed, published frames.
This includes frame pacing (the loop sleeps 16 ms) and surface backpressure.
Host tests check known Fourier spectra/phases, analytic Grover amplification,
walk symmetry/parity, 20-qubit Bell correlations, particle populations,
deterministic payloads, and animated geometry bounds. New scene rendering
and hotkeys need a hardware run.

Select the frame with the RDP/physical pointer, release the selection click,
then move within its content to steer. Dragging the frame keeps the pointer's
local position approximately unchanged; that is not an app steering gesture.
Press 1 with the frame focused to cycle native point widths in pixels:
1, 2, 4, 8, 16, 32, 64, 128, 255.875, 0.125, 0.25, 0.5, then 1 again.
The kernel validates the full nonzero 11-bit U8.3 domain. No vertex uploads or
shader rebakes occur when changing size. camera.jitter_frame[3] carries raw width.

The kernel must already support layout 11; this scene update only needs maxpix.bp.
Layout 11 prevents an older kernel silently ignoring pointer/raster input. The first
kernel submission logs maxpix-native: input-contract=pointer-v1 and the width.
The app logs each new source and every 120th pointer report, plus a cumulative
pointer-reports count in the FPS line. These distinguish input routing from a
shader/kernel mismatch.
Scene changes log the selected name and preparation time; FPS logs include
the current scene. Physical rendering and keyboard routing need a hardware run.
