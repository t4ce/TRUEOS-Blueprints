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

The current hot-path gate also covers per-call `_ftol`, `rand`, `strtol`,
`TlsGetValue`, registry-open, critical-section-init, and synchronization-return
diagnostics. With `trace-api` disabled, their diagnostic-only guest reads,
string allocations, and formatting are skipped. Exact octal `strtol("0")`
calls use a two-byte parser shortcut; other inputs retain the general parser.

The idle message loop also gates empty `PeekMessageA`, successful
`GetThreadPriority`, `TlsSetValue`, and `ClipCursor` diagnostics behind
`trace-api`, including diagnostic-only guest-memory reads. Messages found,
failures, and signaled waits remain visible. Empty zero-timeout waits log the
first four polls and every 1024th poll thereafter, with a cumulative counter,
timeout, and caller address; `trace-api` restores every poll. This counter is
shared across the child wait callsite. Event polling, TLS writes, message
delivery, and scheduling are unchanged; this does not unblock offline IOCP.

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

`strncmp` and initial-C-locale `toupper` also run directly in the guest.
`strncmp` reads unsigned bytes up to the first mismatch, NUL, or count limit;
zero count reads neither pointer. `toupper` handles byte values and EOF locally,
and sends out-of-domain inputs to the existing Rust provider frontier with the
original provider id and stack. These calls avoid the provider/actor/carrier
round trip and do not increment host provider counters. Enable `host-strncmp`
or `host-toupper` to restore the corresponding Rust path and its diagnostics.
The tests `tests/test_native_strncmp.py` and `tests/test_native_toupper.py`
execute the emitted thunks on Linux IA32. The latter substitutes RET at the
fallback VMCALL to check its provider id, stack, and register state; actual
fallback dispatch still needs a carrier run.

`_strnicmp` now uses a native bounded byte comparison with the personality's
CP1252 folding table, including accented case pairs. It stops at the first
mismatch, NUL, or count limit instead of fetching both complete strings through
the host. `host-strnicmp` restores the Rust implementation; its per-call logging
is gated by `trace-api`.

`atoi`/`atol` handle 1–9 leading unsigned decimal digits natively after checking
that the full input is ASCII and NUL-terminated within 256 bytes. Suffixes such
as `;K1` are accepted. Other inputs return to the complete Rust parser, retaining
its sign, whitespace, overflow, and invalid-input handling. `host-decimal`
restores the Rust path for every call. Normal runs also omit the duplicate parse
that was used solely to format decimal-call diagnostics. Native calls are absent
from host provider counters. `tests/test_native_strnicmp.py` and
`tests/test_native_decimal.py` exercise the emitted helpers and ABI; the decimal
test substitutes RET for fallback VMCALL to inspect the restored dispatch state.

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

## Map discovery at startup

Before guest execution, WC3 selects `WARCRAFT3_MAP` files under the shared
`/common/Warcraft III/Maps` selector (resolved to `apps/common/Warcraft III/Maps`).
The catalog stays in `Wc3Session.maps`; paths are relative to that folder. The
normal log emits one `WC3 MAP CATALOG` summary; `trace-init` prints every path.
A missing Maps folder is reported and does not prevent startup. Selection I/O
errors are reported as initialization errors; depth or capacity truncation is
retained in the catalog and shown explicitly in the summary.

The reusable `async_fs::select_files(folder, ContentTypeId, max_depth)` API
accepts any registered native type, not filename globs. Depth 0 means immediate
files; WC3 uses depth 8. Results are sorted and carry `depth_limited` and
`truncated` flags. Selection has separate budgets of 65,536 matching files and
4,096 directories, rather than the interactive listing's 1,024-entry cap.
Stored types are authoritative. Old BLOB files can be recognized by reading at
most a 4 KiB signature prefix; they are not rewritten. Prefix probing does not
attempt whole-file UTF-8 classification and is not a full format validator.

The new inference identity detects the retail HM3W map preamble followed by an
MPQ header at byte 512. The `.w3m` and `.w3x` names describe the same container
identity; discovery does not claim that a TFT map is playable by the RoC guest.
Header reference: [Warcraft III map format specification](https://alanfox2000software.github.io/war3-diy/doc/w3x/index.html).

This adds the versioned `trueos_cabi_async_fs_select_files_start_v1` import;
update the kernel and Blueprint together. Discovery does not preload map
payloads or launch a selected map.

Window creation now runs the guest's synchronous creation callbacks after opening the UI4 frame.
The binary evidence, callback contract, regression test and next-run markers are in
[the window-creation audit](docs/window-creation-frontier.md).

### Runtime inspection

The WC3 Blueprint now imports Shell2's existing command-input ABI, which enables
command passthrough on its VM slot. After rebuilding/repacking WC3, enter commands
without a `vmx_` prefix:

```text
debug help
debug state
debug regs 2 3
debug stack 2 3 32
debug mem 2 0x00444480 256
debug object 2 0x5743280c
```

Numbers are decimal or `0x` hexadecimal. These commands only observe state.
`regs` reports the selected thread's last stopped registers. `stack` validates
ESP and the requested range against that thread's guest TIB bounds and labels
words as candidates, not a backtrace. `mem` reads only the selected guest address
space, with a 256-byte cap and explicit errors for incomplete reads. `object`
resolves the process handle through the session object table. `state` reports
queue counts, up to 64 live contexts and 32 processes/windows, with userdata and
pending paint/message information. It does not claim to sample rendered frames.

Commands are consumed at existing guest exits, at most once per 100 ms, checking
ordinary exits in groups of 32 and also checking existing preemption exits. No
new yield, event signal, guest write or blocking input read is introduced. If
execution never returns to the coordinator, a request cannot be serviced there.
Lines longer than 160 bytes are discarded in full. Replies use `WC3 DEBUG` records
on the normal Blueprint text/log path (subject to the existing `nolog` feature).
An already-running older pack cannot gain these commands without a relaunch.

`debug post PID HWND MESSAGE WPARAM LPARAM` is an explicit mutation for window
notification experiments. It queues one message for the normal guest message
pump; it does not synchronously invoke the window procedure or signal an event.
The owner PID and live window are checked. Only scalar SIZE, ACTIVATE, SETFOCUS,
KILLFOCUS, SHOWWINDOW, ACTIVATEAPP and NCACTIVATE notifications are accepted with
bounded payloads. Creation, teardown, pointer messages and input are rejected.
`WC3 DEBUG POST QUEUED` means queued, not delivered: inspect the subsequent
PeekMessage/DispatchMessage callback and state before trying another message.
Unhandled API semantics still stop at a frontier, including default processing
of activation messages; this command does not bypass those frontiers.

The current first experiment is WM_SIZE, using this run's recorded 2560x1440
client dimensions: `debug post 2 0x57434003 5 0 0x05a00a00`.
Addresses/handles are run-specific. See `docs/window-notification-probes.md`.
