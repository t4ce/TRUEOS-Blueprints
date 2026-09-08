# UI4 image viewer

Run `img apps/common/images` in Shell2, or select the img Matrix slot and enter
`show apps/common/images` on the normal Shell2 prompt row. The Blueprint never
claims a terminal TUI. Inferred PNG and JPEG files are sorted by name. Left/Up selects the
previous image; Right/Down selects the next; both ends wrap. Folder browsing
reuses one window and fits the complete image within it. Transparent pixels
are composited onto black. A single file still opens the native pan/resize
viewer. Multiple file arguments open one fixed frame per image. `list` reports
each open frame and `close all` closes them.

Launching without a source opens the first inferred image in
`apps/common/images`; if that folder has no supported image, img opens a
640×480 neutral-gray frame instead.

`img` uses the shared kernel media service for both PNG and JPEG. Its output
reports file-read time, the complete media-service decode/readback call, and
time through UI4 publish. Publish is submission, not proof of display scanout.

`list`, `show`, `close all`, and `exit` are VMX-minishell commands entered on
the persistent Shell2 prompt row. UI4 Escape closes the selected image frame.

The gallery retains only its current decoded image; navigation reads and
decodes each selection, so measurements do not hide work behind a cache.
