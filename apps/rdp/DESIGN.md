# Ubuntu desktop viewing on TRUEOS

The `rdp` Blueprint opens one live Ubuntu monitor or application window in a native TRUEOS UI4 video frame. The Ubuntu user chooses the source and can stop sharing locally. The viewer receives video for the duration of the share. Its keyboard and pointer operate the local UI4 window. The session exposes zero Ubuntu login, input, clipboard, file, audio, or device capabilities.

## Components

| Component | Responsibility |
| --- | --- |
| `TRUEOS-Blueprints/apps/rdp` | On-demand viewer. Pair to an Ubuntu host, receive and validate video records, and submit H.264 access units through `vmedia::LiveVideo`. Reuse the existing UI4 window and Intel media decode path. |
| `TRUEOS-RDP/server` | Rust sharing daemon in the logged-in Ubuntu user's graphical session. Own the portal session, video pipeline, and one authenticated viewer connection. Start each share through a local action. |

The existing prototype already streams H.264 from an Xorg window. Its synthetic source and receive probe provide a baseline for testing. The current UDP subscription is for prototype diagnostics on a trusted test network. The supported viewer path will require authenticated transport with an explicit protocol version and zero automatic downgrade paths.

## Capture and encoding

1. On Ubuntu GNOME Wayland, the Rust daemon opens an XDG ScreenCast portal session and requests one monitor or window. The user makes the selection in the compositor's sharing dialog. Request an embedded cursor where advertised so the viewer can see the Ubuntu user's pointer. Use a fresh grant for every share. [XDG ScreenCast portal interface](https://github.com/flatpak/xdg-desktop-portal/blob/main/data/org.freedesktop.portal.ScreenCast.xml)
2. Consume the selected stream through the portal-provided PipeWire remote file descriptor. Run `pipewiresrc` and the encoder in one GStreamer pipeline, with Rust handling session state and reading complete encoded access units from `appsink`. Cap the raw frame queue and discard old raw frames under load. [GNOME Network Displays portal capture](https://sources.debian.org/src/gnome-network-displays/0.90.5-3/src/nd-screencast-portal.c/) · [GStreamer x264 encoder](https://gstreamer.freedesktop.org/documentation/x264/index.html)
3. Start with a fixed 1280×720 output at up to 30 fps. Preserve the source aspect ratio as windows resize. Encode 8-bit 4:2:0 baseline-compatible H.264 as Annex B access units, with zero B-frames and SPS/PPS at each IDR. Keep a roughly one-second keyframe interval and start each new viewer at a complete IDR with its parameter sets. Inter-frame prediction keeps static desktop regions cheap and handles moving content in the same stream. [GStreamer x264 encoder](https://gstreamer.freedesktop.org/documentation/x264/index.html)

Use software encoding for the first end-to-end pass. Evaluate DMA-BUF import and hardware encoding after the baseline works. PipeWire exposes damage and cursor metadata when negotiated and supplied by the producer. Measure whether using it changes capture cost or idle bandwidth before adding metadata to the wire format. [PipeWire buffer metadata](https://docs.pipewire.org/group__spa__buffer.html)

## Pairing and transport

- Start with one direct LAN connection over TLS/TCP. Show the Ubuntu address, server certificate fingerprint, and a short-lived high-entropy invitation locally. The viewer verifies the certificate against that fingerprint and sends the invitation inside the TLS connection. Consume the invitation only after successful admission. Invalidate it when sharing ends. Ubuntu desktop credentials are outside the protocol.
- Use a versioned, bounded record format. The viewer sends `Join` once. The server sends stream parameters, length-prefixed H.264 access units with sequence numbers and monotonic presentation timestamps, and shutdown status when possible. The viewer closes the connection to leave. Only encoded video that belongs to the active, authenticated share reaches `LiveVideo`.
- Keep pending media bounded by size and age. If TCP backpressure makes video stale, close the connection and discard its queued media. If the decoder queue fills, reset playback and resume only at a complete IDR with parameter sets. Measure live latency under congestion and packet loss before considering a datagram transport.

## Share lifecycle

Start capture after local approval. Begin network delivery only after invitation validation. A local Stop action, portal closure, screen lock, or logout ends the share, invalidates the invitation, and closes the viewer connection. Verify that the Ubuntu lock signal is reliable on the target release and that unlock requires a new local share. On disconnect or read timeout, TRUEOS releases the native playback session and reports the status in the viewer. Each new viewing connection requires a fresh invitation.

## First implementation and acceptance

1. Carry the existing synthetic H.264 source over TLS to the Blueprint's receive probe. Check cross-target TLS support, certificate pinning, invitation rejection and reuse, and bounded record parsing. Confirm an invalid viewer receives zero video bytes.
2. Feed the authenticated stream through `LiveVideo` on the target TRUEOS hardware. Verify first picture, playback, decoder recovery under backpressure, reconnect, and UI4 cleanup after closing the video window.
3. Replace the synthetic source with portal-selected Wayland capture on the target Ubuntu GNOME release. Test monitor and window selection, source resizes, local Stop, screen lock, logout, and the visible cursor. Verify that TRUEOS pointer and keyboard actions stay local. Record first-frame time, capture-to-display latency, and bandwidth for an idle desktop and a moving window.
