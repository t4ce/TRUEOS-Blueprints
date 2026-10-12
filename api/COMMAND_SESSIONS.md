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

Codex's TRUEOS PTY layer adapts this contract through `spawn_shell3_script`.
The live `KernelBackend` is installed automatically on TRUEOS; integrations may
install an alternative before the first launch. Local `exec_command` passes its
`cmd` text directly to this backend. Remote executors retain their shell behavior.

The versioned API queues a dedicated Shell3 terminal on the resident AP pool.
It uses the same keyboard decoder and VM console routing as SSH, without an SSH
connection. Initial script text receives a final Enter if it has no line ending;
subsequent input remains raw. Sessions persist across commands. Entering `exit`
at Shell3 ends a session; stdin closure alone does not cancel it.

Four additive ABI operations provide open, read, write, and control over the
existing VM call channel. Session operations check the authenticated VM and its
Matrix lifetime. Closing a session disposes of its own initial slot, never an
unrelated slot selected later with `§`. This requires a kernel containing the new
session API; rebuilding a Blueprint alone does not upgrade the running kernel.

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
