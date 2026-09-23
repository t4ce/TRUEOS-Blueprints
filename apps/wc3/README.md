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

Default builds omit scan observers, execution-sample dumps, detailed SEH
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

Further execution acceleration has two separate paths: verified checkpoints
for additional pure regions, or a complete session snapshot. A pure-region
checkpoint must cover all guest inputs and writes, CPU state, and any host
side effects; matching an instruction address or having reached it before is
insufficient. A session snapshot must additionally restore the XP process,
threads, handles, scheduler and external resources. Neither is implemented
by these diagnostic switches.
