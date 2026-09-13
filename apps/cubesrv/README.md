# Three-tier CubeImage gallery

Key 8 connects Cubes to CubeSrv and downloads its embedded gallery. Six image
slabs surround the center at 10% of the standard 4×4×4-chunk world radius and
face inward. A white 3×3×3 c4 landmark sits at the origin. Every three seconds,
the server spawns six temporary c4 terrain cubes: the original four cardinal
positions 5–10 blocks from the center, plus one directly above and one below at
7–10 blocks. The extra vertical clearance keeps the lower full-size billboard
out of the landmark. After 500 ms, one independently selected 32×32 effect plays
once above each terrain cube; each pair then disappears. All six effects remain
billboarded, and duplicate rolls are allowed.
The player starts on the +Z part of the landmark's top face, looking along -Z.
Another numbered mode disconnects; Key 8 reconnects.

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

The fixed `slides/pixvfx/Frames/<category>/<effect>/` pack supplies all 150 VFX.
The importer generates `vfx.bin` and a named catalog in `gallery.json`; builds
embed the catalog automatically. Server helper `select_vfx(Some("Magic/Arcane Orb"))`
selects an exact category/name; `select_vfx(None)` chooses randomly using TRUEOS's
RNG. The timer calls it six times per three-second cycle. Repeats are allowed.
All effects share a palette equivalent to the client's existing RGB555 colours,
so switching effects requires no new gallery download. There is only one VFX
path; the old standalone VFX assets, importer and frame protocol are removed.
Playback uses 150 ms/frame, shortened for strips over 16 frames to fit the
2.4-second playback window. No authored frames are removed.

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
output directory, reads all numbered 32×32 RGBA frames from the fixed pack, and embeds the
resulting packages. Frame filenames need a trailing frame number and are sorted
numerically. Every frame must be 32×32; alpha-zero pixels are omitted, while
every nonzero-alpha pixel becomes one colored c1 cube. Python 3 and Pillow are
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

`--manifest`, `--gallery`, `--source-root`, `--vfx-root` and `--output` override their respective
paths. `--faces` overrides the saved selection; otherwise the importer preserves
`gallery.json`'s faces. The importer accepts PNG/JPG/JPEG/JGP. The hash fields in
`gallery.json` are generated bookkeeping; they never need hand editing. The build
validates the freshly generated packages before embedding them alongside the existing
49-asset catalog. Original pack PNGs are kept unchanged. `vfx.bin` is the generated
lifetime-compressed catalog used by the server.

## Rendering and transport

The client uses the reference 44-triangle beveled cube, removes touching flat
interior faces, and keeps the bevels, backs and outer edges. A continuous image
projection spans fronts and bevels; normals change lighting only. All six slabs
share one indexed retained PBR mesh and one nearest-filtered PNG atlas. There
are no per-image-pixel cube seeds or simulated normal/occlusion maps.

Every image constituent cube is **c1**, with a side of 0.2 renderer units. The presets
therefore occupy 6×6, 8×8 and 16×16 c1 (1.2×1.2, 1.6×1.6 and 3.2×3.2 renderer
units), all one c1 thick. Their centers are at ±102.5 c1 (±20.5 renderer units),
the nearest half-cell to 10% of the 1024-c1 world radius. The 2048-c1 world still
spans 409.6 renderer units and keeps its normal movement boundary. All image cube
minima lie on the integer c1 lattice and have no walking collision or Space-snap
target.

The center landmark contains 27 white c4 cubes, using the same 8-c1/1.6-renderer-
unit cube size as platforms and pathways. It spans 24 c1 (4.8 renderer units) on
each axis and has ordinary walking collision. The shared VFX palette remains in
the atlas's final row and uses the same RGB555 colors as placed assets.
Each VFX is a 32×32 c1 grid, bottom-center anchored at its terrain cube's top.
There is one slot per cube, ordered +X, +Z, -X, -Z, above, below.
Camera right/up orient both its pixel positions and cube rotations; terrain and
the gallery retain their world orientation. Only visible pixels become geometry.
The 27 landmark cubes, six terrain cubes and six effects use one immutable
44-patch cube mesh. GPU seed buffers still update for camera-facing positions;
compression eliminates network frame retransmission, not these GPU uploads.
The textured gallery mesh stays resident. V4 submits both meshes with shared
depth, one clear and one completion fence.

The largest atlas is 390×261, including duplicated edge texels and the white row:
about 397.5 KiB
when resident as RGBA. Six tier3 slabs have 56,064 triangles and about 6.3 MiB
of vertex/index data, excluding carrier copies and allocator overhead. These
are storage counts, not measured native frame-time or peak-memory claims.

UDP port 30018 retains the `CUB1` v1 envelope:

- `0x85`: u32 player ID, u32 content revision, three f32 spawn coordinates,
  u32 package length.
- `0x05`: u32 revision, u16 requested chunk index.
- `0x86`: u32 revision, u16 chunk index, up to 1024 package bytes.
- `0x89`: u32 gallery revision, u32 event, u16 event age in ms, followed
  by six descriptors: u32 asset revision, u32 byte length, three i16 c1 anchor
  coordinates, u8 frame count, u16 frame period. Body length is 112 bytes; the previous 78- and 418-byte layouts are rejected.
