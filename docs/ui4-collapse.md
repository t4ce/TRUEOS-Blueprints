# UI4 collapse

After opening a `ui4_scene::Frame`, register a decoded image:

```rust
frame.enable_collapse(image.width(), image.height(), image.as_raw())?;
```

This is the complete collapse integration. UI4 adds **collapse** to the frame's
menu, displays the image near the window's bottom-left, and provides **restore**
and primary click/release to restore the normal presentation. A drag never
restores the window. Existing fixed and dynamic app menus keep their actions;
the standard row is added by the kernel. Restore works without an app menu pump.

The image must be tightly packed, top-down, straight-alpha RGBA8, with each
extent from 1 through 256 pixels. Its exact size and aspect ratio become the
compact frame extent: no resampling or forced square. PNG decoding belongs to
the app. Registration copies the bytes; the source can be dropped afterward.
Calling again replaces the icon, including while collapsed.

The producer's frame, layers, position, docking state, and logical dimensions
remain intact. Ongoing publications update the normal presentation without
replacing the compact icon. UI4 consumes compact pointer and keyboard input;
apps need no compact renderer, resize, state machine, or restore handler.
Registered icon frames are retired with their window/session, after compositor
readers release them.

Kernel callers can use the reexported `ui4::enable_window_collapse` with an owned
window identity and pitched RGBA bytes. `src/ui4/collapse.rs` owns projection,
alpha conversion, and click policy; the window broker owns registration lifetime.
The versioned C ABI uses the existing chunked image upload transport, so a full
256x256 RGBA image can cross the smaller VM communication page safely.

Rebuild the kernel and each migrated Blueprint together. The new C ABI is
additive; existing symbol signatures are unchanged.
