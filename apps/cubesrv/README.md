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
| tier3 | 256×256 | 1×16×16 | 128 | 256 |

These are the only supported presets in the importer, client and HTML preview.
The last column of the requested texture configuration means pixels across the
whole slab face; the world still has six image slabs. Each preset aligns exactly
with the reference cube bevels. Tier1 has one texture cell per block side; tier2 has two and tier3 has eight. There is no additional alignment crop.

## Source import and preview

`slides/sources.json` contains records with exactly these fields:

```json
{ "slide": 11, "source": "testboard/checker_bw_48x48.png", "Size": "tier1" }
```

Slide IDs are unique non-negative integers; `0` is valid. Edit the `faces`
array in `slides/gallery.json` to select six IDs in **-Z, +X, +Z, -X, -Y, +Y**
order. The current selection is 10–15. A new catalog without gallery.json uses
the first six sources. Additional catalog entries remain available for selection.
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

`cargo bp cubesrv` automatically prepares the selected images in Cargo's build
output directory and embeds the resulting package. Python 3 and Pillow are
required on the build host. Source/config edits and image changes trigger a
rebake; the build does not rewrite files under `slides/`.

To also refresh the checked-in package and generated hashes, run:

```sh
python3 -B tools/prepare_slides.py
```

To select six catalog IDs explicitly in face order:

```sh
python3 -B tools/prepare_slides.py --faces 10 11 12 13 14 15
```

`--manifest`, `--gallery`, `--source-root` and `--output` override their respective
paths. `--faces` overrides the saved selection; otherwise the importer preserves
`gallery.json`'s faces. The importer accepts PNG/JPG/JPEG/JGP. The hash fields in
`gallery.json` are generated bookkeeping; they never need hand editing. The build
validates the freshly generated package before embedding it alongside the existing
49-asset catalog. Original image files are kept.

## Rendering and transport

The client uses the reference 44-triangle beveled cube, removes touching flat
interior faces, and keeps the bevels, backs and outer edges. A continuous image
projection spans fronts and bevels; normals change lighting only. All six slabs
share one indexed retained PBR mesh and one nearest-filtered PNG atlas. There
are no per-image-pixel cube seeds or simulated normal/occlusion maps.

Every constituent cube is **c1**, with a side of 0.2 renderer units. The presets
therefore occupy 6×6, 8×8 and 16×16 c1 (1.2×1.2, 1.6×1.6 and 3.2×3.2 renderer
units), all one c1 thick. They stay centered on the six outer world faces.
The 2048-c1 world is 409.6 renderer units across; each slab's depth cell sits
just inside the boundary, with its center at ±1023.5 c1 (±204.7 renderer units).
All cube minima lie on the integer c1 lattice. Like other c1 detail in Cubes,
these cubes have no walking collision or Space-snap target. Flight still starts
at the origin, so the images are small in the distance until approached.

The largest atlas is 390×260, including duplicated edge texels: about 396.1 KiB
when resident as RGBA. Six tier3 slabs have 56,064 triangles and about 6.3 MiB
of vertex/index data, excluding carrier copies and allocator overhead. These
are storage counts, not measured native frame-time or peak-memory claims.

UDP port 30018 retains the `CUB1` v1 envelope:

- `0x85`: u32 player ID, u32 content revision, three f32 spawn coordinates,
  u32 package length.
- `0x05`: u32 revision, u16 requested chunk index.
- `0x86`: u32 revision, u16 chunk index, up to 1024 package bytes.

The package has a 16-byte header followed by a standard RGB PNG: `CGA1`,
version byte **6**, face count 6, six tier bytes (1–3), cube side byte 1,
coordinate-unit byte 1 (c1), and u16 little-endian world half-size 1024.
Each face/tier is a compact regular grid descriptor for N² c1 cubes plus its
atlas tile; the client expands exact integer cube positions through the shared
`Layout::cube_min` contract. Rows run bottom to top and columns image-right to
image-left. This sends cube placement without repeating per-cube coordinates
or transmitting a mesh. Side, unit and world extent are validated on receipt.
Older package versions and tier4 are rejected. The atlas dimensions are derived
from the tiers and checked before decoding. Encoded size is bounded to 4 MiB.
The revision comes from the package hash. Revision-mismatched chunks cannot mix
images. Bounded chunk windows, retries and duplicate rejection remain in use.

Cubes decodes the atlas in its networking worker through `vmedia::decode_retained`.
Only a complete resident texture replaces the displayed scene. Failed transfers
keep the current gallery visible. Matching layouts reuse the mesh; layout
changes build the replacement before releasing the old one. Telemetry and
periodic announcements continue; peers expire after 30 seconds disconnected.

**Rebuild CubeSrv and Cubes together** for the v6 c1 placement contract.
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
