# Native Code Mode probe

This optional probe runs the production QuickJS provider and Code Mode session
protocol. Its default cases exercise ES modules and top-level await, nested
delegated tools with computed JSON arguments, session store/load, and cooperative
interruption and cleanup of an infinite loop. It does not use a model or remote
JavaScript host.

Before runtime construction, native execution brackets the actual C
`clock_gettime(CLOCK_MONOTONIC)` call with platform monotonic reads and requires
the C result to lie inside that bracket. A separate standard condvar test waits
for 2 ms; its watchdog uses platform time and nonblocking channel reads, with a
1-second bound independent of Tokio and condvar timed parking.

It runs the production multithreaded Tokio scheduler first, with one worker and
all drivers enabled, then four workers, then a separate current-thread control. Each
profile gets its own runtime, session and 60-second watchdog. Native `gri` runs
once in the first profile so a failure in the current-thread control does not
prevent observing the production execution path.

A no-JavaScript control exercises the same bridge independently: each of 64
native threads submits two awaited 2 ms Tokio timers through `Handle::spawn`,
receives results through a standard channel with 1 ms timed waits, then notifies
the parent through a oneshot. It runs after the production profile's JavaScript
and `gri`, and before JavaScript in the current-thread control. Both host and
native current-thread controls intentionally enable time only, preserving the
no-I/O timed-parking path being investigated. Cancellation aborts and joins
pending async tasks, with bounded worker cleanup and runtime shutdown.

Each bridge control is preceded by 64 direct sequential 2 ms Tokio sleeps,
without JavaScript, worker threads or channels. This separates timer driver and
wheel progress from cross-thread bridge progress. Only aggregate results and
watchdog failure counters are printed.

It then runs 64 quiet sequential cells with two real awaited delegate calls per
cell. Each delegate includes a Tokio timer; each cell computes 42 from the first
call's returned data. The probe verifies 128 calls and 64 cleanup callbacks and
reports aggregate elapsed time and the slowest cell. There are no per-cell
provider diagnostics to perturb the scheduling being checked.

On watchdog failure only, it also reports quiet cells completed, delegated calls
entered, delegate timers returned, and cells closed. The delegate and counters
remain owned outside the timed future so timeout cancellation preserves this
evidence. Quiet cells update one relaxed progress counter without printing.

Host baseline:

```sh
cargo run --manifest-path probes/code_mode_probe/Cargo.toml
```

Native build and publish to the existing probe Apps destination:

```sh
probes/code_mode_probe/build_publish.sh
```

On TRUEOS, launching `code_mode_probe` additionally authorizes one unchanged
`gri` script through the production Shell3 `KernelBackend`, as required for this
native vertical slice. Ensure the owned test instance has the `gri` alias mapped
to `gridpaper`; the embedded default alias is named `grid`. Shell3 can launch the
probe directly with `§§code_mode_probe`.

The native delegate accepts only that exact script. It captures output and the
actual numeric exit status for up to 30 seconds. A persistent session reports
`running_at_deadline` with a null exit code; this is not an asserted successful
command exit. Confirming Gridpaper UI4 appeared requires observing the native
display. The host default performs only the controlled protocol cases; host
`--gri` explicitly refuses native execution.

Native result logs include only session state, actual exit status (or null), and
captured byte count. Captured terminal escape sequences are not printed to the
probe log.

The probe is isolated from the main Blueprint Cargo workspace because Codex's
protocol build dependency and unrelated Blueprint probes pin incompatible patch
versions. Its patches mirror the protocol workspace contracts. It is registered
only in `probes.json`, never in default applications or startup.

The immutable control package
`a6eb684a0d82b9dd441eb7c9ef1cb94fe16dc197eb400faf241c4935d56bc633`
was run unchanged on two owned QEMU kernels. The captured
[kernel6 results](validation/native-kernel6-controls.json) passed both
multithreaded profiles, but the current-thread JavaScript case timed out after
three cells; its direct and no-JavaScript controls passed. The
[kernel8 results](validation/native-kernel8-controls.json) passed all three
profiles. Quiet JavaScript cells took 622 ms with one worker, 784 ms with four
workers, and 683 ms in the current-thread profile, each with 128 delegate calls
and 64 closures. Kernel8 combines timer-waker deduplication, C clock alignment,
and eager environment initialization fixes, so this comparison does not isolate
one repair as the sole cause. The independently reproduced duplicate-waker
queue-exhaustion defect has a separate production-driver regression test.

Earlier native `gri` runs reached the real Shell3 command session and remained
running at the 30-second capture deadline while Appstore's cold Gridpaper fetch
was still in progress. Gridpaper UI4 visibility is unproven. These results prove
the native JavaScript/delegate/session route and its measured protocol cases;
they do not establish a completed application launch.

The final package `4358aa75` also passed the C/platform clock bracket and the
independently bounded std condvar test on kernel9. Its three quiet profiles
completed in 541/765/703 ms, each with 128 delegate calls and 64 closures.
See [the final native evidence](validation/native-kernel9-preflight.json).
These are controlled provider measurements; authenticated full-Codex turn
latency remains unmeasured on the updated physical rig.
