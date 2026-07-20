# Cut Tetris

Cut Tetris reverses the usual pressure: full rows continually enter from the
top, while standard tetrominoes remove four occupied cells at a time.

- Left/Right or A/D: move the cutter
- Up/Down or W/S: rotate clockwise/counter-clockwise
- Space/Enter: cut the lowest intact match
- R: restart
- Escape: exit

A cut is valid only when all four cells under the tetromino still contain
material and every remaining block stays connected to the top-fed mass.
Surrounding blocks are allowed; producing holes is the point, but orphaned
floating islands are illegal. Rows initially spawn every six seconds and speed
up as pressure grows. Cuts are rate-limited to one step of the incoming-row bar
(600 ms initially, accelerating with the rows). A row spawn ends the game when
the cumulative number of blocks that have reached the bottom row reaches the
board width. Every newly arriving bottom block is counted, even when one tick
lands several blocks in different columns; the ten-wide board therefore grants
ten lives in total.
