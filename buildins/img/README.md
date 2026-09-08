# UI4 image viewer

Run `img common/images` in Shell2, or enter `show common/images` at the img
prompt. PNG, JPG and JPEG files are sorted by name. Left/Up selects the
previous image; Right/Down selects the next; both ends wrap. Folder browsing
reuses one window and fits the complete image within it. Transparent pixels
are composited onto black. A single file still opens the native pan/resize
viewer. `list` reports open frames and `close all` closes them.

`img` uses the shared kernel media service for both PNG and JPEG. Its output
reports file-read time, the complete media-service decode/readback call, and
time through UI4 publish. Publish is submission, not proof of display scanout.

Enter `shell` to return the terminal to Shell2 while retaining the images.
Select the img Matrix slot and use `vmx_tui` to re-enter its prompt. `exit`
closes the viewer. UI4 Escape closes the selected image frame.

The gallery retains only its current decoded image; navigation reads and
decodes each selection, so measurements do not hide work behind a cache.
