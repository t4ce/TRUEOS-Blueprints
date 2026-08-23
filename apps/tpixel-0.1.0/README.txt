tpixel 0.1.0
============

A tiny monochrome pixel editor for TrueOS. Unicode Braille gives each terminal
cell a 2×4 logical pixel block, so the fixed 96×64 canvas contains 6,144 pixels
without a GUI framework.

Everything lives in the process. There is no file format, image library, daemon,
network service, async runtime, or Ratatui dependency. The retained framebuffer
emits only changed terminal runs.

Run on TrueOS
-------------

    cargo run

The default Cargo.toml expects the usual TrueOS app tree:

    ../../api
    ../../vendor/crossterm-0.29.0-trueos
    ../../vendor/mio-1.2.0
    ../../vendor/rustix-1.1.4-trueos
    ../../vendor/signal-hook-mio-0.2.5-trueos

Launch directives
-----------------

Use either:

    seed demo

or:

    seed empty

in vFile:launch. Command-line aliases are --demo and --empty.

Native smoke test
-----------------

In a scratch copy, replace Cargo.toml with Cargo.native.toml. The native manifest
uses crates.io crossterm 0.29.0 and disables TrueOS.

Controls
--------

- Arrow keys: move the exact logical pixel cursor
- Space: apply current tool
- P / E / T: pencil / eraser / toggle
- [ / ] or mouse wheel: change brush from 1×1 through 4×4
- Z / Y: undo / redo, with up to 64 snapshots
- I / C / R: invert / clear / demo art
- Left mouse drag: draw
- Right mouse drag: erase
- Middle mouse drag: pan
- Ctrl+arrows: pan by eight logical pixels
- Home: center the viewport
- 0..9: numbered menu commands
- ? or F1: help
- Esc or Ctrl-Q: exit

Mouse clicks preserve the cursor's sub-cell Braille position. Move the cursor by
one logical pixel with the arrow keys, then mouse-draw at that same dot position
across terminal cells.
