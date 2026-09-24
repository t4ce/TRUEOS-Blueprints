# Static GL Triangle

This opt-in Blueprint draws one RGB triangle into a basic UI4 frame through
the authenticated vGPU indexed-triangle path. Its `staticgl_triangle` library
is also the rendering helper used by the WC3 static OpenGL provider.

Build this Blueprint independently with:

```sh
TRUEOS_BLUEPRINT_SKIP_APPS_PUBLISH=1 cargo bp apps/wc3/staticgl-triangle
```

The build writes `dist/staticgl-triangle.bp` without publishing it. Launch that
artifact through the usual local Blueprint workflow. It opens a 640x480 streaming UI4 frame,
acquires a vGPU surface, draws red/green/blue vertex colors, waits for the
submission, and publishes the frame.

`TriangleRenderer::new(device)` creates the position-plus-RGBA shader pipeline
and an index buffer for indices `[0, 1, 2]`. `draw(queue, surface, vertices,
clear_rgba8_srgb)` uploads exactly three `Vertex { position, color }` records
and consumes the acquired surface with a triangle-list submission. The caller
waits for the returned `TimelinePoint` and publishes the UI4 frame.
