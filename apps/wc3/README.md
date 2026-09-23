# Warcraft III Blueprint

This Blueprint owns the Windows XP compatibility semantics used by the
Warcraft III 1.00 launcher. It loads the original PE32 image, patches imports
to x86 `VMCALL` thunks, and services those traps in ordinary Rust userspace.

The only privileged boundary is `trueos::x86`: generic address-space,
context, memory and trap operations. The original kernel launcher remains a
temporary parity oracle and is not called by this app.

Run with the normal Blueprint workflow:

```text
!cargo bp wc3
run wc3
```

The launcher image is read from
`/common/Warcraft III/Warcraft III.exe` and must have SHA-256
`5a8cca727c719ae054adf8d15523a8e3099745225e2f4f9885f88774aa6f36d9`.

## Frontier iteration

Default builds also omit scan observers, execution-sample dumps, detailed SEH
traces, and selected repetitive API call/return traces. The trace gates run
before argument formatting and diagnostic guest-memory reads. Guest code,
SEH dispatch, provider validation, checkpoint validation, exception summaries,
single-step heartbeats, and frontier/error reporting still run. This reduces
instrumentation overhead; it does not skip guest instructions or restore a
whole session.

Re-enable only the diagnostics needed via Cargo features:

- `trace-scan`: scan/table progress observers and execution samples.
- `trace-seh`: SEH dispatch/return details, register/code dumps and step transitions.
- `trace-api`: the gated API call/return records.
- `trace-all`: all three categories (the previous tracing behavior).

For the normal Blueprint workflow, add the desired features to `default = []`
in this app's `Cargo.toml`, then rebuild. For host checks, pass
`--features trace-seh` (or another category) to Cargo. Diagnostic features do
not change the table checkpoint format or its restore preconditions.

## Audited loop checkpoints

Two additional execution regions now learn checkpoints on their first normal
pass and jump to the saved exit on a matching subsequent launch:

| Region | Entry | Exit (before the next operation) |
| --- | --- | --- |
| Decrypt scan | `0x0045af54` | `0x0045b005` |
| Dword checksum | `0x0045b0b1` | `0x0045b0e1` |

These are guest execution skips, including the repeated SEH dispatches, not
logging switches. The first run still executes the loops. Watch for
`LOOP CHECKPOINT CREATED`, then `LOOP CHECKPOINT HIT ... skipped_steps=...`.
`BYPASS` or `DISCARD` means normal execution continues. The existing table-fill
checkpoint remains separate. `replay-loops` disables the two new caches for
comparison or investigation; it does not disable the table-fill cache.

The loop and complete exception-handler bytes must match their audited hashes.
Entry guards constrain all indirect buffers to captured image/heap ranges and
the handler's instruction range to the loop. A checkpoint checks the complete
CPU/debug/extended state and exact page set/hashes before writing anything.
Captured memory includes the full image, stack (including SEH scratch), TEB,
process-data page and committed CRT/Windows heaps. The decrypt scan's heap XOR
and fixed `0x00470990` data writes, checksum accumulator/index writes, and the
handler's stage/source/checksum/global writes are all inside those ranges.
The checksum's backward increment branch at `0x0045b0a2` is part of its audited
code even though its first entry is `0x0045b0b1`.

Any other context execution, provider call, non-debug exception, or exit from
the audited instruction region discards the capture. Storage/decode failures
and input mismatches fall back to execution. A failure after restore writes
begin aborts rather than running partially restored state. A code change in the
checkpoint/dispatch/SEH source invalidates the new caches. The codec preserves
the existing table-fill format and rejects unsorted/duplicate/unaligned pages.

The later loop around `0x0046160c–0x0046162c` remains uncached: those bytes are
decrypted at runtime, so the on-disk disassembly is not enough to audit it.
The `sintfnt.dll` / `Corrupt Data!` frontier is unchanged.
