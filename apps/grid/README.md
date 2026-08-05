# grid

`grid` owns exactly one thing: a buffered UI4 scene frame, and the lifecycle
that keeps that frame honest across a replication checkpoint.

It is the replacement for `gridpaper`. Where `gridpaper` carried its own
snapshot pool, cell-patch transport, per-owner lease bookkeeping, and a
retained presentation that outlived its own window, `grid` carries none of it.

## The contract

    Frame::open_streaming(x, y, width, height)   // triple-buffered scene frame
    frame.begin(clear_rgba)                      // acquire the write buffer
    frame.publish(Damage::full(width, height))   // hand it to the compositor
    frame.close(CloseRequest::default())         // release the capability

Plus the two events a frame contract cannot be honest without: resize (the
extent the compositor gave us is the extent we draw to) and keyboard (`Esc`
closes).

## The one stamp

The frame is not blank: `"Grid"` is stamped once, centered, by the FontKernel
executor pool.

    frame.retain_font_canvas(Font::Inconsolata, (width, height), &[row])
    frame.present_font_canvas_view((width, height), (0, 0), clear_rgba)

`retain_font_canvas` is the whole claim/release cycle in one call. The kernel
takes an executor out of the pool, materializes the frame-sized RGBA canvas
once, then releases the claim together with the glyph masks. What the frame
keeps afterwards is the warm RGBA buffer — every later publish composes that
buffer and never re-enters FontKernel.

`Error::Busy` means the pool had nothing free. `grid` treats that as "ask
again next iteration" rather than blocking, which is what makes the stamp
**asynchronous to initialization**: the frame publishes empty from its very
first iteration and adopts the canvas on whichever iteration the pool answers.
A resize invalidates the canvas (it is frame-sized), so the stamp is retaken.

### Centering without a measurement call

FontKernel exposes no advance-width query to a Blueprint. It does not need to:
Inconsolata is monospace at exactly 500/1000 units per em, so `"Grid"` is
`4 × 0.5 × font_pixels` wide. `SceneOrigin` positioning places the baseline one
em below the row's `y`, making `y` the top of the em box. Both axes are
therefore exact, not approximated.

## Replication lifecycle

A `Frame` is a host capability handle. On `PreparePause`, `grid` stops producing
frames, closes the frame, and only then calls `replication::ready`. The resumed
instance — original or clone — opens a **fresh** frame and takes a **fresh**
stamp.

Nothing is retained across the checkpoint boundary. A resumed instance holds no
window, no session, no canvas, and no GPU allocation belonging to its
predecessor, so two live instances can never reference the same presentation.
