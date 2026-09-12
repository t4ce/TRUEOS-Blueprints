# Three-tier CubeImage gallery

Key 8 connects Cubes to CubeSrv and downloads its embedded gallery. Six image
slabs sit at the centers of the six outer faces of the whole 4×4×4-chunk world,
facing inward. The player starts in flight at `[0, 0, 0]`, looking along -Z.
The gallery is static. Another numbered mode disconnects; Key 8 reconnects.

## Final asset presets

| Size | Source canvas | Cube build | Texture pixels across a face | Blocks |
|---|---|---|---|---|
| tier1 | 48×48 | 1×6×6 | 6 | 36 |
| tier2 | 128×128 | 1×8×8 | 16 | 64 |
| tier3 | 256×256 | 1×16×16 | 32 | 256 |

These are the only supported presets in the importer, client and HTML preview.
The last column of the requested texture configuration means pixels across the
whole slab face; the world still has six image slabs. Each preset aligns exactly
with the reference cube bevels. Tier1 has one texture cell per block; tiers 2
and 3 have two per side. There is no additional alignment crop.

## Source import and preview

`slides/sources.json` contains records with exactly these fields:

```json
{ "slide": 11, "source": "testboard/checker_bw_48x48.png", "Size": "tier1" }
```

The first six entries fill **-Z, +X, +Z, -X, -Y, +Y**. The current examples
show black/white tier1–3, followed by RGB tier1–3. Additional catalog entries
remain available for selection. Legacy tier4 entries have moved to tier3.
Paths resolve relative to the manifest directory.

Both the importer and `tools/CubeImage.html` create only 48×48, 128×128 or
256×256 source canvases. The selected preset determines the destination size:
large axes are center-cropped; small axes are centered on black. Pixels are
never rescaled. An image that is wide but short is cropped horizontally and
padded vertically. Odd spare pixels go on the right/bottom. Transparency is
composited over black; JPEG orientation is honored.

The HTML has one size selector that sets source size, cube count and texture
pixel count together. Its fixed opposite-side view matches the source preview.
Wheel zoom, image loading, image reset and PNG capture remain. The old free-form
geometry/grid/exposure controls, placement modes, orbit and experimental render
modes are removed. Quantization stays fixed at 16 channel levels and exposure
1.05. Native lighting uses PBR rather than the HTML's simple preview light.

After changing the catalog or its images, run:

```sh
python3 -B tools/prepare_slides.py
```

To select six catalog IDs explicitly in face order:

```sh
python3 -B tools/prepare_slides.py --faces 11 12 13 19 17 15
```

`--manifest`, `--source-root` and `--output` override their respective paths.
The importer accepts PNG/JPG/JPEG/JGP and needs Pillow only at preparation time.
It outputs `slides/gallery.cga` and the generated receipt `slides/gallery.json`.
The build checks manifest/package hashes and the package header, then embeds
that gallery and the existing 49-asset catalog. Original image files are kept.

## Rendering and transport

The client uses the reference 44-triangle beveled cube, removes touching flat
interior faces, and keeps the bevels, backs and outer edges. A continuous image
projection spans fronts and bevels; normals change lighting only. All six slabs
share one indexed retained PBR mesh and one nearest-filtered PNG atlas. There
are no per-image-pixel cube seeds or simulated normal/occlusion maps.

The 2048-c1 world is 409.6 renderer units across. Slab centers are at ±204.8 on
their corresponding axes. Each slab spans 144×144 units and is one block thick.
Collision uses six analytic volumes and block bounds, without a dense empty
world allocation. Small bevel recesses are solid for navigation.

The largest atlas is 102×68, including duplicated edge texels: about 27.1 KiB
when resident as RGBA. Six tier3 slabs have 56,064 triangles and about 6.3 MiB
of vertex/index data, excluding carrier copies and allocator overhead. These
are storage counts, not measured native frame-time or peak-memory claims.

UDP port 30018 retains the `CUB1` v1 envelope:

- `0x85`: u32 player ID, u32 content revision, three f32 spawn coordinates,
  u32 package length.
- `0x05`: u32 revision, u16 requested chunk index.
- `0x86`: u32 revision, u16 chunk index, up to 1024 package bytes.

The package has a 16-byte header followed by a standard RGB PNG: `CGA1`,
version byte **4**, face count 6, six tier bytes (1–3), four reserved zero bytes.
Older package versions and tier4 are rejected. The atlas dimensions are derived
from the tiers and checked before decoding. Encoded size is bounded to 4 MiB.
The revision comes from the package hash. Revision-mismatched chunks cannot mix
images. Bounded chunk windows, retries and duplicate rejection remain in use.

Cubes decodes the atlas in its networking worker through `vmedia::decode_retained`.
Only a complete resident texture replaces the displayed scene. Failed transfers
keep the current gallery visible. Matching layouts reuse the mesh; layout
changes build the replacement before releasing the old one. Telemetry and
periodic announcements continue; peers expire after 30 seconds disconnected.

**Rebuild CubeSrv and Cubes together** for the final three-tier contract.
TRUEOS must already support `RETAINED_MATERIAL_FLAG_NEAREST`; older kernels
reject that material option. No new shader or kernel change is needed here.

## Validation

```sh
python3 -B tools/test_prepare_slides.py
python3 -B tools/test_cube_image.py
python3 -B tools/check_host.py
```

In Cubes: `cargo check --offline`, `python3 -B tools/test_slideshow_network.py`
and `python3 -B tools/test_walker_camera.py`. The host suites check all three
presets, crop/padding, source orientation, geometry, winding, collision, packet
validation and native material descriptors. A live TRUEOS run is still needed
to verify appearance, frame timing and actual GPU residency.
