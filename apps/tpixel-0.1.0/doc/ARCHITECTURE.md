# tpixel architecture

```text
crossterm events
      |
      v
App state + 96×64 bool canvas
      |
      v
Braille packing (2×4 pixels per cell)
      |
      v
retained Frame -> changed terminal runs
```

The canvas is a plain `Vec<bool>`. Undo and redo store bounded full-canvas
snapshots; at this size that keeps the implementation obvious and the memory
cost small. A mouse stroke takes one snapshot when the button goes down, not on
every drag event.

No platform graphics API is required. TrueOS is used only for the optional
launch vFile; crossterm provides the surface and events.
