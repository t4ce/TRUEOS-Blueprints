# UI4 image viewer

Run `img apps/common/images` in Shell2, or select the img Matrix slot and enter
`show apps/common/images` on the normal Shell2 prompt row. The Blueprint never
claims a terminal TUI. Inferred PNG and JPEG files are sorted by name. Left/Up selects the
previous image; Right/Down selects the next; both ends wrap. Folder browsing
reuses one window and fits the complete image within it. Transparent pixels
are composited onto black. A single file still opens the native pan/resize
viewer. Mouse-wheel up/down zooms around the cursor in 10-percentage-point
steps, bounded to 10%–500% of native size. Middle-button dragging pans the
zoomed image. Resizing preserves manual zoom; selecting another gallery image
returns to fit-to-window. Multiple file arguments open one fixed frame per image. `list` reports
each open frame and `close all` closes them.

Launching without a source opens the first inferred image in
`apps/common/images`; if that folder has no supported image, img opens a
640×480 neutral-gray frame instead.

`img` uses the shared kernel media service for both PNG and JPEG. Its output
reports file-read time, the complete media-service decode/readback call, and
time through UI4 publish. Publish is submission, not proof of display scanout.

`list`, `show`, `convert`, `close all`, and `exit` are VMX-minishell commands entered on
the persistent Shell2 prompt row. UI4 Escape closes the selected image frame.

The gallery retains only its current decoded image; navigation reads and
decodes each selection, so measurements do not hide work behind a cache.

Projection regression tests can run on the host:

```sh
printf '#[path = "%s/buildins/img/src/view.rs"] mod view;\n' "$PWD" > /tmp/img-view-tests.rs
rustc --edition 2024 --test /tmp/img-view-tests.rs -o /tmp/img-view-tests
/tmp/img-view-tests
```

`img kernel:logo` fits the complete embedded logo to the output, scaling up or
down proportionally. Its centered UI4 frame hugs the image so the unused
screen area exposes the display background color. It stays in fit mode after
resizing unless manually zoomed. This uses the regular viewer and frame lifecycle.

`convert` saves the selected UI4 frame's full decoded image beside its source:
`photo.png` becomes `photo.jpg`, and `photo.jpg` / `photo.jpeg` becomes
`photo.png`. The decoded format determines the conversion, and the write sets
TRUEOSFS's native JPEG/PNG content identity so inferred gallery listings recognize
it immediately. JPEG uses quality 90 and composites transparency onto black.
The source and displayed frame stay unchanged; an existing destination is replaced.

With one open frame, selection is optional. With multiple frames, exactly one
must be selected in UI4. No frames, no selection, ambiguous selections, the gray
placeholder, and kernel images do nothing. A mislabeled source whose output path
would equal its input is also left untouched. Gallery conversion uses the current
image; reopen the folder to include newly saved siblings in its listing.

Run conversion codec roundtrip and selection tests with
`python3 tools/test_img_convert.py` from the Blueprint repository.
