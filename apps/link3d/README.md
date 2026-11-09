# Link3D

One shared K70 Mini GLB mesh, instanced once per virtual HID-HUT keyboard. Combo
colors come from the shared input palette; the chassis is gray, metallic 0.5,
roughness 0.4. Drag to orbit, scroll to zoom, Home to reset.

The audited German map in `keyfarm/mapping.json` supplies **36 black legends:
A–Z and 0–9**. Y and Z follow the recorded physical layout. The build checks the
map's GLB hash, finds the corresponding key's top plane and centers a flat glyph
quad 0.025 model units above it. The glyph cell is 1.0 model units high (20% smaller
than the first pass), with Microfont's 6:11 aspect. Other keys remain blank.

Glyph coverage comes from the kernel's existing shared Microfont alpha atlas,
through the font-sprite request/status facade. Each of the 36 transparent black
glyph textures is requested once and reused across all boards. Each texture stays
35×64 pixels, with 2.5 extra source pixels of native stroke width. Styled R8 masks
are cached by the kernel font service; original UI font tiles retain their style.
Cold variants are prepared on the blocking font worker, outside the frame lease.
There is no
Blueprint font rasterizer or pixel readback. The GLB vertex/index buffers and
per-board material colors stay on their shared instanced path.

This first pass projects four corners into UI4's ordinary textured sprite quads
after the keyboard pass. It rejects back-facing or near-clipped labels and follows
orbit, zoom, resizing and roster changes. Sprite composition has no shared scene
depth attachment or perspective-correct UV interpolation, so overlap at grazing
angles is approximate. This is the initial placement/size pass.

The kernel must include the Microfont-backed implementations of
`trueos_cabi_ui4_scene_font_sprite_request_v2` (explicit stroke width) and
`trueos_cabi_ui4_scene_font_sprite_status_v1`. Their previously declared Blueprint
facade alone is insufficient on an older kernel. Rebuild/deploy the kernel once
for this bridge, then reopen the updated Blueprint.

While a combo's cursor selects Link3D, physical key presses routed to this frame
show a red key-top patch with a white border on that source keyboard. Black
legends remain readable over the patch; blank keys and modifiers get the same
feedback. The border keeps presses visible on red combo keyboards as well.
Held keys stay marked until release, with a 90 ms minimum for quick taps.
Deselection, device loss, queue reset or removal clears old feedback.

Highlight placement uses the audited HID usage-to-mesh map, independently of
typed Unicode, Shift or the kernel's text layout. It covers 61 keys; hardware-local
Fn emits no event. Each board tracks its exact endpoint, so one combo's typing
does not light another keyboard. The physical event stream and combo selection
come from existing UI4 APIs; this feedback addition needs no kernel update.
Escape is delivered to the application for validation, and Home still resets the
camera. Kernel-reserved shortcuts keep their usual routing policy.
