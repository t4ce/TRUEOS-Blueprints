TRUEOS OSM demo

Starts at 51.471336, 13.827807, zoom 17.
TRUEOS: UI4 pan gestures move the map; wheel changes zoom (0..19).
Host preview: run cargo run from apps/osm; output is osm-demo.png.
OSM_CACHE_DIR selects a persistent writable directory (default osm-tile-cache).
OSM_TILE_URL selects an HTTPS raster tile server base URL, using /z/x/y.png.
Only current viewport tiles are fetched. Cache is retained for seven days.
Keep it between launches. Failed tiles leave uncovered white areas and stderr errors.
Decoded RGBA tiles also live in a session-only REDB ImageDatabase from the
trueos-redb RAM helper. LRU eviction limits it to 128 tiles (32 MiB pixels plus
REDB overhead). RAM hits skip file I/O and PNG decoding; entries expire no later
than their disk source. The RAM database is never serialized or persisted.
OSM_CACHE_MODE=ram disables the disk layer for an alternate provider whose
policy allows it. The public tile.openstreetmap.org service retains disk caching.

UI4 presentation uploads one opaque sprite using the same ID, then submits a
full-frame 1:1 copy quad with source_over=false. This meets the existing kernel
BCS0 XY_FAST_COPY_BLT fast path. Busy responses yield through Tokio timers before retrying.
CPU still assembles and uploads each viewport. BCS0 replaces final framebuffer
painting. Immediate pan/resize uses clipped CPU row copies; zoom reprojects the
already displayed pixels with nearest-neighbor sampling. Kernel log evidence:
ui4/blueprint: opaque viewport retired backend=bcs0
The diagnostic is logged once globally, so it may already have been emitted by
another app. Emulator and unsupported hardware can use UI4's fallback paths.
The UI and sequential tile loader run as local tasks on one pinned Blueprint
Tokio current-thread runtime. Viewport demand uses Tokio watch; completions use
Tokio async channels. There is no OS thread spawn or spawn_blocking in this app.
Pan, zoom and resize drain input first, then immediately publish the current
view with opaque white uncovered areas. Network waits, disk I/O and PNG decoding
run in the loader task; pending network I/O yields to the UI task. The existing URL/client/cache policy and limits are unchanged.
Only decoded tiles contributing to the current view are held as display state.
Tile completions are checked against the latest zoom and wrapped tile keys;
a still-needed tile is placed at its current screen position, otherwise skipped.
New navigation replaces queued old viewport work; an in-flight fetch finishes
normally. Tile arrivals incrementally repaint through the same opaque BCS0 path.
The first four/every 120 presentations emit an Important "immediate viewport"
marker with zoom, extent, missing tiles and stale completions skipped.
Attribution: https://www.openstreetmap.org/copyright
Policy: https://operations.osmfoundation.org/policies/tiles/

API v0.6 is the editing/data API, not the image service. osm-api 0.1.1
wraps that API and does not supply raster tiles. It also uses the development
server in debug builds. It is therefore not a dependency of this image demo;
a future geographic-data overlay can integrate it separately.
TLS certificate validation is enabled.

Resize/maximize/restore events are handled by staging frame.resize, rendering
the immediate white/reprojected view at the requested extent, and publishing
the replacement before waiting for tiles. The map center stays
fixed. Footer text is clipped safely for small frames.
Kernel retirement fix: begin_blueprint_frame must reap retired generations on
the shared path, including guest vmcalls. Updating only osm.bp does not fix a
kernel that still bypasses reclamation; boot the accompanying updated OS.
Retirement evidence is sampled at Important, so the normal log profile admits
"frame retirement reaped" without enabling noisy UI4 Trace logging.

Validation:
  cargo test --offline --target x86_64-unknown-linux-gnu \
    --manifest-path apps/osm/Cargo.toml
Includes a deliberately pending tile future on the same Tokio executor while
pan/zoom and timers continue, stale
completion rejection, latest-position tile painting, zoom scaling, resize,
footer clipping and world wrap. Host preview still waits for its final PNG.
On-OS validation: pan/zoom while tiles arrive, maximize/restore and pan farther
than one screen; movement should precede tile fills and white borders stay opaque.

Execution evidence at Important: "osm: execution=tokio-local runtime=current-thread loader=cooperative".

App-owned compact window:
The frame registers its dynamic context menu before the first publication,
so right-click offers only "collapse" instead of the generic desktop menu.
Collapse saves the expanded extent and position, shrinks the frame to 128x128,
displays an embedded 24,033-byte transparent PNG converted from logo.svg, and
animates toward the saved window's bottom-left over 180 ms, following Solara.
The compact tile enables UI4 primary-click activation. Left-click/release
inside it restores the saved extent and position; dragging does not restore.
The menu retains its single disabled collapse row while already compact.
The expanded map center/zoom/pixels are preserved. Queued tile loading pauses
until restore; an already-running request may complete normally.
Important markers: "osm: collapsed" and "osm: restored".
This uses the existing APIs without kernel changes or native worker threads.

Compact logo pixels use source-over on a transparent frame; PNG alpha is
preserved through UI4. The expanded map retains its opaque BCS0 copy path.
