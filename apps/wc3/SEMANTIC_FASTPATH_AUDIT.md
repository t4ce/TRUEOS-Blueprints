# Whole-operation Rust candidates

Audit date: 2026-09-24. Candidate 2 below (the simple initializer templates, option 1 in the user
summary) is now implemented with runtime guards in `src/initterm.rs` and
`advance_child_initterm`. The other candidates remain assessments. No physical
timing measurement has been made.

Evidence: the last War3 initializer sequence in
`TRUEOS/bld/baremetal-logs/LatestOfThree.logs` (starting at line 22414), existing
initializer/provider code, and PE images downloaded read-only from the rig's
discovered TRUEOSFS root. The checked-in tools installation is a different
version and must not supply signatures for these paths.

Downloaded image SHA-256:

- War3.exe: `e3789bc11da75e68efb8ed832d9160a543fcb8815ad192fb98f0f3c8e70c8124`
- Game.dll: `c8e21c031e52c06a91c0b110b1d5d6f978112b395da7d05f029120d7b8219b42`
  (also matches the logged LoadLibrary image digest).

## 1. Construct the event pools in Rust

The War3 `_initterm` interval contains 2,054 CreateEvent operations. Disassembly
identifies 2,048 of these as a single two-bank pool construction in
`0x004029a0..0x00402a47`. Each bank has 1,024 unnamed, initially unsignaled events;
bank zero uses automatic reset and bank one manual reset. The four CreateEventA
arguments at `0x004029e0` establish these properties.

The operation writes:

- 2,048 event handles consecutively from `0x004570b8`.
- 1,024 generation values per bank, at `0x004590b8` and `0x0045b0b8`, each
  `(index + 1) * 0x00200000` with wrapping 32-bit arithmetic.
- Per-bank fields at `0x004570ac`, `0x0045709c`, and `0x004570a4` (plus four
  bytes for bank one), respectively `0x7ff`, `0x3ff`, and `0x400`.

A semantic Rust implementation would call the existing session event allocator
in the original order, then write the same tables. It would eliminate 2,048
individual provider round trips. It must retain distinct real event objects,
reset behavior, handle allocation order, failure results, and thread last-error
effects. Repeating a recorded list of handles or returning one shared event is
incorrect. Lazy creation would change handle ordering and failure timing.

The wrapper `0x00402920` performs locking and a reference-count transition;
keep it. Resume at `0x00402a47` with the loop's exact stack/register/flag state.
The tail calls GetSystemInfo, derives a processor-count-dependent value, and
calls `0x0042f500`, which performs dynamic library/symbol resolution. Keep that
tail live: freezing today's missing-symbol outcome would limit future progress.
This candidate needs an interception boundary and a full state-equivalence test
before activation; provider logs alone do not prove its complete state.

## 2. Evaluate simple initializer templates in Rust

Walking the actual initializer tables and following up to four direct E9 jumps
finds these exact terminal instruction shapes:

| Shape | War3 | Game |
|---|---:|---:|
| `mov eax,[source]; mov [destination],eax; ret` | 53 | 343 |
| `mov dword ptr [destination],immediate; ret` | 0 | 185 |
| `ret` | 0 | 9 |
| Total | 53 | 537 |

For example, War3 callback `0x00401050` jumps to `0x00401060`, reads
`0x0044b7ac` into EAX and writes it to `0x00457040`. These are 590 whole
callbacks whose computation has a very small Rust description. Runtime values
must be read fresh; do not cache yesterday's output. The remaining callbacks
are unclassified, not assumed unsafe or unnecessary.

Use `advance_child_initterm` as the boundary. Verify current guest instruction
bytes (including jumps), preserve callback order, perform the normal callback
return-address stack write, and reproduce EAX for the copy form. MOV and RET
do not change arithmetic flags. Read/write failures must use the original
execution/fault path without partial fast-path mutations. Debug stepping,
execution/data breakpoints, executable-page permissions, and stack aliases
need explicit handling or fallback. A successful fast path can continue the
initializer loop without scheduling another guest callback.

This is the lowest-complexity first implementation. Differential tests should
compare memory, registers, flags, and fault behavior against the emitted x86,
including overlap with stack/code and changed input values. There are 4,130
callbacks across Storm, War3 and Game in this trace; this count does not measure
how much wall-clock time the 590 simple ones consume.

## 3. Replace the record-expansion operation

The earlier measured move sequence has 21,516 copies of 16 bytes, source stride
16 and destination stride 44. One contiguous memmove cannot express it. A Rust
record-expansion operation can: preserve the 16-byte payload copies and the
surrounding constructor's writes into the other 28 bytes of each record.

The native memmove helper already removes per-copy provider exits. A whole
operation replacement would additionally remove the guest loop and surrounding
per-record work. It requires identifying and disassembling the caller, proving
the complete 44-byte layout, preserving alias/order behavior, and checking
allocation, registration and failure effects. The move trace alone does not
establish that contract. Keep this behind the first two candidates.

## Activation rule

Recognize a verified operation and compute its current effects in Rust. On a
signature or precondition mismatch, run the original guest code before making
any fast-path changes. Compare the reference and replacement from identical
input states; do not infer equivalence merely from reaching the same frontier.
Keep a feature switch for reference execution and aggregate fast-path counts.
Only the simple initializer templates have been enabled; `guest-initterm`
restores reference execution. Event pools and record expansion are unchanged.
