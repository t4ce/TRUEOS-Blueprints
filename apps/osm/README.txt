TRUEOS OSM demo

Starts at 51.471336, 13.827807, zoom 17.
TRUEOS: UI4 pan gestures move the map; wheel changes zoom (0..19).
Host preview: run cargo run from apps/osm; output is osm-demo.png.
OSM_CACHE_DIR selects a persistent writable directory (default osm-tile-cache).
OSM_TILE_URL selects an HTTPS raster tile server base URL, using /z/x/y.png.
Only current viewport tiles are fetched. Cache is retained for seven days.
Keep it between launches. Failed tiles leave gray squares and stderr errors.
Sequential loading pauses input while fetching; this is an initial demo.
Attribution: https://www.openstreetmap.org/copyright
Policy: https://operations.osmfoundation.org/policies/tiles/

API v0.6 is the editing/data API, not the image service. osm-api 0.1.1
wraps that API and does not supply raster tiles. It also uses the development
server in debug builds. It is therefore not a dependency of this image demo;
a future geographic-data overlay can integrate it separately.
TLS certificate validation is enabled.
