# Cubes slideshow demo

The server embeds ten prepared images from `TRUEOS/tools`, announces a new image
every ten seconds, and wraps after image ten. It retains the health routes and
asset catalog; the 27 static worlds are no longer embedded or served.

Build both cubesrv and Cubes with these changes. Key 8 connects to the local
server (UDP 30018), or reconnects if already connected. The server's manifest
sets the player's initial position to `[0, 0, 0]`; the client faces -Z. Subsequent
slides replace the image without resetting the camera. Existing mouse look,
flight, surface walking and placement remain available. Selecting another
numbered mode disconnects the slideshow. Disconnected peers expire after 30s.

The prepared image is 512×512. The server embeds and streams ordinary compressed
PNG/JPEG bytes. Cubes decodes each completed transfer at runtime with
`trueos::vmedia::decode` in its background worker; the UI does not run a decoder. **The current display is a 60×60
mosaic (3,600 cubes), not one cube per source pixel.** Each sample averages its
source region and uses c12, the largest authored tier (2.4 renderer units per
side before the existing gap). The wall is centered at `[0, 0, -240]`, framed
by the usual walker FOV at the default landscape viewport. The normal render
sliders remain effective. At the default budget the complete mosaic uses full
cubes rather than distant markers. 262,144 simultaneous cubes would require a
separate rendering expansion beyond the current 8,192-seed/3,840-detail limits.

## Image preparation

Run `python3 tools/prepare_slides.py` from this directory (Pillow required).
The defaults read the sibling TRUEOS repository; `--source-root` overrides it.
You can supply exactly ten PNG/JPG/JPEG paths relative to that root.

EXIF orientation is applied. Images at least 512 pixels on both axes are
downscaled and center-cropped. Smaller images are fitted without upscaling and
centered on their average color. Transparency is composited over that color.
`slides/sources.json` records the source paths. Only the prepared standard image
files are embedded; there are no raw RGB sidecars or custom image file formats.
The catalog accepts PNG, JPG and JPEG (ten files, sorted by filename). Prepared
images must be 512×512 and at most 4 MiB each. Runtime decoding uses the TRUEOS
media API rather than adding a decoder library to Cubes or cubesrv.

## UDP additions

All packets retain the `CUB1`, version 1, kind and little-endian u16 payload-length
header. Telemetry still carries world 27 as the single session identifier.

- `0x85`: server image/connect event: u32 player ID, u32 revision, three f32 spawn coordinates, u32 encoded file length.
- `0x05`: client image chunk request: u32 revision, u16 chunk index.
- `0x86`: server image chunk: u32 revision, u16 chunk index, up to 1,024 original PNG/JPEG file bytes.

Revision modulo ten selects the immutable embedded image. Transfers use 32-chunk
windows with bounded retries. The client rejects stale-revision, malformed and
duplicate chunks, and only publishes complete, successfully decoded images. Regular telemetry obtains
a fresh manifest if an announcement is lost. An incomplete image leaves the
previous scene visible. Old world requests receive error 5.

## Validation

- `python3 tools/test_prepare_slides.py`
- In Cubes: `python3 tools/test_slideshow_network.py`
- In Cubes: `python3 tools/test_walker_camera.py`
- In Cubes: `cargo check --offline`

The host tests cover preparation, packet compatibility, reordering, duplicates,
revision isolation, image orientation, initial camera pose and geometry budget. Native decoder
execution requires TRUEOS; host tests check its stride/dimension conversion.
Live rendering and sustained multi-player throughput still need rig validation.
