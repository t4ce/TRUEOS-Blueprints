# Commander

TRUEOS terminal-to-VLayer control Blueprint.

## Intended location

Place this directory at:

`TRUEOS-Blueprints/buildins/commander`

The relative dependency paths in `Cargo.toml` are written for that location.

## What it does

- claims the current typed terminal lease
- enters Crossterm raw/alternate-screen mode
- enables keyboard, focus, mouse and any-motion terminal reporting
- allocates one VLayer `InputCombo` with source kind `Remote`
- binds one `VKeyboard` and one `VCursor`
- maps Crossterm keyboard events into `KeyboardControlCommand`
- maps terminal mouse cell coordinates across the current TRUEOS output
- maps click/drag/wheel into `MouseMotionCommand`
- exits on `Esc`, `Ctrl-Q`, or `Ctrl-]`
- restores terminal state, removes the combo and releases virtual-device capabilities on exit
- releases the terminal lease back to Shell2 before shutting down

## Deliberate scope

This is a Commander Blueprint, not a replacement terminal protocol and not a Texplo fork.
It uses the already-integrated Crossterm decoder as the terminal-side input parser and
the already-existing VLayer virtual devices as the TRUEOS-side control sink.

The existing keyboard-control service is clocked/mediated, so this first version inherits
that behavior for keyboard strokes. A later kernel-side refinement can expose a low-latency
Remote producer below the automation timing layer without changing Commander's conceptual
event mapping.

## Build note

This package mirrors the current standalone Blueprint app layout (`Cargo.toml` + `src/main.rs`).
It expects the TRUEOS-Blueprints repository's vendored Crossterm/Mio/Rustix stack.


## Narrow diagnostic build

This build adds no alternate input consumer and no kernel change.

On startup it submits one VCursor teleport to `(output_width/4, output_height/4)`.
That proves the VLayer half independently of terminal input.

Structured `commander` logs then record:
- each Crossterm Key/Mouse event
- mouse cell -> output coordinate translation
- each successful VKeyboard/VCursor submit
- resize events for an event-loop liveness reference

Interpretation:
- startup teleport does not move: investigate VLayer VMCall / mouse-control / UI4 ring
- startup teleport moves, but no `rx event=key|mouse`: investigate NetShell direct RX -> fd0 -> Mio/Crossterm
- `rx event=...` exists but no `submit ... result=ok`: Commander translation/submission error
- submit logs exist but UI4 does not react: investigate the VLayer/UI4 consumer after submit
