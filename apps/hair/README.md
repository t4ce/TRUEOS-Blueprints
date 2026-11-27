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

| Key | Effect |
| --- | --- |
| `1` | Toggle static avatar and short hair |
| `2` | Toggle detached back hair |
| `3` | Toggle the display of the head sphere |
| `4` | Toggle streamlines |
| `5` | Switch between velocity change and speed coloring |
| `6` | Toggle the back-hair reach outline |
| `7` | Toggle the torso and arm obstacle overlay |
| Left / Right | Orbit around the avatar |
| Space | Pause airflow |
| Esc | Close |

A headless run can write a PNG.

```sh
target/x86_64-unknown-linux-gnu/release/hair --frames 180 --snapshot /tmp/hair.png
```

The UI4 frame entry shares the same renderer. Check the native target locally
with `CARGO_NET_OFFLINE=true cargo bp hair --check`.

## Solver and assets

The CPU solver adapts the D3Q19 single-relaxation-time model and streamline
sampling from the [t4ce FluidX3D fork][fork] by Dr. Moritz Lehmann. It uses two
buffers for the lattice populations. The incoming flow follows the back-hair
rest axis, straight down in the standing pose. Its speed changes gently over
time and the field bends around the head, torso and arms. The solver uses
these volumes even when their overlays are hidden. Streamlines sample local
velocities, with the fork's rainbow mapping in [the local raster shader](shaders/raster.rs).
The solver box surrounds the full hair-reach sphere. The 120 streamline seeds
are distributed across a cross-section above the roots, perpendicular to the
rest axis. Each trace ends at the sphere boundary, even with the reach outline
hidden.
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
