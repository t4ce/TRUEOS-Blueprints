# Ruler

A regular Blueprint with a translucent gray straight ruler and a circular angle
tool. Graduations use the same physical display metrics as Grid: EDID dimensions
on a physical output, or the existing 96 DPI convention on a virtual output. The
tool never invents a physical calibration when that information is unavailable.
Text stays upright while the instrument rotates; metric and inch scales extend
in both directions from a movable zero.

Build from the `TRUEOS-Blueprints` repository:

```sh
TRUEOS_BLUEPRINT_SKIP_APPS_PUBLISH=1 cargo bp ruler
```

Launch `ruler` through the normal Blueprint launcher. Use the usual UI4 frame
drag to move the instrument. The transparent surface still has UI4's rectangular
input bounds: transparent pixels do not pass clicks through to other apps.

| Control | Action |
| --- | --- |
| `cm` / `mm` / `in` chip | Cycle the main scale's units; the opposite edge keeps the other scale. |
| `π` chip | Switch to the circular angle tool. |
| Angle chip | Return the straight ruler to zero rotation. |
| Right round grip + primary drag | Rotate the instrument. Hold Shift to snap to 15°. |
| A graduation | Move zero to the clicked position, including negative distances to its left. |
| Center spine | Return zero to the center. |
| Mouse wheel | Change straight-ruler length or circular radius. Graduations keep their physical spacing. |
| Circular ring / ray + primary drag | Set the measured angle relative to the reference ray. Hold Shift for 15° steps. |
| Circular center measurement | Switch between degrees and multiples of π. |
| Circular radius label | Cycle radius units. |
| `RULER` | Return to the straight ruler. |

Keyboard shortcuts: `U` cycles units; `M` or `P` switches instrument; `A` changes
degrees/π; `Home` resets rotation, zero, and the measured angle; left/right arrows
rotate; `+`/`-` change length or radius. Shift enables angle snapping.

The display's independent horizontal and vertical physical scales are applied
after rotation, so the markings retain their distances even when the reported
pixel density differs between axes. Length and radius are trimmed to the display
bounds without stretching the scale.
Docking centers the instrument inside the requested frame, leaving transparent
margins instead of stretching its measurements.

Generate a standalone SVG preview on the development host:

```sh
cargo run -p ruler --example preview -- /tmp/trueos-ruler-preview.svg
```

The filename is optional and defaults to `/tmp/trueos-ruler-preview.svg`. Open the
SVG in a browser or image viewer. It renders the exact shared scene geometry at
a 96 DPI reference, with horizontal, 35° rotated, and circular π views over a
grid to show the transparency. It is a design preview, not a hardware screenshot
or a physical-scale guarantee for the screen displaying the SVG.
