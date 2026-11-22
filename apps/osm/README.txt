TRUEOS OSM demo

Starts at 51.471336, 13.827807, zoom 17.
TRUEOS: drag with the primary or middle button to pan. The wheel zooms around
the pointer over the map (0..19). Each detent changes displayed zoom by 0.25,
including every detent in a batched event. Intermediate steps reuse the current
tile level until the view is one full level away. Reversing unwinds local steps.
The attribution footer stays fixed.
Host preview: run cargo run from apps/osm; output is osm-demo.png.
OSM_CACHE_DIR selects a persistent writable directory (default osm-tile-cache).
OSM_TILE_URL selects an HTTPS raster tile server base URL, using /z/x/y.png.
Only current viewport tiles are fetched. Cache is retained for seven days.
Keep it between launches. Failed tiles use recent imagery where available and
leave any uncovered areas white. The loader retries with exponential backoff
from one to thirty seconds. Retries continue without navigation and stop when
a tile leaves the current demand.
Hardware logs identify fetch request/status/body or PNG decode failures and
the next retry delay. Invalid disk PNGs are evicted, and HTTP errors or invalid
downloaded PNGs are never written to the cache.
Decoded RGBA tiles also live in a session-only REDB ImageDatabase from the
trueos-redb RAM helper. LRU eviction limits it to 128 tiles (32 MiB pixels plus
REDB overhead). RAM hits skip file I/O and PNG decoding; entries expire no later
than their disk source. The RAM database is never serialized or persisted.
OSM_CACHE_MODE=ram disables the disk layer for an alternate provider whose
policy allows it. The public tile.openstreetmap.org service retains disk caching.

UI4 presentation uploads one opaque sprite using the same ID and submits one
full-frame 1:1 copy quad with source_over=false for the existing BCS0 fast path.
The app polls input between Busy attempts. A newer view can replace an
unsubmitted sprite. After a draw succeeds, the presenter finishes publishing
its write lease before submitting the next view. Admission Busy cancels the
lease and logs "sprite admission busy action=restart-frame". Kernel log evidence:
ui4/blueprint: opaque viewport retired backend=bcs0
The diagnostic is logged once globally. Emulator and unsupported hardware can
use UI4's fallback paths.

The map composes directly from decoded tiles into a reusable frame buffer.
Unscaled tiles use clipped row copies. Scaled tiles use nearest-neighbor
sampling from the original pixels. The current viewport and up to 64 recently
visible tiles stay resident. Recent tiles reappear immediately when panning
back and cover gaps at other zoom levels until the requested tiles arrive.
The loader requests visible missing tiles from the center out. Completions
are checked against the latest tile keys, then composed at their current
position and scale. Navigation and tile arrivals in one turn share a frame.

The UI and sequential tile loader run as local tasks on a pinned Blueprint
Tokio current-thread runtime. Viewport demand uses Tokio watch and completions
use an async channel. Pending network I/O yields to the UI task. New navigation
replaces queued old viewport work and lets an in-flight fetch finish.
The first four/every 120 presentations emit an Important "immediate viewport"
marker with zoom, extent and stale completions skipped.
Attribution: https://www.openstreetmap.org/copyright
Policy: https://operations.osmfoundation.org/policies/tiles/

API v0.6 is the editing/data API, not the image service. osm-api 0.1.1
wraps that API and does not supply raster tiles. It also uses the development
server in debug builds. It is therefore not a dependency of this image demo;
a future geographic-data overlay can integrate it separately.
TLS certificate validation is enabled.

Resize/maximize/restore events are handled by staging frame.resize, rendering
the immediate cached view at the requested extent, and publishing the
replacement while tiles load. The map center stays fixed. Footer text is
clipped safely for small frames.
Kernel retirement fix: begin_blueprint_frame must reap retired generations on
the shared path, including guest vmcalls. Updating only osm.bp does not fix a
kernel that still bypasses reclamation; boot the accompanying updated OS.
Retirement evidence is sampled at Important, so the normal log profile admits
"frame retirement reaped" without enabling noisy UI4 Trace logging.

Validation:
  cargo test --offline --target x86_64-unknown-linux-gnu \
    --manifest-path apps/osm/tests/host/Cargo.toml
The host harness compiles the actual app and presenter sources with upstream
reqwest/hyper and no TLS backend; network fixtures use HTTP on localhost.
It avoids the kernel-only imports in patched hyper on Linux. Production builds
continue using the app manifest and the patched TLS client.
The fixtures cover coalesced wheel input, cursor anchoring, recent tile reuse,
wrap, resize, footer clipping, stale completions, presentation Busy retries and
navigation during a pending tile fetch. Retry fixtures also cover recovery,
obsolete retries and invalid cache entries.
On TRUEOS, drag and zoom while tiles arrive, reverse direction across a tile
level, and maximize and restore while dragging. Cached coverage should move
with the camera and remain visible until newer imagery arrives.

Execution evidence at Important: "osm: execution=tokio-local runtime=current-thread loader=cooperative".

Kernel-owned compact window:
The frame registers its dynamic context menu before the first publication,
so right-click offers "collapse" and "Frog lon,lat" instead of the generic desktop menu.
Frog uses the frozen right-click map point, displays four decimal places, and
launches App Store headlessly with a one-shot vFile:launch script:
launch --sh3 Frog -- weather <longitude> <latitude>
App Store downloads/verifies Frog and forwards weather <longitude> <latitude>
as Frog's own start script. The kernel creates one Shell3 window and enters
Frog's Matrix slot there. Frog does not need to be installed in AppDB.
The launch retains full coordinate precision. Frog is disabled over the footer,
outside the projected world, and while compact.
The app registers its embedded 128x128 PNG once with Frame::enable_collapse.
UI4 adds collapse/restore to the app menu and resamples the image to its standard 64x64 icon,
with transparent pixels preserved. Click/release inside the tile restores the
normal window; dragging does not restore. While compact, only restore is offered.
The producer frame, map center/zoom, and logical extent remain intact. Background
tile work can continue without overwriting the compact icon. No app-side compact
renderer, resize transaction, animation, or input pump is needed.
The expanded map retains its opaque BCS0 copy path.
