tboard 0.1.0
============

A tiny, direct-crossterm three-lane card board for TrueOS.

The whole board lives in RAM. There is no database, daemon, network service,
async runtime, widget framework, or persistence layer. The application uses the
same retained terminal-cell rendering approach as Texplo/tredb: draw a complete
Frame, then emit only changed terminal runs.

Run on TrueOS
-------------

Place the project in the usual app tree and run:

    cargo run

The default Cargo.toml expects the TrueOS app layout:

    ../../api
    ../../vendor/crossterm-0.29.0-trueos
    ../../vendor/mio-1.2.0
    ../../vendor/rustix-1.1.4-trueos
    ../../vendor/signal-hook-mio-0.2.5-trueos

Launch directives
-----------------

The optional vFile:launch content is:

    seed demo

or:

    seed empty

Native smoke test
-----------------

In a scratch copy, replace Cargo.toml with Cargo.native.toml and run cargo.
The native manifest uses crates.io crossterm 0.29.0 and disables TrueOS.

Controls
--------

- Up/Down or J/K: select cards in the active lane
- Left/Right or H/L: select a lane
- Shift+Left/Right: move the selected card
- [ and ], or Space: move a card left/right
- Enter or E: edit the title
- N: new card
- D: edit detail
- X or Delete: delete card
- Left mouse drag: move a card between lanes
- Mouse wheel: select previous/next card
- 0..9: run the matching menu entry
- Ctrl-Q or Esc: exit

The board is intentionally transient. "demo board" and "empty board" replace
the current RAM session after confirmation.
