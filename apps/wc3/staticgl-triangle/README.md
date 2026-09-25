# Static GL Triangle

This opt-in Blueprint opens a basic UI4 frame and renders an RGB triangle from
three position and color vertices. Its `staticgl_triangle` library is also the
renderer used by the WC3 static OpenGL provider.

Build and publish it through the normal Blueprint workflow:

```sh
cargo bp staticgl-triangle
```

The current authenticated single indexed-draw shader uses a fixed green
fragment color. The helper instead subdivides the three colored vertices into
144 small triangles, computes a color for each, and submits them together
through the authenticated immediate-RGBA indexed-batch path. This gives a
visible red, green, and blue gradient on the existing vGPU runtime. It is a
bounded approximation of per-fragment color interpolation.

The demo draws once, waits for the vGPU timeline, publishes its 640×480 UI4
frame, and keeps that frame open. The caller of `TriangleRenderer::draw` owns
the UI4 frame, vGPU device and queue, and acquired surface.
