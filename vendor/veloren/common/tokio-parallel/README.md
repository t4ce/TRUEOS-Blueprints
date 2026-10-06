# Veloren Tokio parallel execution

The game algorithms retain the parallel iterator implementation from the local
Rayon 1.12.0 vendor tree (MIT OR Apache-2.0; licenses included). Its scheduler and
rayon-core dependency are replaced by `runtime.rs`, which submits CPU closures
as tasks to Tokio 1.52.3. This is an explicit iterator fork, not a renamed Rayon
worker scheduler.

The server selects its existing runtime through `State::pools_on`. ECS dispatch,
parallel component joins, terrain/world algorithms and bounded slow jobs use
those workers. Shared-runtime selection also covers parallel calls outside an
explicit `install`. Desktop clients and examples can build a standalone Tokio
runtime with `ThreadPoolBuilder`.

Borrowed jobs require a small unsafe lifetime bridge because Tokio accepts only
static tasks. Its private join/scope guards keep every job alive and wait for
closure destruction before borrowed data expires. Both join branches and all
scope descendants complete before a panic propagates. An unstarted child can be
claimed by its waiting caller; nested synchronous joins therefore make progress
with one Tokio worker. Queued task cancellation or runtime shutdown cannot skip
borrowed-job completion. Task contexts retain runtime handles rather than
runtime ownership, preventing runtime destruction inside its own worker.

`install` runs its root inline. Detached slow jobs go directly to Tokio and are
never claimed by scoped waits. Existing slow-job fairness and category/global
limits are retained; the server caps their admission below the worker count
when possible. Tokio worker count now covers CPU execution as well as I/O.
CPU closures are synchronous and cooperative, so long individual jobs are not
preempted. A Tokio worker still relies on TRUEOS's std thread backend.

On TRUEOS, the pinned Tokio vendor disables the unstealable worker LIFO slot.
Otherwise a synchronous caller can park with its child hidden in that slot,
while spare workers have no way to claim it. Other targets retain Tokio's
default; worker-local blocking dependencies there require a runtime built with
`disable_lifo_slot` (Tokio's unstable configuration).

The adapter also checks a 10 ms carrier-turn budget at root installation, job
execution, and scope submission boundaries on TRUEOS. When the budget expires,
it yields the logical std thread with no adapter lock held, then resumes its
existing continuation. This prevents a succession of inline child jobs from
monopolizing a carrier without ever reaching a condition-variable wait. The
budget is checked at boundaries; a single long closure still needs its own
cooperative checkpoints. No detached slow job is executed by a scoped waiter.
The native Tokio worker loop independently checks its carrier budget between
async task polls, so repeated Tokio task yields also let std carrier peers run.

Specs (upstream revision 4e2da1df29ee840baa9b936593c45592b7c9ae27), Shred 0.16.1,
Hibitset 0.6.4, Hashbrown 0.17.1, and IndexMap 2.14.2 are vendored in `../../vendor`
to use the same iterator types. The collection feature/module names `rayon`
are retained as upstream compatibility switches; their dependencies now select
this Tokio executor. The default packed server's active dependency graph has no
Rayon or rayon-core. Optional Wasmtime plugins and Criterion benchmarks can
still retain their own upstream Rayon dependencies.

Run the executor/ECS integration suite with:

```sh
cargo test -p veloren-tokio-parallel --offline
cargo test -p veloren-common --lib slowjob --offline
cargo test -p veloren-common-ecs --lib --offline
```

Retained upstream iterator unit tests/documentation examples assume the complete
Rayon scheduler API and are disabled for this fork. Its supported execution
contract is covered by the integration suite instead. TRUEOS execution is
covered by `TRUEOS-Blueprints/probes/veloren_executor`, exercising actual Specs
component joins and dependency-ordered dispatch on runtimes with one and two
workers, with nested borrowed joins and descendant scopes.
The same probe forces 1,024 condition-variable handoffs between eight Hull/native
callers and 256 already-claimed child joins from Hull and Tokio callers. Two CPU
tasks also require two sleeping carrier peers to make progress without logging
or manual std yields inside the workload, both during parallel jobs and while
yielding only between Tokio tasks. Run it with `QEMU_SMP=4` to exercise two
background carriers and oversubscribed logical threads.
