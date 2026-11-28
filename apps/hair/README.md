# Hair

Standalone Blueprint for studying procedural back hair and airflow around
AvatarSample_B. The bending-rod preview sweeps two thin crossed strips along
each simulated guide. The cape-like source mesh supplies the resting length and
appears with its original texture in the airflow study. The white,
half-opacity sphere shows the head obstacle. The translucent blue
ellipsoids show five torso and neck obstacles and three per arm, including
the hands. The proxies are fitted to the static T-pose and the outer clothing.
The outer wire sphere marks a 0.75 m evaluation radius from the head center.
It encloses the longest resting back hair at hip level. The camera fits this
sphere and the complete avatar as it orbits.

## Ubuntu

Build from the Blueprint repository using the local Cargo cache.

```sh
cargo build --offline --release --manifest-path apps/hair/Cargo.toml
target/x86_64-unknown-linux-gnu/release/hair
```

The viewer loads the system SDL2 runtime (`libSDL2-2.0.so.0`). The executable
embeds the mesh and texture atlases. The solver and raster shaders are local
and the demo makes no network requests.

The rod preview opens on a close-up of the hair, with room for the scalp
and tips. Move the mouse left or right to spin the avatar
in the plane of the screen. Move down to tilt her toward the camera or up to
tilt her back to horizontal. Hold `A` or `D` to orbit the view around the
avatar's own vertical axis while keeping her simulated pose. Use the window
close button to exit.
The bending rod is the default. Guides get 9 segments at the 0.68 m reference
length, with a minimum of 5 at half that length and a maximum of 13 at 1.5×.
The last two segments share the rest length of the original last link.
Rod motion runs at 90% of the original speed.

- Tip weight is fixed at 50%. The lower half retains more momentum when the hair
  brushes against the body.
- Headband fixation eases from 50% upright to 0% upside down, based on the
  headband's direction relative to gravity.
- Density spread is fixed at 100%. Nearby guides push away from crowded regions.
  The solver estimates density at four points along each guide once per frame.
  The spread weight tops out at 90% along each guide, stays at full weight
  through the tip, and falls off logarithmically toward the root.
- `1` cycles baseline wind strength from 0% to 100% in 10-point steps.
  A gust travels across the guides in world space about every 5.6 seconds,
  peaking at 70% above the baseline before settling back. At 0%, the air is
  calm. `2` cycles eight horizontal world directions, 45° apart. Hold
  `Shift` to step backward. Wind starts at 0% with its heading toward +X/+Z.
- The rod preview uses Medium tip settling and 225 guides. The extra guides
  fill gaps on the authored anchor surface.

The length falls off sharply at the outer sides, where guides are up to 20%
shorter for the same root-height layer. The upper root layers get a shorter
cut to stagger the tips.

The two crossed strips start slightly inside the scalp and taper into
depth-tested strokes on the final two guide links. The longer link takes
two-thirds of the original final length and uses two-pixel strokes. The
shorter link uses one-pixel strokes. Each link uses two line segments to
approximate a slight curve through the simulated joints. They share the
strip fill palette.
The fill uses five colors sampled from the back-hair atlas, projected by
avatar-local height across the whole haircut. A thin dark edge is built into
the fill texture and draws in the same pass.
The guide solver still runs with the hair layer hidden.

Guide lines are disabled by default in rod mode. Set `DRAW_ROD_GUIDES` in
`src/main.rs` to enable them in code, or use `--hide hair --show guides` to
inspect the lines alone.

`--motion 1`, `--motion 2`, and `--motion 3` select the loose rope, elastic
springs, and bending rod at launch.
`--motion flow` opens the original field-tracing study.
Use `--yaw 180 --hide body-obstacles` for a clear view of the strips behind the
avatar.

A headless run can write a PNG.

```sh
target/x86_64-unknown-linux-gnu/release/hair --frames 180 --snapshot /tmp/hair.png
```

Use `--roll 180` to start upside down or `--tilt -90` to start flat on her back.
Gravity and the original airflow study use world-down. The rod preview's wind
is horizontal in world space. Roots and obstacle proxies follow the avatar.
Use `--show sphere --show reach --show field` to include those overlays.

The UI4 frame entry shares the same renderer. Check the native target locally
with `CARGO_NET_OFFLINE=true cargo bp hair --check`.

## Solver and assets

The CPU solver adapts the D3Q19 single-relaxation-time model and streamline
sampling from the [t4ce FluidX3D fork][fork] by Dr. Moritz Lehmann. It uses two
buffers for the lattice populations. The lattice and incoming flow stay in
world space as the avatar spins. The head, torso, and arm obstacles move through
the lattice, and the incoming speed changes gently over time. The solver uses
these volumes even when their overlays are hidden. Streamlines sample local
velocities, with the fork's rainbow mapping in [the local raster shader](shaders/raster.rs).
The solver box surrounds the full hair-reach sphere. There are 44 optional
field seeds across an upstream cross-section. Each of the 150 vertices on the
authored `Back Hair AnkorSurf` mesh starts one guide. Blender coordinates
`(x, y, z)` map to Hair `(x, z, -y)`. The guides start straight down under the
headband and follow the world-space flow after clearing the head proxy.
Fluid traces stop at the reach boundary or a solid obstacle. Ubuntu guides stay visible
at lower flow speeds and use higher minimum opacity for low-contrast segments.
The tip-spread test adds a gentle sideways force to fluid cells behind the
avatar, starting halfway down the resting back hair and building toward the
tips. It follows the avatar during a roll. Use `--no-tip-spread` for baseline
snapshots with the force disabled.
The default color uses velocity differences between consecutive samples.
Speed mode uses velocity magnitude. Headless snapshots report the seed and
traced-segment counts. See [FluidX3D-LICENSE.md](FluidX3D-LICENSE.md).

The embedded [AvatarSample_B][avatar] is distributed under the [VRoid sample
terms][terms]. The back-hair texture crop preserves the original texels and
alpha, with UVs remapped to the crop. The short-hair accessories share the
original alpha-cutout texture crop. The outlined stars, including the dangling
star on the hairband, use a metallic finish. The colored crossed pins use rough
plastic. To regenerate the embedded GLB
from the original VRM,
run `python3 tools/prepare_avatar.py /path/to/AvatarSample_B.vrm` from this
directory with Pillow installed.

[fork]: https://github.com/t4ce/FluidX3D/blob/3d46aef9d4e67c1e84f41f7ae10e9573af4bf81b/src/kernel.cpp
[avatar]: https://hub.vroid.com/en/characters/7939147878897061040/models/2292219474373673889
[terms]: https://vroid.pixiv.help/hc/en-us/articles/4402394424089-VRoidPreset-A-Z
