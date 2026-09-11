# Cubes slideshow demo

The server embeds the prepared images in `slides/`, announces a new image
every ten seconds, and wraps after the last image. It retains the health routes and
asset catalog; the 27 static worlds are no longer embedded or served.

Build both cubesrv and Cubes with these changes. Key 8 connects to the local
server (UDP 30018), or reconnects if already connected. The server's manifest
sets the player's initial position to `[0, 0, 0]`; the client faces -Z. Subsequent
slides replace the image without resetting the camera. Existing mouse look,
flight and surface walking remain available. Asset placement and the companion
cube belong to the local-world modes, outside this dedicated image pass. Selecting another
numbered mode disconnects the slideshow. Disconnected peers expire after 30s.

The prepared image is 512×512. The server embeds and streams ordinary compressed
PNG/JPEG bytes. Cubes uses `trueos::vmedia::decode_retained` in its background
worker to decode directly into the device's Picasso texture residency. No RGB
readback or custom image format is involved. The old texture remains visible
until the next image is completely transferred, decoded and resident.

The image wall is now **one four-vertex, two-triangle PBR panel**, centered at
`[0, 0, -240]` with a 144×144 world-unit extent. It uses the same tangent-space
normal/material shader as Picasso-Example. The incoming image supplies color;
two fixed client-side maps supply bevel normals and edge occlusion aligned to
512×512 cells. These maps are reused across images and reconnects (32 MiB decoded
total). Lighting and highlights respond to the live camera; relief fades when
cells become subpixel because the current native sampler uses only mip level 0.

This is a flat surface with simulated cell relief, not extruded cube geometry:
it has no per-cell silhouette or parallax. It bypasses the dynamic-cube renderer,
its seed budgets and its LOD preparation entirely. The existing walker uses a
separate coarse collision grid built only on connection. Camera position is
preserved between slides. See Cubes `tools/SLIDESHOW.md` for client details.

## Image preparation

Run `python3 tools/prepare_slides.py` from this directory (Pillow required).
The defaults read the sibling TRUEOS repository; `--source-root` overrides it.
You can supply exactly ten PNG/JPG/JPEG paths relative to that root.

EXIF orientation is applied. Images at least 512 pixels on both axes are
downscaled and center-cropped. Smaller images are fitted without upscaling and
centered on their average color. Transparency is composited over that color.
`slides/sources.json` records the source paths. Only the prepared standard image
files are embedded; there are no raw RGB sidecars or custom image file formats.
The catalog accepts PNG, JPG and JPEG (any nonempty set, sorted by filename). Prepared
images must be 512×512 and at most 4 MiB each. Runtime decoding uses the TRUEOS
media API rather than adding a decoder library to Cubes or cubesrv.

## UDP additions

All packets retain the `CUB1`, version 1, kind and little-endian u16 payload-length
header. Telemetry still carries world 27 as the single session identifier.

- `0x85`: server image/connect event: u32 player ID, u32 revision, three f32 spawn coordinates, u32 encoded file length.
- `0x05`: client image chunk request: u32 revision, u16 chunk index.
- `0x86`: server image chunk: u32 revision, u16 chunk index, up to 1,024 original PNG/JPEG file bytes.

Revision modulo the catalog size selects the immutable embedded image. Transfers use 32-chunk
windows with bounded retries. The client rejects stale-revision, malformed and
duplicate chunks, and only publishes complete, successfully decoded images. Regular telemetry obtains
a fresh manifest if an announcement is lost. An incomplete image leaves the
previous scene visible. Old world requests receive error 5.

## Validation

- `python3 tools/test_prepare_slides.py`
- In Cubes: `python3 tools/test_slideshow_network.py`
- In Cubes: `python3 -B tools/test_slideshow_material.py`
- In Cubes: `python3 tools/test_walker_camera.py`
- In Cubes: `cargo check --offline`

The host tests cover preparation, packet compatibility, reordering, duplicates,
revision isolation, two-triangle geometry, material bindings, tangent alignment,
normal/occlusion maps and camera collision. Native decoding, texture residency
and rendered appearance still need TRUEOS rig validation.
