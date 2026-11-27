# Hair

Standalone Blueprint for studying airflow around an avatar. The full
AvatarSample_B stands in its original rest pose. Its cape-like back hair is
a separately drawable mesh with its original texture and cutoff alpha. The
white, half-opacity sphere shows the head obstacle. The translucent blue
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

The preview opens facing the avatar. Move the mouse left or right to spin her
in the plane of the screen. Move down to tilt her toward the camera or up to
tilt her back to horizontal. Use the window close button to exit.
Use `--yaw 150 --hide hair --hide body-obstacles` for a clear view of the
guide roots behind the avatar.

A headless run can write a PNG.

```sh
target/x86_64-unknown-linux-gnu/release/hair --frames 180 --snapshot /tmp/hair.png
```

Use `--roll 180` to start upside down or `--tilt -90` to start flat on her back.
The airflow stays world-down while the roots and obstacle proxies follow the avatar.
In guide mode, Ubuntu starts with the hair guides visible and the head sphere,
reach outline, and general field streamlines hidden. To include these overlays
in snapshots, use `--show sphere --show reach --show field`.

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
alpha, with UVs remapped to the crop. The other atlas regions provide the
complete static avatar. To regenerate the embedded GLB from the original VRM,
run `python3 tools/prepare_avatar.py /path/to/AvatarSample_B.vrm` from this
directory with Pillow installed.

[fork]: https://github.com/t4ce/FluidX3D/blob/3d46aef9d4e67c1e84f41f7ae10e9573af4bf81b/src/kernel.cpp
[avatar]: https://hub.vroid.com/en/characters/7939147878897061040/models/2292219474373673889
[terms]: https://vroid.pixiv.help/hc/en-us/articles/4402394424089-VRoidPreset-A-Z
