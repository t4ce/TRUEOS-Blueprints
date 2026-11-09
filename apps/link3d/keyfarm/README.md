# Keyfarm

Record physical key presses against the **62 keycap meshes** in `Keyboard k70 mini pro.glb`.
This is a Linux host tool, independent of the running TRUEOS system. No kernel or
Blueprint build/deploy is needed to record. Link3D's build consumes the audited map
to place flat A–Z and 0–9 legends on the corresponding keycap meshes.

From this directory:

```sh
python3 record.py --list
python3 record.py --device /dev/input/eventN --layout de
```

Replace `eventN` with the listed physical keyboard. The tool detects GNOME's XKB
layout when run as your desktop user; `--layout de` makes the choice explicit.
Use `--layout us`, etc., for a different layout; `--variant` selects an XKB variant.
If raw input access requires sudo, it prints the exact command with these choices
preserved. Python 3 and the system `libxkbcommon` are sufficient; no pip packages.

After the countdown, tap each key once, **left to right, row by row, starting at
Esc**. Release each key before the next. There are five rows: **14, 14, 13, 13, 8**
positions. Record the tall ISO **Enter at the end of the Tab/Q row**, and only there.
The fourth row includes the extra key beside left Shift. The bottom row is Ctrl,
Super, Alt, Space, then the four remaining keys. Use `python3 record.py --plan` to
inspect the complete mesh order.

- **Space skips the current position**, except at bottom-row position 4, where it
  records the actual spacebar. Use it for Fn or another key that sends no event.
- Shift, Ctrl, Alt, Super, Caps and shortcuts are captured as raw keys. The exclusive
  device grab prevents them reaching desktop shortcuts during recording. Holds and
  repeats do not create extra entries. A normal tap of Esc records Esc.
- **Hold Esc for two seconds to pause** without consuming a position. Run the same
  command again to resume. The device grab is released on exit. Ctrl+C from this
  captured keyboard is a key press, so use the Esc hold to stop.
- To correct a mistake, pause, then run `python3 record.py --layout de --undo N` to
  remove the last N entries (including skips). Run the capture command to resume.
- To make another mapping without replacing this one, use `--output other.json`.

Each press saves `mapping.json` atomically and updates a tab-separated `mapping.txt`.
The JSON records model SHA-256, GLB node/mesh ID, physical row/column, raw Linux key
code, the device scan code when supplied (usually a USB HID page/usage), and XKB
legends for unshifted/Shift/AltGr levels. Modifier taps do not change the legend
lookup, so a previous Caps or Shift press cannot contaminate later letters.
Skipped positions remain explicit holes, rather than shifting the remaining keys.
Special keys get readable labels; their eventual font symbols can be chosen later.

This maps positions and the selected layout's legends. Hardware-only Fn layers and
art printed on the keycaps cannot be discovered from a normal key press.

## Recorded K70 Mini map

`mapping.json` and `mapping.txt` contain the audited German-layout capture from the
physical K70 Mini. `capture-original.txt` preserves the supplied recording before
correction. The map covers all 62 keycap meshes; its bottom row is:

```text
mesh 20   21     22   23     24     25  26    27
     Ctrl Super  Alt  Space  AltGr  Fn  Menu  Ctrl
```

Fn did not emit a standalone event, so Menu and right Ctrl originally occupied the
preceding positions, followed by an extra right Ctrl press. The audited map inserts
Fn at slot 60 / mesh 25, moves Menu to slot 61 / mesh 26 and right Ctrl to slot 62 /
mesh 27, and drops the duplicate Ctrl. Slots 1–59 retain the captured mapping.
This order was checked against [Corsair's keyboard diagram](https://www.corsair.com/uk/en/explorer/gamer/keyboards/k70-pro-mini-wireless-keyboard-shortcut-guide/).
The JSON's `audit` field records provenance and corrections.

Fn has status `unreported` and label `Fn`, with no fabricated Linux key code or HID
usage. It can receive a visible legend even though it sends no input event. All 61
captured input keys retain their scan codes and verified XKB legend levels. These
files supply Link3D's capital-letter and digit legend placement. Use a different
`--output` filename for new recordings to preserve this reference map.
