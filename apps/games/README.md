# Games

A small TRUEOS Blueprint with MicroGames Tetris and Gamie Minesweeper in a
UI4 frame. The playfield
uses the same retained cube patch mesh and kernel hull/domain shader contract
as Cubes Key 2. The shader's opaque RGB555 mode preserves MicroGames'
piece colors. Tetris rules and scoring come from MicroGames; Minesweeper
reveal, flag, clues, and win/loss rules come from Gamie.
TRUEOS FontKernel renders the foreground labels. The board layer clears to
50% alpha; the cube faces remain opaque. The camera fits the entire playfield,
and maximizing or restoring the UI4 window scales the board and text together.
Quarter-size hull cubes frame both the whole UI4 window and the playfield.
A procedural F warm-horizon soundscape plays during the game and pauses with P or
game over. Its stereo carrier sweeps from 75 to 125 Hz and back every 30 seconds;
the binaural difference changes from 2 to 6 to 10 Hz every three minutes.
Weightless chords swell between 70% and 85% each minute. Ocean hush rises
from 25% to 65% over ten seconds, holds for five, then falls over five.
Distant chimes use a 75% layer level and cycle through 60%, 70%, and 80%
note intensity every three notes; a chime has a one-third chance of a quick
repeat. Warmth is 95%, drift is 15%, and the tone setting is 100%. The mix
is peak bounded before PCM output. Games retries the TRUEOS audio device if
it is temporarily unavailable.

Select the frame and press 1 for Tetris or 2 for Minesweeper. In Tetris,
Left/Right move, Up rotates, Down drops one row, and Space hard drops. In
Minesweeper, click a cube to reveal it; right click opens a cell menu with
flag/unflag and reveal. Arrows move the selection, Space/Enter reveals, and F
flags. Both games use P to pause and R to restart. Escape closes the frame.
The first Minesweeper reveal is safe, and switching modes preserves each game.

Build locally from the TRUEOS-Blueprints root with
`TRUEOS_BLUEPRINT_SKIP_APPS_PUBLISH=1 cargo bp games`. The source depends on
the sibling MicroGames and TRUEOS-Picasso repos and the pinned t4ce/gamie
Minesweeper crate.
