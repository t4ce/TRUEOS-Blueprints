# potatomin

One-shot native GPU primitive demo. No window, input, actions, or render loop.

- `dots.png`: 100 green native POINT_LIST dots, 8 pixels wide, in a 10×10 grid.
- `lines.png`: 50 red native LINE_LIST segments, pairing the same grid vertices.
- Both images: 1280×720 (16:9), opaque dark background, RGBA8.

Each run uploads 100 XYZ vertices and 100 indices, renders each image once into
an offscreen GPU backbuffer, reads pixels after exact GPU retirement, and commits
the PNG to the app's TRUEOSFS folder. Relative filenames remain app-local.
Rerunning replaces those two files. The Blueprint then shuts down VMX.
A transient renderer Busy is retried before GPU admission.

Build from the TRUEOS-Blueprints root:

```sh
cargo bp potatomin
```

Requires a kernel rebuilt with the additive
`trueos_cabi_vgpu_offscreen_indexed_batch_v1` API. It uses the same authenticated
immediate-color shader and native batch renderer as Potato Stamps; there is no
CPU primitive rasterizer. The offscreen broker privately allocates RGBA8 storage,
checks ownership and bounds, renders, and copies tightly packed rows into an
ordinary readback buffer. Uncertain GPU retirement quarantines storage.

Host verification: `cargo test -p potatomin --lib` checks geometry and PNG RGBA
roundtripping; GPU execution requires supported TRUEOS hardware.
