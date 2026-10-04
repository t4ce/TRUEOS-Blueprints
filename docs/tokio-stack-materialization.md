# Tokio stack materialization

`tokio_stack` exercises Bytes, Tower, Tracing and a generated Tonic unary gRPC
service/client over loopback TCP. It is a runtime acceptance probe, not proof
that every API in these crates works on TRUEOS.

## Current boundaries

| Surface | Pin | Execution boundary |
| --- | --- | --- |
| Rust core/alloc/std | nightly-2026-07-10 | TRUEOS stackful std threads provide spawn, join, detach and independent key-backed TLS with exit destructors; synchronization, clocks and I/O terminate at TRUEOS. |
| Mio | 1.2.0 | Registrations/selectors remain in userspace. Unix poll/readiness and wake operations terminate at TRUEOS. |
| Tokio | 1.52.3 | Current-thread and multi-thread runtimes use the pinned platform vendor. Multi-thread workers and the blocking pool use the TRUEOS std thread backend; explicit `trueos::worker` remains available for finite native jobs. |
| Bytes | 1.11.1 | Application buffers and operations; storage uses the platform allocator. |
| Hyper / Tower / Axum / Tonic | 1.10.0 / 0.5.3 / 0.8.9 / 0.14.6 | Protocol and application state remain in the Blueprint; transport uses Tokio/Mio. |
| Tracing | 0.1.44 | `trueos::trace::with_default` installs the kernel subscriber on each executing lane. |

The earlier adapter-based size/import measurements do not validate this source
layout. Do not expect the old `trueos_mio_selector_*` import family: selector
registration is intentionally userspace-owned.

## Native work contract

Use `trueos::worker::spawn` for finite owned work and await the returned handle.
Construct and drop any current-thread runtime inside that closure. Capacity is
advisory; partial fleet admission must release barriers and join every accepted
job. A dropped handle detaches a job. Kernel teardown retains its code/resources
until it finishes; an unresponsive native job needs external diagnosis.

Concurrent native workers have distinct stable WLS slots. Sequential jobs may
reuse slots and TLS values. Build/drop/rebuild a runtime on the same worker to
check that Tokio enter guards and runtime context are released correctly.

Normal Tokio runtime shutdown wakes and joins its workers and runs their TLS
destructors. A crash or force-stop with persistent stackful threads retains the
VM's executable pages, heap and process resources until those threads exit
cooperatively. The kernel does not abandon suspended stacks that may still own
guest code, TLS values or host guards; non-cooperative threads can leave teardown
pending. Stackful execution is cooperative on shared carriers, with independent
guarded stacks and inline stack probes.

Each VM-owned thread retains a carrier page-table overlay that maps the Hull's
main stack at the same virtual addresses as the guest. Scoped children can
therefore borrow parent stack values directly; nested scoped children share
their carrier parent's guarded aliases. Hull mutex contention yields through
VMCALL instead of entering a host executor with guest per-CPU state.

Guarded stacks use compact reservations in a monotonic 512 GiB virtual alias
arena. Completion reclaims the physical stack pages after the carrier returns
to its parent stack. Retired virtual aliases are never reused; reuse requires
future synchronous invalidation of remote CPU TLB entries. Admission fails
cleanly if the bounded alias arena or thread capacity is exhausted.

The builder checks native source declarations/exports and installs the canonical
std backend from the selected TRUEOS checkout. Thread lifecycle uses the additive
`trueos_cabi_thread_*` ABI; stale pthread lifecycle imports are rejected before
packing. The `tokio-platform-v7-stackful-threads` build-std cache fingerprints the
installed backend, selectors, key-backed TLS storage and destructor guards.
The old slot-indexed no-threads TLS path is no longer installed for TRUEOS.

## Supported probes and remaining boundaries

`tokio_rt`, `tokio_fs`, `tokio_net`, and `framework_stack` retain focused
current-thread coverage. `tokio_mrt` retains explicit native admission/completion
coverage and adds independent std spawn/join, TLS isolation/destructors,
wake-before-park and cross-thread unpark. Two scoped children also read and
update values borrowed from their parent's stack across carriers, with distinct
thread identities and TLS. Each child also starts a nested scoped child borrowing
its own guarded stack, checking both Hull-to-carrier and carrier-to-carrier
address stability. It builds, runs and shuts down a two-worker Tokio runtime
twice, exercising cross-worker tasks, timers, `spawn_blocking`, `block_in_place`,
loopback TCP and worker TLS destruction. `wls`, `condvar`, `cross`, and
`redb_multirt` retain focused native worker coverage.

Use `trueos::net::resolve_host` for hostname lookup on explicit native capacity.
Generic Tokio hostname lookup uses the supported std-thread blocking pool.
Hyper's TRUEOS GAI path is synchronous unless the application supplies a native
async resolver. Superseedr's TRUEOS tracker client supplies that resolver.

Tokio asynchronous stdin/stdout/stderr I/O uses the std-thread blocking pool.
Constructing those handles in `tokio_rt` is not an I/O acceptance test. The custom
TRUEOS Tokio filesystem implementation uses asynchronous CABI operations.

Player's TRUEOS startup probe uses native work and reports errors without
preventing the UI from opening. Superseedr's TRUEOS shell/trackers are aligned;
the desktop engine's blocking calls are behind a different feature path. This
series does not establish full peer-to-peer operation.

Compilation, packing, symbol inspection and runtime acceptance are subsequent
validation. Syntax/static checks alone must not be reported as a passing rig run.

