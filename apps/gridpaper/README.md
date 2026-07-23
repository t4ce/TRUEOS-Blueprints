# gridpaper

`gridpaper` is a no-heap DIN A4 grid document and Blueprint demo app.

The Blueprint opts into the F2 replicatable lifecycle. On pause, TRUEOS detaches
the VM owner's UI4 presentation while retaining the kernel-owned Gridpaper page,
3D scene, GPU allocations, and last front buffer. Resuming the same VM slot
re-arms that producer and attaches a new UI4 window session to the retained
scene; the Blueprint does not checkpoint UI4 or GPU handles.

The physical page is 210 mm by 297 mm. Its centered grid has 39 columns and 55
rows of 5 mm cells, for a 195 mm by 275 mm grid and 2,145 addressable cells.
That leaves 7.5 mm margins at the left and right and 11 mm margins at the top
and bottom. Each cell is a stable
13-byte record with separate primary and optional upper UTF-8 glyph fields.
Each field accepts one Unicode scalar encoded in up to four bytes. Foreground
and background palette colors plus bold, strikeout, underline, and italic style
bits remain native cell fields shared by both glyphs.

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
27,885-byte page, so changing paint does not edit a cell or rebuild its font
triangles.

The data API has three granularities on both `Snapshot` and `EditSession`:

1. typed cell access with `cell` and `set_cell`;
2. zero-copy encoded row access with `row_bytes` and `row_bytes_mut`;
3. zero-copy whole-page access with `raw` and `raw_mut`.

`cell_bytes` and `cell_bytes_mut` are also available for targeted encoded I/O.
Raw writes are deliberately checked only when read through the typed API.

`GridPaper` owns two 27,885-byte page buffers. A `Snapshot` reads the published
buffer while an `EditSession` writes the other one. `SnapshotCadence` supports
manual, edit-count, millisecond, or combined thresholds. Callers supply their
monotonic millisecond timestamp to `edit`, `tick`, and `publish`, so the data type
does not need a timer or runtime dependency.

`PublishMode::SwapOnly` publishes by exchanging two indices in O(1), intended
for full-page producers. The default `PreserveIncrementalEdits` mode additionally
copies 27,885 bytes after each exchange so the next edit buffer starts from the
latest snapshot.

The app publishes that fixed page image through the dedicated `gridpaper`
transport. The call only validates and copies into a kernel-owned double buffer;
an Embassy task independently consumes the newest generation. The kernel owns
the UI4 window and builds the paper, cell backgrounds, grid, decorations, and
positioned font outlines as resident GPU triangle meshes. The last good scene
and UI4 front buffer remain live until a newer snapshot has been built and
published successfully. Foreground/background colors, bold, underline,
strikeout, and a centered italic outline shear are represented now. A cell with
only its primary glyph uses the normal centered size. When an upper glyph is
present, the primary renders slightly smaller and lower-left while the smaller
second glyph renders at the upper-right, giving the fixed `x²` composition.

Focused UI4 keyboard input starts in the primary field. Typing replaces that
single glyph and advances one cell. Tab toggles the selected cell between its
primary and upper fields. Typing an upper glyph replaces it without advancing;
Delete/Entf or Backspace clears it. An upper glyph cannot exist without a
primary glyph, and deleting the primary clears both fields.

Print Screen captures the focused GridPaper generation immediately and hands
its stable kernel-owned snapshot to `trueos::print2d`. The Blueprint does not
choose or discover a device: the BSP spooler drains the asynchronous queue into
the first online plain-IPP printer advertising PWG Raster, and the app polls the
returned job ID through `Queued`, `WaitingForPrinter`, `Rendering`,
`Connecting`, `Sending`, `Submitted`, `Printing`, and the terminal `Completed`,
`Failed`, `Canceled`, or `OutcomeUnknown` states. The last state prevents an
ambiguous network failure from silently resubmitting and printing a duplicate
page. Every transition is also emitted at INFO through the kernel log router.
Printing always renders the page at physical 100%—the demo's 150% display zoom
and pan are intentionally not part of the A4 job.

The retained text path prefers the uploaded `Inconsolata-Regular.ttf` face for
each cell. If a Unicode value is absent from Inconsolata, the kernel selects the
uploaded `NotoSansSC[wght].ttf` face for that cell, then uses the embedded face
only as a final fallback. This preserves monospaced ASCII specimens without
losing the wider Unicode showcase.

At startup the demo leaves unused cells empty and places three deterministic
Unicode waves at different positions in one edit transaction. It also shows
`0` through `9` and `a` through `g` as normal, bold, and italic specimens, plus
one `x²` composite. The 103 cells cover Greek, mathematical, Cyrillic, geometric, musical, card-suit,
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
199×279 mm viewport is about 853×1196 pixels: cells are about 21.4 pixels at
100% and 32.1 pixels at the demo's 150% zoom, with the enlarged right and bottom
parts naturally outside that viewport.
