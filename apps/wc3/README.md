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
- `trace-init`: per-initializer calls/returns, callback-table registrations and local import dumps.
- `trace-all`: all trace categories.

CRT exit-callback tables grow geometrically instead of reallocating and copying
the table on almost every append. Only capacity is reserved ahead: the guest
end pointer still advances by one callback, order is unchanged, and the old
allocation stays live until copying and pointer updates complete. If the larger
reservation cannot fit, allocation retries with the exact required size.
API argument frames are fetched in a single checked guest-memory read instead
of one host crossing per word. Initializer completion counts remain visible with `trace-init` disabled.

MSVCRT `memmove` imports now jump to a cdecl x86 helper in the child control
page. It copies directly with `REP MOVSB`, backwards for rightward overlap,
returns the destination, and preserves nonvolatile registers and incoming flags.
Zero-size and identical-pointer calls remain no-ops; wrapping ranges trap and
unmapped/protected accesses use the normal guest-fault path. This avoids a
provider exit, a temporary allocation, and guest-to-host-to-guest transfers per
move. Individual moves remain ordered: the observed 16-byte source / 44-byte
destination stride expansion cannot be replaced by one contiguous copy.

Enable `host-memmove` to restore the Rust provider and its per-call diagnostics.
Native moves are absent from host provider call counters/logs. The existing
callback-table relocation copy is separate and still uses host transfers.
`python3 apps/wc3/tests/test_native_memmove.py` executes the emitted helper and
import thunk in a freestanding i386 test executable on Linux.

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

### Simple CRT initializers in Rust

The default `_initterm` path recognizes current guest code consisting of up to
four direct jumps followed by a dword copy through EAX, an immediate dword
store, or RET. It computes the callback's effects directly in Rust and continues
the table in order. Inputs are read fresh; this is not a saved-state replay.
`CRT INITTERM COMPLETE` reports `callbacks` (total) and `rust_callbacks` (the
subset handled this way). The audited War3/Game images contain 590 candidate
callbacks; runtime guards determine the actual count.

Code must be in an executable PE section, operands in non-executable readable
PE data, and destinations in writable PE data within one page. Stack/control
aliases, unrecognized code, failed accesses, debug/step modes and additional
child threads use normal guest execution. The existing return-address stack
write is retained; EAX, other registers and flags follow the recognized x86
instructions. Enable `guest-initterm` to force all callbacks through the guest.
No event-pool or record-expansion replacement is included.

Validation: `cargo test -p wc3 --lib --offline` and
`python3 apps/wc3/tests/test_native_initterm.py`. The latter compares Rust results
with native i386 execution, including flags, preserved registers, stack balance,
source/destination aliasing, and zero through four jump wrappers.

### Event pool construction in Rust

For the audited War3.exe pool loop at `0x004029e0`, the Blueprint checks the
current guest code, substitutes one three-byte VM exit, and restores the
original instruction at the exit. If the running frame or code differs, the
original loop executes. The Rust path allocates the same 2,048 unnamed event
objects in order, with automatic reset in bank zero and manual reset in bank
one. It writes the handle and generation arrays at their original addresses,
including the 4 KiB gap between generation banks, and resumes at `0x00402a47`.
The following GetSystemInfo and dynamic-symbol work remains guest code.
`WC3 EVENT POOL RUST COMPLETE` reports use; `guest-event-pool` forces the
original loop for comparison. This feature does not need a saved checkpoint.

`python3 apps/wc3/tests/test_native_event_pool.py` executes the original pool
loop as i386 code with a deterministic CreateEvent stub and compares every
output byte and the final CPU state with the Rust table builder.

### Storm record expansion in Rust

Storm.dll's verified loop at `0x1501d5e0` expands compact 16-byte records
in place into 44-byte records. In the measured run it has 10,759 input
records, so 10,758 repeated moves. The Blueprint now reads the compact input
once, builds the expanded records in Rust (16 bytes from each input record
followed by 28 zero bytes), writes the output in one guest-memory operation,
and resumes at `0x1501d61e`. Storm still executes its special first-record
setup and later relocation work. The previous log's 21,516 moves came from
two runs of this 10,758-move loop.

The current Storm code is verified before installing a one-time VM trap. At
that trap, code, registers, stack locals, debug state, and the committed guest
allocation are checked; a mismatch restores the original instruction and
executes the guest loop. `WC3 RECORD EXPAND RUST COMPLETE` reports activation.
Enable `guest-record-expand` to keep the original loop for comparison. This
runs independently of the existing native `memmove` helper.

`python3 apps/wc3/tests/test_native_record_expand.py` executes the audited
Storm loop as i386 code and compares its output and ending CPU state to Rust
for 2, 3, 17, and 10,759 input records.
