# Games

A small TRUEOS Blueprint with MicroGames Tetris, Cut Tetris, Gamie Minesweeper and
Tic-Tac-Toe, Sudokitty Sudoku, and cozy-chess in a UI4 frame. The playfield
uses the same retained cube patch mesh and kernel hull/domain shader contract
as Cubes Key 2. The shader's opaque RGB555 mode preserves MicroGames'
piece colors. Tetris rules and scoring come from MicroGames; Minesweeper
reveal, flag, clues, and win/loss rules come from Gamie, as do Tic-Tac-Toe turns
and win/draw rules. Sudokitty core generates Sudoku puzzles, protects clues,
tracks conflicts, and checks completion. Cozy-chess supplies legal chess moves,
turns, castling, promotions, and endgame status.
TRUEOS FontKernel renders the sidebar labels; CPU MicroFont stamps the
Minesweeper digits and markers into a retained UI4 sprite atlas. Mine glyphs
are native 1x size normally and 2x when the frame is maximized. The board layer clears to
50% alpha; the cube faces remain opaque. The camera fits the entire playfield,
and maximizing or restoring the UI4 window scales the board and text together.
Quarter-size hull cubes frame both the whole UI4 window and the playfield
with alternating cube and empty positions.
A procedural F warm-horizon soundscape plays continuously while Games is open,
including when a game ends or is paused.
Its stereo carrier sweeps from 75 to 125 Hz and back every 30 seconds;
the binaural difference changes from 2 to 6 to 10 Hz every three minutes.
Weightless chords swell between 70% and 85% each minute. Ocean hush rises
from 25% to 65% over ten seconds, holds for five, then falls over five.
Distant chimes use a 75% layer level and cycle through 60%, 70%, and 80%
note intensity every three notes; a chime has a one-third chance of a quick
repeat. Warmth is 95%, drift is 15%, and the tone setting is 100%. The mix
is peak bounded before PCM output. A Tokio audio task runs on a separate
TRUEOS native worker and keeps one second of PCM queued to absorb long
shader frames. Games retries the TRUEOS audio device if it is temporarily
unavailable and uses the main loop if no native worker lane is available.

Select the frame and press F1 for Tetris, F2 for Minesweeper, F3 for Tic-Tac-Toe,
F4 for Sudoku, F5 for chess, or F6 for Cut Tetris. F8 opens the game gallery. F9 opens the two hullshader render limits:
Up/Down chooses FULL or SEEDS, and Left/Right adjusts its budget. In Tetris,
Left/Right move, Up rotates, Down drops one row, and Space hard drops. In
Minesweeper, hovering selects a cube, left click reveals it, and right click
toggles its flag. Arrows also move the selection; Space/Enter reveals and F flags. In Tic-Tac-Toe, the first two cursors to select the Games frame claim P1 (X)
and P2 (O). Each player has a separate hover selection and can place a mark
with left click on their turn. Their paired HUT keyboards can use the arrows
to select a square and Space or Enter to play it. A departing cursor releases
its seat when a new selected cursor joins. In Sudoku, hover or use the
arrow keys to select a square, type 1–9 to fill it, and use 0, Backspace, or
Delete to erase. Given digits cannot be changed; conflicting entries appear
red. The digits themselves are made from hullshader cubes. Only F1–F6 switch
games; plain number keys remain available to each game. All games use P to pause and R
to restart; R generates a fresh Sudoku puzzle. In chess, click a piece and
then a highlighted legal destination. The flat chessboard is viewed at an angle;
A/D rotates it and W/S changes its elevation by 15 degrees. Arrows move the
square cursor; Space or Enter selects or plays. Pawn promotion automatically
chooses a queen. Escape closes the frame. The first Minesweeper reveal is safe, and
switching modes preserves each game.

Build locally from the TRUEOS-Blueprints root with
`TRUEOS_BLUEPRINT_SKIP_APPS_PUBLISH=1 cargo bp games`. The source depends on
the sibling MicroGames and TRUEOS-Picasso repos and the pinned t4ce/gamie
crate's Minesweeper and Tic-Tac-Toe modules, the pinned Sudokitty core
crate, and pinned cozy-chess.

The six Deco chess sculptures are stored in this Blueprint's
`assets/chess_{pawn,knight,bishop,rook,queen,king}.cubes` files and embedded in
the packaged app. `tools/export_chess_deco.cjs` reproduces the exports from
Cubes' chess rubric. Its Horse model serves the chess knight, and its armored
Knight model serves the bishop because the rubric has no Bishop entry. Each
file keeps the full authored solid with one neutral placeholder color. Games
uses a compact exposed-cube view at board scale, preserves relative piece
heights, and applies distinct side tints until the two custom materials arrive.
`GAMES_CHESS_ASSETS_DIR` can override the asset directory for build checks.

Cut Tetris uses the same 10×20 hullshader cube board as Tetris. Full rows enter
from the top; the cutter removes the lowest intact tetromino at the selected
column only if the remaining material stays connected to the top. Arrows or
A/D move and W/S or Up/Down rotate clockwise/counter-clockwise. Space or Enter
cuts; either restarts after game over. R restarts and P pauses. The highlighted
cubes show a valid cut when ready, and the marker below the field shows the
cutter columns. The sidebar shows the current and next cutters, score, cuts,
lives, and incoming-row progress. Rows begin six seconds apart and accelerate;
cuts share the original cooldown and ten cumulative bottom-block lives.
Switching games or opening the gallery suspends Cut Tetris and preserves it;
the gallery's fresh-game checkbox resets it.
