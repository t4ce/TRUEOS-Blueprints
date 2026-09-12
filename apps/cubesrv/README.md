# Six-face CubeImage gallery

Key 8 opens the empty cube world with one centered image slab on each of its
six faces. The slabs face inward. The initial camera is at `[0, 0, 0]`, looking
along -Z. Mouse look, flight, surface walking, Space and Home remain available.
Another numbered mode disconnects; Key 8 reconnects. Placement and the companion
cube stay in the local-world renderer.

## Source contract

`slides/sources.json` is a catalog of records with exactly these fields:

```json
{ "slide": 11, "source": "testboard/checker_bw_64x64.png", "Size": "tier1" }
```

| Size | Normalized source | Cube assembly | Texture pixels across a slab face | Blocks |
|---|---|---|---|---|
| tier1 | 64×64 | 1×8×8 | 8 | 64 |
| tier2 | 128×128 | 1×10×10 | 21 | 100 |
| tier3 | 256×256 | 1×14×14 | 31 | 196 |
| tier4 | 512×512 | 1×28×28 | 31 | 784 |

The projected pixel count applies across the **whole assembly**, matching
`CubeImage.html`, not independently to every block. The nominal settings use
its bevel-aligned sampling: tier1 shows 8 cells without cropping; tier2 shows
20 cells with a 20/21 centered source crop; tier3 shows 28 cells with a 28/31
centered source crop; tier4 also shows 28 cells with a 28/31 centered source
crop. This removes about 2.38% per edge for tier2 and 4.84% for tier3 and tier4.

The first six manifest entries fill **-Z, +X, +Z, -X, -Y, +Y**, in that order.
Additional entries remain available for selection. Paths resolve relative to
the manifest directory. Prepare after editing sources or replacing images:

```sh
python3 -B tools/prepare_slides.py
```

To select six other catalog IDs in that same face order:

```sh
python3 -B tools/prepare_slides.py --faces 11 12 13 14 15 16
```

`--manifest`, `--source-root` and `--output` override their respective paths.
The importer accepts PNG/JPG/JPEG/JGP, applies EXIF orientation, composites
transparency over black, center-crops to square, and resizes with nearest
sampling. It then bakes the HTML's grid-center sampling, exposure 1.05 and
16 channel levels. Sources are preserved. Pillow is required only for the
importer, not for a server build or runtime.

The outputs are `slides/gallery.cga` and the bake receipt `slides/gallery.json`.
The latter is generated metadata; edit `sources.json` to configure the gallery.
The build checks manifest/package hashes and validates the package header.
It embeds only the encoded gallery and the existing 49-asset catalog.

## Geometry and rendering

The client uses the same 44-triangle beveled cube as the HTML (the embedded GLB
is identical to `Cubes/Cube/cube.glb`). Cube count follows the tier, independently
of source pixels. Only touching interior flat faces are omitted; bevels, backs
and outer edges remain real geometry. Box-projected UVs preserve the HTML's
axis selection and bevel tie rules. The six slabs share one atlas and one
indexed retained PBR mesh, submitted as one draw. Geometry is retained across
image replacements when tiers match. There are no image-pixel cube seeds,
normal/occlusion maps, or per-frame image geometry generation.

The world is 2048 c1 units across (409.6 renderer units); slab centers lie at
±204.8 on their corresponding axes. Every slab spans 144×144 renderer units,
with thickness `144 / blocks_per_side`. Collision uses six analytic slab
volumes plus block bounds for the walker, without allocating a dense empty
world grid. Decorative bevel recesses are treated as solid for navigation.

The atlas uses a 3×2 layout with a duplicated one-texel border per image.
At tier4 maximum it is 90×60 RGBA when resident: about 21.1 KiB. There is
one base-color texture and no 32 MiB pair of simulated-relief maps. Mesh
vertex/index data is about 19.3 MiB at six tier4 slabs (170,688 triangles),
excluding GPU/carrier copies and allocator overhead. Upload borrows the CPU
geometry directly instead of making a second serialized copy. These are
storage counts, not measured frame-rate or peak-memory claims.

Nearest min/mag sampling uses `RETAINED_MATERIAL_FLAG_NEAREST`, a new retained
PBR material option. Other PBR materials keep their existing linear filtering.
**Rebuild TRUEOS as well as CubeSrv and Cubes**; older kernels reject the new
material flag. No shader rebake is needed. Lighting uses the native PBR material
and live camera; the HTML's simple preview lighting is not reproduced exactly.

## Transfer contract

UDP remains on port 30018 with the existing `CUB1` v1 envelope:

- `0x85`: u32 player ID, u32 content revision, three f32 spawn coordinates,
  u32 package length.
- `0x05`: u32 revision, u16 requested chunk index.
- `0x86`: u32 revision, u16 chunk index, up to 1024 package bytes.

The package is a 16-byte header followed by one ordinary RGB PNG: `CGA1`,
version byte 3, face count byte 6, six tier bytes (1–4), four reserved zero bytes.
The atlas size is derived from the aligned grids. Maximum encoded size is 4 MiB.
Version 3 identifies these revised tiers and crop rules; rebuild CubeSrv and
Cubes together. Earlier package versions are rejected instead of misinterpreted.
The client validates the header and PNG dimensions before native decoding.

The gallery is static, not a timed slideshow. Its revision comes from the
package hash. A request for another revision is rejected. Telemetry and
periodic announcements continue; disconnected peers expire after 30 seconds.
Transfers retain bounded 32-chunk windows, retries and duplicate/stale rejection.
Cubes decodes through `vmedia::decode_retained` in its worker. Only a complete,
resident atlas replaces the scene between completed frames. Failed transfers
or replacements leave the old scene visible. There is no CPU image readback.

## Validation

```sh
python3 -B tools/test_prepare_slides.py
python3 -B tools/check_host.py
```

In Cubes: `cargo check --offline`, `python3 -B tools/prepare_image_cube.py --check`,
`python3 -B tools/test_slideshow_network.py` and `python3 -B tools/test_walker_camera.py`.
In TRUEOS: `python3 -B tools/test_picasso_pbr_state.py` and
`python3 -B tools/test_retained_material.py`.

`check_host.py` checks the actual server and build script with upstream host
dependencies, avoiding the workspace's TRUEOS-specific vendor patches.
Native appearance, upload residency, timing and peak memory require a live
TRUEOS run. The host network/geometry suite passes all 13 tests, and the
walker suite passes all 92 tests, including six-face collision, pose-preserving
gallery replacement and the authored portal fixtures.
