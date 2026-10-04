# Veloren server Blueprint build attempt

This package tries the existing `veloren-server-cli` source through the TRUEOS
Blueprint packer without adding Veloren to the TRUEOS-Blueprints Cargo
workspace. Veloren's root manifest selects the local Tokio executor and ECS
vendor patches described below. `src/main.rs` includes the
upstream CLI as the crate root, so its console commands, web UI, and server
logic remain the same.

The default build enables `worldgen` for the full generated world rather than
the basic test terrain. `persistent_world` remains optional for saving terrain
changes. World generation requires the Veloren world assets at runtime.

From `TRUEOS-Blueprints`, with the Veloren checkout beside it at
`../veloren`, run:

```sh
VELOREN_SOURCE_ROOT="$(realpath ../veloren)" \
CARGO_WORKSPACE_DIR="$(realpath ../veloren)" \
TRUEOS_BLUEPRINT_SKIP_APPS_PUBLISH=1 \
cargo bp apps/velosrv
```

The environment variables provide the source and compile-time workspace path
to Veloren's included modules. Publication is disabled so this is a local pack
attempt only. Build failures identify the next platform dependency that needs
an adapter; a successful `.bp` build still needs a runtime launch check before
the server can be considered supported.

## Runtime storage paths

Veloren uses `current_exe()` only as a fallback for locating userdata. Its
`VELOREN_USERDATA` runtime variable takes precedence. TRUEOS seeds this variable
with `<VM HOME>/userdata` for `velosrv`, so it is visible in `vmx_env` and uses
the existing per-instance filesystem scope. For the default instance it is
`/apps/velosrv/userdata`; named instances use their own VM home. This works with
older packed server code that still has the executable-relative fallback.

CLI settings live below `userdata/server-cli`, while server configuration and
saves live below `userdata/server` (saves use `userdata/server/saves`).
The `vmx_env` view currently displays variables; it does not edit them.

Assets are a separate directory, selected by `VELOREN_ASSETS`. The current
TRUEOS source also searches app-local `assets` and then
`/common/veloren/assets` (the latter maps to `apps/common/veloren/assets`).
The absolute shared alias explicitly names the shared installation rather than
a path below the app's working directory.
That directory needs the actual Veloren asset tree. Setting a path does not
grant filesystem access outside the Blueprint's app/common scope.

On TRUEOS, the asset source uses `trueos::async_fs` directly for discovery,
directory enumeration, canary checks, reads, and asset-tree traversal. Native
listings supply each child's node kind without a separate metadata call. The
synchronous asset-cache interface waits cooperatively for kernel-owned async
jobs; it does not create a nested Tokio runtime. Asset IDs, extensions, and
override precedence stay compatible with Veloren. A truncated directory listing
is reported as an error rather than silently omitting assets.

The native transport contract can be checked on the host with:

```sh
python3 ../veloren/common/assets/tests/trueos_source_contract.py
```

This runs the actual TRUEOS async client against pending mock CABI operations
using the kernel's binary directory encoder, including short result reads,
override merging, malformed/truncated listings, and real server asset files.

## Tokio CPU scheduling

The server and its Specs/collection adapters now use Veloren's local
`common/tokio-parallel` iterator fork. It retains the iterator algorithms and
runs scoped CPU tasks on the existing Tokio 1.52.3 runtime. ECS dependency
ordering and borrowed component lifetimes are preserved. The separate Rayon
ECS and slow-job worker pools are removed. Slow-job admission reserves worker
capacity for ticks/networking when possible. This requires the local vendored
Specs, Shred, Hibitset, Hashbrown and IndexMap patches in this manifest.

Tokio workers still use the platform std thread backend; CPU closures run
cooperatively and are not preempted by Tokio. The migration does not establish
that every previously observed platform stall was caused by Rayon.

The first TRUEOS executor run exposed a separate kernel fault: allocator
diagnostics walked an invalid guest frame pointer on a native std-thread stack
while holding the guest heap lock. `TRUEOS/src/allocators.rs` now excludes both
Hull and native Blueprint stacks from that kernel-only frame walk. The fixed
QEMU run completed 64 Specs ticks with one and two workers, nested borrowed
joins, and descendant scopes (`bld/veloren-tokio/qemu-ecs-fixed/result.json`).

