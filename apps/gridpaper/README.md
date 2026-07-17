# gridpaper

`gridpaper` is a no-heap DIN A4 grid document and Blueprint demo app.

The physical page is 210 mm by 297 mm. Its centered grid has 37 columns and 53
rows of 5 mm cells, for a 185 mm by 265 mm grid and 1,961 addressable cells.
That leaves 12.5 mm margins at the left and right and 16 mm margins at the top
and bottom, matching the reference graph-paper PDF. Each cell is a stable
20-byte record with a 16-byte fixed UTF-8 value,
foreground and background palette colors, and bold, strikeout, underline, and
italic style bits.

The page has one global font/render scale. It defaults to 100% and is available
through `GridPaper::scale_percent` and `GridPaper::set_scale_percent`; snapshots
also expose the scale that belongs to their view. The scale is applied to the
grid, rulers, decorations, and glyphs as one top-left-anchored document zoom.
The current Unicode showcase sets its document instance to 150% for inspection.

Foreground palette values also act as stable text-animation selectors, similar
to CSS classes. `GridPaper::set_text_color_animation` assigns a fixed-storage
`ColorAnimation` to every active text cell using one foreground color. A program
has two to eight RGBA keyframes at 0..1000 offsets, an RGBA channel mask, a
16 ms to 600 s duration, linear or ease-in-out-sine timing, and `once`, `loop`,
or `alternate` iteration. Animation metadata is transported separately from the
39,220-byte page, so changing paint does not edit a cell or rebuild its font
triangles.

The data API has three granularities on both `Snapshot` and `EditSession`:

1. typed cell access with `cell` and `set_cell`;
2. zero-copy encoded row access with `row_bytes` and `row_bytes_mut`;
3. zero-copy whole-page access with `raw` and `raw_mut`.

`cell_bytes` and `cell_bytes_mut` are also available for targeted encoded I/O.
Raw writes are deliberately checked only when read through the typed API.

`GridPaper` owns two 39,220-byte page buffers. A `Snapshot` reads the published
buffer while an `EditSession` writes the other one. `SnapshotCadence` supports
manual, edit-count, millisecond, or combined thresholds. Callers supply their
monotonic millisecond timestamp to `edit`, `tick`, and `publish`, so the data type
does not need a timer or runtime dependency.

`PublishMode::SwapOnly` publishes by exchanging two indices in O(1), intended
for full-page producers. The default `PreserveIncrementalEdits` mode additionally
copies 39,220 bytes after each exchange so the next edit buffer starts from the
latest snapshot.

The app publishes that fixed page image through the dedicated `gridpaper`
transport. The call only validates and copies into a kernel-owned double buffer;
an Embassy task independently consumes the newest generation. The kernel owns
the UI4 window and builds the paper, cell backgrounds, grid, decorations, and
positioned font outlines as resident GPU triangle meshes. The last good scene
and UI4 front buffer remain live until a newer snapshot has been built and
published successfully. Foreground/background colors, bold, underline,
strikeout, and a centered italic outline shear are represented now.

The retained text path prefers the uploaded `Inconsolata-Regular.ttf` face for
each cell. If a Unicode value is absent from Inconsolata, the kernel selects the
uploaded `NotoSansSC[wght].ttf` face for that cell, then uses the embedded face
only as a final fallback. This preserves monospaced ASCII specimens without
losing the wider Unicode showcase.

At startup the demo leaves unused cells empty and places three deterministic
Unicode waves at different positions in one edit transaction. It also shows
`0` through `9` and `a` through `g` as normal, bold, and italic specimens. The
102 cells cover Greek, mathematical, Cyrillic, geometric, musical, card-suit,
arrow, and operator glyphs together with underline and strikeout variants. It
uses no startup RNG, Perlin field, `x`, or `o` fill.

Each wave uses all 17 active foreground selectors. Every selector receives an
eight-keyframe RGBA rainbow loop, filling both the animation selector table and
the fixed keyframe capacity. The kernel samples the phase-shifted programs on
GridPaper's 16 ms Embassy cadence and republishes only when a sampled text color
changes; resident font VB/IB allocations remain untouched.

UI4 routes middle-button pan gestures to the GridPaper kernel consumer. The
consumer applies motion during the active gesture, converts pixel deltas to
retained-scene coordinates, and clamps them to the scaled document bounds. The
3D renderer moves every resident grid and font layer with its fixed-function
viewport transform, so hot dragging neither enters the Blueprint snapshot API
nor rewrites or reallocates resident geometry. Translated draws bypass the
original canonical clip volume and retain the target scissor as their final
clip, allowing rows and glyphs outside the initial view to become visible.

The kernel retains the scene directly in millimetre coordinates and uses the
display's EDID physical dimensions to rasterize the document. The unused A4
paper margins are not part of the UI4 frame. A 4 mm transparent gutter above
and to the left carries ruler ticks every 5 mm, with longer 1 cm ticks and the
largest ticks every 3 cm. On the currently detected HP E273q, the fixed
189×269 mm viewport is about 810×1153 pixels: cells are about 21.4 pixels at
100% and 32.1 pixels at the demo's 150% zoom, with the enlarged right and bottom
parts naturally outside that viewport.
