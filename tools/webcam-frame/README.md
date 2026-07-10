# webcam-frame

A deliberately small, Ubuntu 26-only webcam window.

```sh
cd tools/webcam-frame
cargo run --release
```

The default camera resolution is 16:9 HD (`1280×720`). To request Full HD:

```sh
cargo run --release -- --1080p
```

Controls:

- `1`–`5`: select a detected camera
- `Space`: save a full-resolution PNG in the localized Pictures directory
- left mouse down anywhere: drag the borderless window
- `Esc`: close

The app uses V4L2 directly for capture and a software Wayland buffer for display. For smooth HD
capture it prefers MJPEG and falls back to YUYV/RGB, so it does not need GTK, GStreamer, or a GPU
API.

Snapshots use Ubuntu's localized XDG Pictures directory. For example, camera 1 on a German
desktop writes timestamp-named files to `~/Bilder/webcam_1/`.