- `0x07`: u32 asset revision, u16 requested chunk index.
- `0x8a`: u32 asset revision, u16 chunk index, up to 1024 asset bytes.

The package has a 16-byte header followed by a standard RGB PNG: `CGA1`,
version byte **8**, face count 6, six tier bytes (1–3), cube side byte 1,
coordinate-unit byte 1 (c1), and u16 little-endian world half-size 1024.
Each face/tier is a compact regular grid descriptor for N² c1 cubes plus its
atlas tile; the client expands exact integer cube positions through the shared
`Layout::cube_min` contract. Rows run bottom to top and columns image-right to
image-left. The v8 placement rule fixes the gallery at 10% radius and reserves
the atlas's final row for the landmark and VFX palette. This sends cube placement without repeating per-cube coordinates
or transmitting a mesh. Side, unit and world extent are validated on receipt.
Older package versions and tier4 are rejected. The atlas dimensions are derived
from the tiers and checked before decoding. Encoded size is bounded to 4 MiB.
The revision comes from the package hash. Revision-mismatched chunks cannot mix
images. Bounded chunk windows, retries and duplicate rejection remain in use.

`vfx.bin` contains revision-addressed VFX1 sequences. Each has a 12-byte header
(magic, version 1, width 32, height 32, frame count, u16 frame period, palette
count, reserved zero), RGB palette, then five-byte
`(x, y, palette, first_frame, end_frame_exclusive)` records. Consecutive identical
RGB555 pixels share a single lifetime, including runs across N frames; transparent
frames end a run rather than incorrectly retaining a vanished pixel. The importer
precomputes this without modifying source PNGs. All 150 effects reconstruct
exactly at the existing display-color precision. The bundle is 813,510 bytes
(previous full-frame bundle: 954,664 bytes).

Clients cache complete validated sequences by revision (4 MiB bound, enough for
the whole pack), reuse them across all six slots and future rolls, and evaluate lifetimes
locally. After caching, only small server timing snapshots are needed. Missing
or reordered chunks cannot mix revisions; stale events cannot rewind playback.
A missing snapshot does not prevent local expiry of visuals or collision.
Snapshots arrive every 50 ms. `spawn.rs::anchors` supplies six separated locations:
first spawn at 3 s, first frame at 3.5 s, each effect expires after one
loop along with its base cube, all cleared by 5.9 s, next spawn at 6 s.
The four horizontal blocks have their tops at landmark-top height; there is no nearby
authored terrain in this sky world. They use world palette entry zero and walking
collision. VFX pixels appear immediately without placement's growth delay.

Cubes decodes the atlas in its networking worker through `vmedia::decode_retained`.
Only a complete resident texture replaces the displayed scene. Failed transfers
keep the current gallery visible. Matching layouts reuse the mesh; layout
changes build the replacement before releasing the old one. Telemetry and
periodic announcements continue; peers expire after 30 seconds disconnected.

**Rebuild TRUEOS, CubeSrv and Cubes together** for the v8 gallery and new VFX1/snapshot contract.
The hull fastpath integration also requires rebuilding TRUEOS with
`RetainedFrameSubmitV4` / `trueos_cabi_vgpu_retained_frame_submit_v4`. This is an
additive kernel/SDK interface; older retained submissions keep their contracts.
The new VFX protocol replaces the old frame requests; old clients and servers
must not be mixed.

## Validation

```sh
python3 -B tools/test_prepare_slides.py
python3 -B tools/test_cube_image.py
python3 -B tools/check_host.py
```

In Cubes: `cargo check --offline`, `python3 -B tools/test_slideshow_network.py`
and `python3 -B tools/test_walker_camera.py`. The host suites check all three
presets, crop/padding, VFX alpha sparsity/order, source orientation, geometry, winding, collision, packet
validation and native material descriptors. A live TRUEOS run is still needed
to verify appearance, frame timing and actual GPU residency.

## Key8 world1 terrain

CubeSrv embeds `Cubes/Cube/lvl27/world_01_sky.cubes` at build time alongside the
gallery and VFX packages. The normal CUB1 welcome (`0x81`) announces world ID 1
and its byte/chunk counts; world requests (`0x03`) receive world chunks (`0x83`).
The client downloads a complete world once per connection with bounded retries
and validates it through the normal `.cubes` level decoder before entering.
World1 is fixed for that server build; rebuilding the world requires reconnecting.

Key8 uses that terrain's normal walker collision and nearest-first visibility
selection, with the six images, center 3×3×3 c4 landmark and VFX rendered
in the same depth-tested frame. Spawn stays on top of the landmark. Portals and
local editing remain disabled in this server-owned scene. Terrain submission
reserves room for the landmark and all six 32×32 planes, six terrain cubes and 129 navigation slots within the
retained 32768-instance limit. This leaves 26462 world-terrain seeds; collision retains
the full terrain. Both the SDK cap and native retained-transform row cap are
32768; other Cubes modes retain their existing 8192-seed UI budget.
Rebuild both CubeSrv and Cubes for this addition.