The stackful-thread change passes all 75 Blueprint builder tests and packs the
extended `tokio_mrt` probe for the custom TRUEOS target with matching CABI
signatures. The nested scoped probe's local `dist/tokio_mrt.bp` SHA-256 is
`b08f5e6ed91c650a6606eacaf2271c02021c01bb44b5836ad154afe7da4195fb`.
Its std/multi-thread test logic also passes on Linux with the
vendored Tokio 1.52.3: two joined threads, one detached thread, two scoped
children sharing parent stack values, five std TLS destructors, and six
started/stopped/TLS-destroyed runtime threads per wave. Two additional nested
children share their carrier parents' guarded stack values and stable identities.
The same packed probe passes on TRUEOS in isolated QEMU using the final normal
ISO. The recorded result is
`../TRUEOS/bld/thread-acceptance/qemu-run-6/result.json` (`status=PASS`, elapsed
12.849 seconds), with its runtime output in `shell.log`. The local seeded
Blueprint was launched through Shell2 Default mode, with no remote fetch or
publication. Both Tokio runtime
waves start, stop and destroy TLS for six threads, complete sixteen blocking
tasks, pass `block_in_place`, and exchange TCP ping/pong on assigned loopback
ports 50000 and 50001. Runtime shutdown completes in each wave. The joined,
detached, scoped and nested scoped std-thread checks pass, followed by the
retained two-lane native stress test and the final `tokio_mrt: PASS` marker.
This establishes the probe's thread/runtime contract; velosrv startup is a
separate smoke test.


The light-stress follow-up gives `tokio_stack` two native lane-owned runtimes,
each with four clients sending eight gRPC requests to an isolated ephemeral
loopback server. Successful output includes `tokio_stack: PASS`, two lanes,
64 total verified replies and joined server shutdown. Each lane installs its own
Tracing subscriber. This expected output is acceptance criteria, not a recorded
runtime result.


## Velosrv thread-spawn failure provenance

The failing `velosrv.bp` fetched with SHA-256
`748d6b2b9b0979fb41af72e3e0d52962cee780b9035e6864add9d7c9ca2614dd`
exactly matched the locally packed artifact. Its retained Cargo lock selected
Tokio 1.52.3 without a registry source; dependency files point to
`vendor/tokio-1.52.3`, and the Tokio fingerprint enables `rt-multi-thread`.
The target is `x86_64-unknown-trueos` (`os=trueos`, `env=musl`, Unix family),
compiled using the pinned nightly-2026-07-10 Rust source. Its linked module
contains `std::sys::thread::trueos::Thread::new`, whose previous implementation
returned `UNSUPPORTED_PLATFORM` unconditionally. Veloren builds a multi-thread
runtime immediately on entry; Tokio reaches that unsupported lifecycle while
starting its workers. The failure therefore comes from the platform backend,
not an unpinned Tokio or a host pthread implementation.

The replacement adds the missing stackful lifecycle and schedules independent
thread contexts, while preserving the ordinary Tokio 1.52.3 worker/blocking-pool
code paths. Repacking is required: the fetched hash identifies the old backend.
Use `TRUEOS_BLUEPRINT_SKIP_APPS_PUBLISH=1 cargo bp apps/velosrv` for local build
validation; compilation and packing do not replace a runtime acceptance run.

The local rebuild passes the CABI guard with 16 matching imports and produces
`dist/velosrv.bp` with SHA-256
`0dd31276738e05e542a137fb852126a5f42edf18724205f0eb259d7aa68accdb`.
The linked module imports the canonical spawn/join/detach functions and no
POSIX thread lifecycle functions. Publication was disabled during validation;
the remote artifact identified by the old hash remains a separate deployment.

The separate velosrv smoke is recorded in
`../TRUEOS/bld/thread-acceptance/qemu-velosrv-2/result.json`. A transient 128 MiB
TRUEOSFS RAM disc mounted successfully (`root_mounted=1`); the sole unresolved
import from the earlier smoke, `renameat`, has a real kernel implementation.
Velosrv then reached `common_frontend::init` and panicked at
`common/base/src/userdata_dir.rs:64` because `std::env::current_exe()` returned
errno 38 (`Unsupported`). Auditing `server-cli/src/main.rs` establishes that its
multi-thread Tokio `build().unwrap()` returned before this initialization call.
No explicit server-ready marker was observed, so the smoke records successful
runtime construction followed by an application startup failure. Generic
executable-path discovery remains a separate platform boundary.

On 2026-10-04, the host launch environment now supplies
`VELOREN_USERDATA=<VM HOME>/userdata`. This selects the real per-instance data
directory before Veloren's executable-path fallback and grants no additional
filesystem scope. The absolute app path keeps the same location if broader FS
scope is explicitly granted later.
`../TRUEOS/bld/thread-acceptance/qemu-velosrv-env-2/result.json` records a smoke
with the existing local packed server and the updated kernel. The exact embedded
server SHA-256 is
`96ea744cd2f467bb74f239b3667edd482b01f872e80ed4084881e2b2b2b8bd9b`;
no server source change or repack was performed for this environment bridge. Its native
`vmx_env` view reports `VELOREN_USERDATA=/apps/velosrv/userdata`; the userdata
executable-path panic is gone. Startup proceeds to asset discovery, where the
empty test filesystem has no Veloren asset tree. The full-startup result remains
`FAIL` for that missing directory, rather than claiming server readiness.
CLI settings and server saves use this userdata root; `VELOREN_ASSETS` selects
the separate asset tree. See `apps/velosrv/README.md` for logical storage paths.
