# Cut Tetris

Cut Tetris reverses the usual pressure: full rows continually enter from the
top, while standard tetrominoes remove four occupied cells at a time.

- Left/Right or A/D: move the cutter
- Up/Down or W/S: rotate clockwise/counter-clockwise
- Space/Enter: cut the lowest intact match
- R: restart
- Escape: exit

A cut is valid only when all four cells under the tetromino still contain
material. Surrounding blocks are allowed; producing holes is the point. A row
spawn ends the game when it would push any remaining block past the bottom.
