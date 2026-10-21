# Gridpape

A standalone, static DIN A4 sheet: white paper and a gray 5 mm grid.
The Blueprint owns its UI4 immutable frame through `trueos::ui4_scene`.
It does not use the Gridpaper service or its snapshot/font API.

The page is 210 × 297 mm. As in Gridpaper, the centered grid has 39 × 55
cells (195 × 275 mm), leaving 7.5 mm side margins and 11 mm top/bottom
margins. UI4's physical-extent query uses the active output's EDID metrics;
if those metrics are unavailable, the app exits rather than guessing DPI.
Pixel-center snapping limits physical spacing error to pixel quantization.

One V2 indexed batch draws 96 independent native `LINE_LIST` primitives
(192 vertices/indices), single sampled, without adjacency or triangle
expansion. The native line-only batch path first fills the target white
through BCS0 and waits for completion before the RCS line pass. There is no
fullscreen clear triangle. An unavailable BCS engine returns Busy; an
uncertain submitted fill quarantines the target through the existing vGPU
failure path.

The completed frame stays resident without continuous redraw. Resize redraws
once and preserves physical cell size, clipping or exposing white space
instead of stretching the grid. Closing the window releases the frame and
GPU device. This app does not advertise the replicatable lifecycle.

Build from the Blueprint repository:

```sh
cargo bp gridpape
```

Requires a kernel built with the additive
`trueos_cabi_ui4_scene_physical_extent_v1` API and BCS0 native-line clear path.
