# txt

`txt` is the Blueprint Ratatui text editor used by the shell2 `txt [FILE]`
command. It deliberately has no TTSTT, microphone, subprocess, or network
capabilities; the Blueprint only needs terminal and filesystem access.

Keys:

- `Ctrl-S`: save
- `Ctrl-Q`: quit (press twice when the buffer is modified)
- arrows, Home/End, PageUp/PageDown: move
- Backspace/Delete/Enter/Tab: edit
- click or drag: place the cursor or select text
- `Alt` + drag: rectangular selection
- mouse wheel: scroll
