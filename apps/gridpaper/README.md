# gridpaper

`gridpaper` is a no-heap DIN A4 grid document and Blueprint demo app.

The physical page is 210 mm by 297 mm. Storage has 21 columns and 30 rows:
29 complete 10 mm rows plus one 7 mm final row, for 630 addressable cells.
Each cell is a stable 20-byte record with a 16-byte fixed UTF-8 value,
foreground and background palette colors, and bold, strikeout, underline, and
italic style bits.

The page has one global font/render scale. It defaults to 100% and is available
through `GridPaper::scale_percent` and `GridPaper::set_scale_percent`; snapshots
also expose the scale that belongs to their view.

The data API has three granularities on both `Snapshot` and `EditSession`:

1. typed cell access with `cell` and `set_cell`;
2. zero-copy encoded row access with `row_bytes` and `row_bytes_mut`;
3. zero-copy whole-page access with `raw` and `raw_mut`.

`cell_bytes` and `cell_bytes_mut` are also available for targeted encoded I/O.
Raw writes are deliberately checked only when read through the typed API.

`GridPaper` owns two 12,600-byte page buffers. A `Snapshot` reads the published
buffer while an `EditSession` writes the other one. `SnapshotCadence` supports
manual, edit-count, millisecond, or combined thresholds. Callers supply their
monotonic millisecond timestamp to `edit`, `tick`, and `publish`, so the data type
does not need a timer or runtime dependency.

`PublishMode::SwapOnly` publishes by exchanging two indices in O(1), intended
for full-page producers. The default `PreserveIncrementalEdits` mode additionally
copies 12,600 bytes after each exchange so the next edit buffer starts from the
latest snapshot.

The app publishes that fixed page image through the dedicated `gridpaper`
transport. The call only validates and copies into a kernel-owned double buffer;
an Embassy task independently consumes the newest generation. The kernel owns
the UI4 window and builds the paper, cell backgrounds, grid, decorations, and
positioned font outlines as resident GPU triangle meshes. The last good scene
and UI4 front buffer remain live until a newer snapshot has been built and
published successfully. Foreground/background colors, bold, underline, and
strikeout are represented now; italic remains preserved in the wire data for a
future shader-side slant transform.

At startup the demo takes one seed from the Blueprint kernel RNG, evaluates a
small 2D Perlin field over the page, and fills every cell with `x` or `o` in one
edit transaction. The complete initialized page is then sent as one snapshot.
