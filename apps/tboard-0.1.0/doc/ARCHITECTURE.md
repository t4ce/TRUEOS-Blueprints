# tboard architecture

`tboard` is deliberately one process and one terminal surface.

```text
crossterm events
      |
      v
App + Board model
      |
      v
retained Frame -> changed terminal runs
```

The board model owns plain Rust strings and lane assignments. UI state keeps
only card IDs, so edits and deletes do not create borrowed/self-referential
state. Input and confirmation modals are ordinary application state.

Mouse hit regions are rebuilt with each frame. A card drag changes only its lane
when the button is released; no hidden service or filesystem operation exists.
