# RDP window viewer

On-demand, view-only Blueprint for one Ubuntu window. Launch `rdp`, enter the
Ubuntu machine's IP, and press Enter. The default port is UDP **43402**;
`IP:port` overrides it. Esc disconnects; Ctrl+Q quits. Keyboard and pointer
input are never sent to Ubuntu.

The TUI shows connection status. Video opens in a separate native UI4 video
frame using TRUEOS's existing Intel media decode and GuC conversion path.
The stream is baseline H.264, normally 1280×720 at 30 fps. It needs a kernel
containing the new `vmedia::LiveVideo` ingress operations (10–16); updating
the Blueprint alone on an older kernel is insufficient.

Start the Ubuntu side using the instructions in
[TRUEOS-RDP/server](../../../TRUEOS-RDP/server/README.md). For the first
hardware test, use its synthetic source, then select one X11 window. An Xorg
desktop session is the supported first capture target. Native Wayland window
capture is not implemented. This is a TRUEOS video protocol, without Microsoft
RDP compatibility, audio, or remote control. Use a trusted LAN: the prototype
does not authenticate or encrypt its UDP stream.

Build without publishing or deploying:

```sh
cd /home/t4ce/Repositories/TRUEOS-Blueprints
TRUEOS_BLUEPRINT_SKIP_APPS_PUBLISH=1 cargo run --bin trueos-blueprint -- rdp
```

The result is `dist/rdp.bp`. Build the matching kernel separately:

```sh
cd /home/t4ce/Repositories/TRUEOS
cargo build --release --bin TRUEOS --locked
```

The kernel ELF is `tgt/x86_64-unknown-trueos/release/TRUEOS`. These commands
build local artifacts; they do not publish, deploy, or reboot a machine.

Host checks and a receive probe (the host probe does not decode/display video):

```sh
cd /home/t4ce/Repositories/TRUEOS-Blueprints
cargo test --manifest-path apps/rdp/Cargo.toml --target x86_64-unknown-linux-gnu
cargo run --manifest-path apps/rdp/Cargo.toml --target x86_64-unknown-linux-gnu -- --probe 5 127.0.0.1
```

The receive path checks the source endpoint, session, lengths and CRC, reorders
fragments/pictures within bounded buffers, and abandons missing pictures after
120 ms. Losing a reference picture or filling the decoder queue closes the
native decoder and waits for a new complete SPS/PPS/IDR picture. The server
sends one every second. Disconnecting closes the UI4 playback session; the
server expires the subscription after three seconds without a keepalive.

Bare-metal acceptance checks:

1. With the matching kernel and Blueprint available, run the Ubuntu server
   with `--test-source`, connect in `rdp`, and verify an animated native UI4
   frame appears while the TUI remains responsive.
2. Disconnect and reconnect; close the video frame and reconnect; verify slots
   are released and reused. Moving/resizing the UI4 frame should work through
   the existing video window controls.
3. Stop/restart the Ubuntu server and verify timeout and keyframe recovery.
4. Repeat with `--window` selecting a visible X11 window. Verify keyboard and
   pointer actions on TRUEOS do not control Ubuntu.

Local unit tests and synthetic UDP/software-decode checks do not verify Intel
hardware decode, display publication, or actual Ubuntu window capture. Those
are the remaining live acceptance checks.