`probes/veloren_executor` tests the same scheduler and actual Specs dispatch on
TRUEOS. With an ISO embedding that probe, run from the TRUEOS checkout:

```sh
python3 tools/qemu/verify-tokio-platform.py --iso <private.iso> \
  --output <new-evidence-directory> --probe veloren_executor
```

## Cooperative world bootstrap

The worldgen build selects `world/cooperative-worldgen`. Startup economy history
updates sites sequentially with quiet, timed scheduling checkpoints between
complete updates. Deliveries, site calculations, order distribution and trade
retain their original phase order; live ECS and chunk-job scheduling still use
the shared Tokio runtime. A startup tick no longer waits on a parallel site
completion barrier for each of its 2,000 history iterations.

Peak naming uses neighbouring spatial buckets instead of comparing every peak
with every other peak. The strict radius, equal-height ties, original peak order
and removal by higher peaks that themselves get removed are preserved. Naming,
biome scans and subsequent civilisation/resource loops reach a 10 ms work-budget
checkpoint between units. Logging is separate from these checkpoints. Long
individual site generators still need their own internal scheduling points.

The first civilisation markers after `terrain-simulation-complete` are
`civs stage=enter`, followed by peak/biome naming start and completion. These
bracket the previously silent startup work without logging every iteration.
Spot placement also checkpoints between candidate locations. Its markers separate
the first manifest load from each placement pass, followed by `spots-complete`
and worldgen `complete`. Map preparation then reports marker, column-sample,
horizon and pixel phases. The cooperative build collects samples and both
horizon directions in input order without parallel joins, and checkpoints
between complete samples and pixels; the terrain and horizon calculations are
unchanged. These boundaries distinguish a spot/asset wait from subsequent map
preparation rather than attributing a stall to the last printed stage.
Column sampling reports its exact buffer reservation separately from the first
sample, then reports completed/total samples about every two seconds. Horizon
directions and pixel generation have their own completed-work counters. These
counts measure returned work; a running VM or busy CPU alone is not a progress
guarantee. Sampling keeps the original calculations and visits every chunk.
After the map returns, the server still builds its complete distant scenery
(LOD) cache. The cooperative build visits each zone sequentially rather than
entering nested parallel zone iterators. Tree candidate, column and structure
work reaches quiet checkpoints inside each zone; `lod` completed-zone counters
show returned work. The same tree/structure algorithms, positions and complete
zone cache are retained. Startup now also brackets LOD, ECS setup, persistence,
spawn-point generation, networking and RtSim so later work is not mistaken for
a map-generation stall.
`world/tests/cooperative_bootstrap.rs` compares the new peak rule with an
all-pairs oracle and checks borrowed site-phase completion and map sample order;
the native
`veloren_executor` probe includes the actual worldgen site helper and requires
sleeping std carrier peers to advance during its workload.

## Graceful VM stop

The server registers `trueos::shutdown::ShutdownGuard` before its Tokio runtime
and logging guards, then polls the host request between game ticks. `vmx_stop`
and Apps `stop <vmid>` leave the Hull and native-job admission available while
server cleanup flushes persistence/logging and drops the runtime. The guard
acknowledges shutdown last; the host drains remaining thread destruction before
releasing the realm and publishing the VM offline. ECS/background pools retain
only a Tokio handle so they cannot keep the runtime alive during cleanup.

This requires a kernel with `trueos_cabi_blueprint_stop_control_v1` and a newly
packed server. Updating only the Blueprint on an older ISO is insufficient.
Healthy cleanup is cooperative; a stalled game tick or nonreturning native job
can still retain its resources. The QEMU `tokio_stop` probe verifies worker/TLS
cleanup, zero native jobs, carrier release, and reuse of the same VM slot.
Kernel keyed wait registries and waker buffers must also use host allocation;
the updated teardown retires that VM's registry entries after workers return,
while concurrent network wakeups retain safe queue references.
