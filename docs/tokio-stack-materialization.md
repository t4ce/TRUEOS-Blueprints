# Tokio stack materialization

`tokio_stack` exercises Bytes, Tower, Tracing and a generated Tonic unary gRPC
service/client over loopback TCP. It is a runtime acceptance probe, not proof
that every API in these crates works on TRUEOS.

## Current boundaries

| Surface | Pin | Execution boundary |
| --- | --- | --- |
| Rust core/alloc/std | nightly-2026-07-10 | TRUEOS std thread lifecycle is unsupported; atomics, synchronization, WLS, clocks and I/O remain enabled. |
| Mio | 1.2.0 | Registrations/selectors remain in userspace. Unix poll/readiness and wake operations terminate at TRUEOS. |
| Tokio | 1.52.3 | Each native lane owns a current-thread runtime. Async tasks stay in that runtime; explicit `trueos::worker` supplies additional native lanes. |
| Bytes | 1.11.1 | Application buffers and operations; storage uses the platform allocator. |
| Hyper / Tower / Axum / Tonic | 1.9.0 / 0.5.3 / 0.8.9 / 0.14.6 | Protocol and application state remain in the Blueprint; transport uses Tokio/Mio. |
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

The builder checks native source declarations/exports and installs the canonical
std backend from the selected TRUEOS checkout. A stale pthread lifecycle import
is rejected before packing. Changes to installed backend, selectors and WLS
sources change the build-std cache fingerprint.

## Supported probes and remaining boundaries

`tokio_rt`, `tokio_fs`, `tokio_net`, and `framework_stack` retain focused
current-thread coverage. `tokio_mrt`, `wls`, `condvar`, `cross`, and
`redb_multirt` use explicit native worker admission and completion.

Use `trueos::net::resolve_host` for asynchronous hostname lookup. Generic Tokio
hostname lookup still uses its unsupported std-thread blocking pool; Hyper's
TRUEOS GAI path is synchronous unless the application supplies a native async
resolver. Superseedr's TRUEOS tracker client supplies that resolver.

Actual Tokio asynchronous stdin/stdout/stderr I/O still uses the blocking pool.
Constructing those handles in `tokio_rt` is not an I/O acceptance test. The custom
TRUEOS Tokio filesystem implementation uses asynchronous CABI operations.

Player's TRUEOS startup probe uses native work and reports errors without
preventing the UI from opening. Superseedr's TRUEOS shell/trackers are aligned;
the desktop engine's blocking calls are behind a different feature path. This
series does not establish full peer-to-peer operation.

Compilation, packing, symbol inspection and runtime acceptance are subsequent
validation. Syntax/static checks alone must not be reported as a passing rig run.


The light-stress follow-up gives `tokio_stack` two native lane-owned runtimes,
each with four clients sending eight gRPC requests to an isolated ephemeral
loopback server. Successful output includes `tokio_stack: PASS`, two lanes,
64 total verified replies and joined server shutdown. Each lane installs its own
Tracing subscriber. This expected output is acceptance criteria, not a recorded
runtime result.
