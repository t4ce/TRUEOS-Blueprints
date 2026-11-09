# Link3D

One shared K70 Mini GLB mesh, instanced once per virtual HID-HUT keyboard. Combo
colors come from the shared input palette; the chassis is gray, metallic 0.5,
roughness 0.4. Drag to orbit, scroll to zoom, Home to reset.

Physical and virtual HID-HUT mice appear as palette-colored cuboids. Each is four
regular key pitches wide and matches the GLB keyboard's height and front-to-back
depth. A mouse sits to the right of the keyboard in its combo bucket; unpaired
mice get their own rows. The camera frames both device types, including mouse-only
scenes. The live roster, combo membership and colors refresh every three seconds.
Tablets have their own slot after the mouse slot, use a quarter-height cuboid,
and carry a `TABLET` label; mouse placeholders carry `MOUSE`. RDP's absolute
pointer is a tablet and follows its actual HUT combo. Pointer movement does not
move these placeholders, and bucket assignment remains in Link.

One resident vertex/index buffer holds the keyboard and one 24-vertex mouse box.
Both use shared PBR geometry with 64-byte instance transforms, separate index
ranges and the central combo palette. Adding devices uploads transforms and
palette colors rather than duplicating either mesh. Mouse display requires the
host-backed mouse and tablet snapshots in the Blueprint HUT bridge (kinds 3 and
4); older guest-local registries return empty rosters. Deploy the corrected
kernel, then reopen Link3D. All three device classes share the same resident
geometry buffers, with up to 32 instances of each class.

Each visible bucket has a `BUCKET <combo_id>` caption anchored above its row;
unassigned devices show `UNASSIGNED`. Captions face the viewer and reuse the same
kernel Microfont glyph textures as the key legends, with a light backing for
readability. A paired keyboard and mouse share one caption.

Cold styled Microfont rasterization owns Render0. Link3D publishes the 3D scene
on Render1 while waiting for all glyph textures, then enables the overlay on
Render0. Caption backings are also withheld until their glyphs are ready. Ordinary
overlay backpressure still reacquires and redraws a canceled frame; a busy final
publication retries the same completed lease.

The audited German map in `keyfarm/mapping.json` supplies **36 black legends:
A–Z and 0–9**. Y and Z follow the recorded physical layout. The build checks the
map's GLB hash and finds the corresponding key's top plane. A flat glyph quad sits
0.025 model units above it. All glyph cells start 0.05 units inside the red key
highlight's top edge. Capitals are horizontally centered; numbers remain 0.4 units
left of the key center. The glyph cell is 0.7 model units high, with Microfont's
6:11 aspect. Other keys remain blank.

Glyph coverage comes from the kernel's existing shared Microfont alpha atlas,
through the font-sprite request/status facade. Each of the 36 transparent black
glyph textures is requested once and reused across all boards. Each texture stays
35×64 pixels, with 1.5 extra source pixels of native stroke width. Styled R8 masks
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
