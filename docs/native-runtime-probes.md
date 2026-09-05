# Native runtime probe acceptance

Baseline integration uses two native lanes, each constructing and dropping its
own current-thread runtime. `tokio_mrt` initially uses one wave, one task and one
round per lane; SQLite initially uses one transaction with one row per lane.
Apply the separate follow-up patch to increase this bounded workload.

Run focused baselines (`tokio_rt`, `tokio_fs`, `tokio_net`, `framework_stack`)
before the native probes. `condvar`, `wls`, `cross`, `tokio_mrt`, and
`rusqlite_multirt` must report their positive native result; merely starting the
Blueprint is not acceptance. Insufficient native capacity is a failure to run
the multi-lane witness, not a sequential fallback or a pass.

Verify std Builder::spawn returns Unsupported; current-thread runtime creation,
normal async tasks, synchronization, timers and runtime drop still work. WLS
must be distinct concurrently and stable through two build/drop cycles on the
same worker. Prior worker-local values may persist on a reused slot.

Exercise partial admission failure while another Blueprint holds capacity;
accepted workers must be released and drained. Exercise stop/pause with accepted
finite work still pending: resources and the VM slot must remain retained until
work completes. A job that does not terminate keeps teardown pending; retain an
external watchdog/log capture because a wedged executor cannot run its timeout.
Never treat a timeout or a dropped handle as proof of cancellation.

For networking, include numeric IPv4/IPv6, a resolvable hostname and a failing
hostname, and observe that the coordinating runtime continues making progress.
Player's startup sentinel failure must not block its UI. Superseedr validation
covers its existing TRUEOS shell and tracker commands, not a new peer engine.

The source handoff includes installer fixtures, source ABI/export checks, and
Rust regression cases. The Rust cases still need compilation/execution with the
pinned toolchain; the patches do not claim a successful build or rig run.


## Light-stress follow-up

- `tokio_mrt`: two native runtime lanes plus the coordinator, two waves,
  16 tasks per lane and 32 rounds. Bounded cross-runtime channels carry exactly
  1,024 steps per wave. Expected checksums are 524,800 and 1,573,376. Lane slots
  must stay distinct; each runtime is dropped before the next wave.
- `rusqlite_multirt`: two runtime lanes, 16 transaction batches with 16 rows each
  (512 operations total). Each lane owns its SQLite connection and image. Verify
  every serialized row, aggregate checksum, and the final persisted summary.
  Readiness confirms overlap; `max_active` is diagnostic rather than a
  scheduling-dependent pass condition.
- `tokio_stack`: two lane-owned network runtimes, each with its own ephemeral
  loopback server, four clients, and eight requests per client (64 replies).
  Every reply must match its payload/generation; both servers must shut down and
  join. Tracing is installed separately on each native lane.

Deadlines are ten seconds for the runtime coordination stage, five seconds for
WLS/condvar completion, and twenty seconds for each framework lane. SQL readiness
has a two-second bound and native completion has a ten-second diagnostic timeout.
On timeout probes release/cancel cooperative work and continue draining accepted
native jobs; the external watchdog remains necessary for a completely stuck lane.
