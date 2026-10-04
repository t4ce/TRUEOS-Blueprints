TRUEOS OSM demo

Starts at 51.471336, 13.827807, zoom 17.
TRUEOS: UI4 pan gestures move the map; wheel changes zoom (0..19).
Host preview: run cargo run from apps/osm; output is osm-demo.png.
OSM_CACHE_DIR selects a persistent writable directory (default osm-tile-cache).
OSM_TILE_URL selects an HTTPS raster tile server base URL, using /z/x/y.png.
Only current viewport tiles are fetched. Cache is retained for seven days.
Keep it between launches. Failed tiles leave gray squares and stderr errors.
Decoded RGBA tiles also live in a session-only REDB ImageDatabase from the
trueos-redb RAM helper. LRU eviction limits it to 128 tiles (32 MiB pixels plus
REDB overhead). RAM hits skip file I/O and PNG decoding; entries expire no later
than their disk source. The RAM database is never serialized or persisted.
OSM_CACHE_MODE=ram disables the disk layer for an alternate provider whose
policy allows it. The public tile.openstreetmap.org service retains disk caching.

UI4 presentation uploads one opaque sprite using the same ID, then submits a
full-frame 1:1 copy quad with source_over=false. This meets the existing kernel
BCS0 XY_FAST_COPY_BLT fast path. All Busy responses are cooperatively retried.
CPU still assembles and uploads each viewport. BCS0 replaces final framebuffer
painting; it does not scale tiles or remove network waits. Kernel log evidence:
ui4/blueprint: opaque viewport retired backend=bcs0
The diagnostic is logged once globally, so it may already have been emitted by
another app. Emulator and unsupported hardware can use UI4's fallback paths.
Sequential loading still pauses input while fetching.
Attribution: https://www.openstreetmap.org/copyright
Policy: https://operations.osmfoundation.org/policies/tiles/

API v0.6 is the editing/data API, not the image service. osm-api 0.1.1
wraps that API and does not supply raster tiles. It also uses the development
server in debug builds. It is therefore not a dependency of this image demo;
a future geographic-data overlay can integrate it separately.
TLS certificate validation is enabled.
