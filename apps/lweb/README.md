# lweb — local websites

An on-demand Blueprint hosting small server-rendered apps with Axum. The app
index is `/`; the first app is `/weather`. No JavaScript, CDN, frontend build,
or runtime asset directory is required. CSS and PNGs are embedded in the binary.

## Run on the host

From `apps/lweb`:

```sh
cargo run --locked --target x86_64-unknown-linux-gnu
```

Open `http://127.0.0.1:8787/` in Hull or another browser. The listener binds only
to IPv4 loopback, using an explicit port accepted by Hull's local resource loader.
On TRUEOS it uses the same native network runtime as Frog and remains an app in
`apps.json`; it is not a buildin. Close the Blueprint to stop the server.

## Weather

- `/weather`: live weather at Frog's default Holzminden coordinates.
- `/weather?lon=13.405&lat=52.52`: select a point (longitude first).
- `/weather?demo=true`: bundled Frog sample, explicitly labelled as offline data.

The weather page reuses `frog::weather` and its existing OpenWeather integration.
It shows current conditions and up to eight daily rows: summary, day/feels/min/max/
night temperatures, rain probability, humidity, wind speed/direction, and UV.
Live responses are cached for five minutes; failed requests retry after 30 seconds.
Only the latest selected point is retained. Live failures show an error and a
sample link, without silently replacing the forecast with sample data. API URLs
and raw upstream errors are not rendered. The existing Frog credentials and TLS
configuration are inherited; lweb does not add another provider or API key.

## Hull layout and icons

The local stylesheet uses Hull's existing block/flex layout, basic selectors,
colors, padding, and rounded corners. It needs no framework adaptation. The
semantic HTML forecast table uses explicit flex column widths because Hull does
not implement browser table layout. Font sizes account for Hull's 2x page text
scale. Long summaries wrap and rows grow with their content. The page canvas
uses `rgba(237, 242, 240, 0.5)` for a 50% background tint; text, weather icons,
and panels retain their own opacity. This requires Hull's alpha-preserving canvas
renderer; it does not change the opacity of the entire UI4 window.

Ten weather icons were recovered from the old Frog assets in local Git history.
Their first 64×64 RGBA animation frames are preserved losslessly as static PNGs;
see [asset provenance](assets/weather/README.md) and [MIT notice](assets/weather/LICENSE).
The OpenWeather icon code selects the image, independently of translated text.
Missing codes use a cloud; summaries remain visible alongside every icon.

## Extend

Add a server route and an entry to `APPS` in `src/lib.rs`. The common page wrapper
provides the app index, navigation, and stylesheet. Ordinary GET navigation works
without DOM updates or client scripts. Keep future app state and data on the
server where possible.

## Check

```sh
cargo test --locked --target x86_64-unknown-linux-gnu
```

Tests exercise the offline forecast, data escaping, invalid coordinates, missing
routes/assets, stylesheet/license responses, and every icon mapping without an
upstream request. Host checks do not substitute for a TRUEOS boot/render check.
