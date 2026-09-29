# Games

A small TRUEOS Blueprint with MicroGames Tetris in a UI4 frame. The playfield
uses the same retained cube patch mesh and kernel hull/domain shader contract
as Cubes Key 2. The shader's opaque RGB555 mode preserves MicroGames'
piece colors; all rules, score, and level progression come from MicroGames.
TRUEOS FontKernel renders the foreground labels.

Select the frame, then use Left/Right to move, Up to rotate, Down to drop one
row, Space to hard drop, P to pause, and R to restart. Escape closes the frame.

Build locally from the TRUEOS-Blueprints root with
`TRUEOS_BLUEPRINT_SKIP_APPS_PUBLISH=1 cargo bp games`. The source depends on
the sibling MicroGames and TRUEOS-Picasso repos.
