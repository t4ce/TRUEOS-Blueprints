# Shell3 command sessions

`trueos::command_session` is the minimal transport contract for a dedicated
Shell3 session. Its launch request contains native script text, optional terminal
geometry, and whether stdin stays open. Tab characters and command text pass
through unchanged. There is no Unix executable/argument list, environment map,
or working-directory emulation. An empty script opens an interactive session.

A backend admits a session and returns an owned `Transport`. It supports:

- nonblocking reads with an output stream and explicit running/exit state;
- partial stdin writes and a separate stdin-close operation;
- terminal resizing, interruption, and termination;
- idempotent release of the exact session lifetime on drop.

The backend must reserve an isolated session, retain generation-safe ownership,
and drain output before reporting exit. A prompt is not an exit event, and a lost
transport is not success. If Shell3 provides one combined output stream, it uses
`Stdout`; `Stderr` is available only when the backend can distinguish it.

Codex's TRUEOS PTY layer adapts this contract to its existing command-session
handles via `install_shell3_backend` and `spawn_shell3_script`. The platform must
supply the backend implementation. The current Blueprint API does not yet expose
a kernel Shell3 command-session transport. This change does not invent syscalls or
revive the retired Shell2 frontend. Missing registration fails explicitly, and
Unix program/argv launch requests are rejected on TRUEOS.

The Codex Crossterm interface still uses the app's own terminal lease. Command
sessions do not borrow that lease or type commands into the visible Codex console.

## Existing API audit

| Interface | Status | Consumers |
| --- | --- | --- |
| `Shell2Frontend`, frontend constants, raw Shell2 write, and five ABI imports | Removed; no application consumers; kernel implementations are retired stubs | None |
| Terminal lease and surface snapshot | Preserved | Builtins: backup, appstore, os, commander, termdir, edit. Apps: solara, RbGB, pi_snake, qjs, lumen, badc |
| `shell2_print_line` compatibility name | Preserved | Generic printing and qjs reexport |
| `vcabi` | Preserved | Blueprint API allocation, randomness, tunnels, and other low-level services |

The separate x86-carrier repository still contains legacy adapter definitions;
it has not been changed by this cleanup.
