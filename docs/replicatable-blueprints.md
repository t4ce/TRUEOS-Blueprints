# Replicatable Blueprint lifecycle exploration

F2 Apps should own orchestration. A Blueprint may opt into a cooperative
lifecycle contract when it has external resources that cannot safely be copied.
Blueprints without lifecycle support continue to start and stop normally.

The package opt-in is explicit metadata which is encoded into the `.bp` header:

```toml
[package.metadata.trueos-blueprint]
replicatable = true
```

F2 requires both the artifact tag and imports of the lifecycle
poll/Ready/identity ABI before exposing an app in its `pause` table. Metadata
alone is not a safety claim. Blank submit lists only protocol-capable running or
latched VM slots; submitting a displayed VM ID toggles pause/resume. `unpause`
is retained only as a command alias, not a separate tab, and neither action
silently defaults to VM0. Paused slots keep independent VM store devices, so
saving a second slot does not replace the first slot's checkpoint index.

Both attached control paths, including the inner Hull `vmx>` mini shell, follow
the same distinction: `stop` exits without a checkpoint, `pause` enters the
replicatable lifecycle and tells the user to resume by ID from the F2 `pause`
table, and `preserve` performs a raw checkpoint-and-stop without retaining the
Blueprint lifecycle latch. The host VM-exit dispatcher carries stop and
preserve as distinct outcomes rather than inferring preserve from every
terminating `vmcall`.

## Required boundary

The snapshot contains logical, in-VM state. It must not claim ownership of live
host capabilities such as sockets, in-flight fetches, file-write transactions,
GPU queues, UI windows, audio streams, or device sessions. Those capabilities
are released before a checkpoint and reacquired after resume or replication.

The host-side flow should be two phase:

1. F2 sends `PreparePause { operation, deadline, reason }`.
2. The app stops new work, drains or cancels in-flight work, releases external
   capabilities, and returns `Ready { checkpoint_version }`.
3. F2 freezes/checkpoints only after `Ready`. Timeout fails safely unless the
   user explicitly requests a force stop.
4. Resume retains the instance identity and increments its generation.
   Replication assigns a fresh instance identity and sends
   `Resume { operation, instance, lineage, generation }`.
5. The app reconstructs external resources and reports `Running` or a typed
   rebind failure.

`reason` should distinguish `Pause`, `Replicate`, and `Migrate`; apps may need a
different drain policy for each. Every request and acknowledgement needs an
operation ID so delayed messages cannot affect a later lifecycle transition.

## Resource policies

- Listening sockets declare `Fixed`, `Increment { preferred, attempts }`, or
  `Ephemeral`. TRUEOS owns the port lease and reports the actual endpoint.
- Outgoing sockets are closed and reconnected. Protocol sessions decide whether
  to resume, authenticate again, or start cleanly.
- Read-only files may be shared when immutable/versioned. Mutable files need an
  instance overlay, an explicit shared-data declaration, or an exclusive lease.
- GPU, UI, audio, fetch, and device handles are always recreated for the new VM
  principal. Numeric handle values are never checkpoint data.
- Background tasks must be joined/cancelled before `Ready`, not left holding a
  capability behind the app's lifecycle handler.

## Compact Axum default

Most HTTP Blueprints do not need a bespoke lifecycle object. Enable the
`lifecycle-net` facade feature, keep the capability metadata explicit, and
replace the ordinary Tokio listener with one macro call:

```rust
// trueos-blueprint: features=["lifecycle-net"]

static HTTP_PORT: AtomicU16 = AtomicU16::new(0);

let address = SocketAddr::from(([0, 0, 0, 0], 8080));
let listener =
    trueos::lifecycle_axum_listener!("my-http", address, &HTTP_PORT).await;
axum::serve(listener, router()).await?;
```

The default policy prefers the declared port and tries the next 31 ports when
another instance owns it. It publishes the actual port through the supplied
atomic. While idle it probes the listener lease every 500 ms. After F2 resume,
the old host socket reports `NotFound`; the adapter drops only that listener,
binds a fresh one, and keeps the Axum router and ordinary in-VM state alive.
The periodic probe is important because a revoked Tokio readiness registration
cannot be relied on to wake itself.

`ServerConfig` and `PortPolicy` expose fixed, incrementing, and ephemeral forms;
pass a custom configuration with
`lifecycle_axum_listener!(@config config, &HTTP_PORT)`. This helper deliberately
covers listening sockets, not every external capability. Outgoing connections
still need protocol-level reconnect logic. Path-based, request-scoped file
access is a good default; long-lived file handles and in-flight writes need an
app-specific boundary.

## Instance identity

Every fresh launch receives a UUIDv4 instance and lineage ID. Its writable root
is `apps/<archive-stem>/<instance-guid>`; same-instance Resume keeps that root
and increments `TRUEOS_APP_GENERATION`. `TRUEOS_APP_INSTANCE`,
`TRUEOS_APP_LINEAGE`, and `TRUEOS_APP_IS_CLONE` expose the remaining identity.
This prevents repeated launches from silently writing the same state directory.

TRUEOS does not copy mutable app files as part of VM pause, snapshot, or clone.
The Blueprint owns that decision at `PreparePause`: it may copy selected files,
write an app-level checkpoint, use immutable shared data, or deliberately start
the clone with an empty root. The explicit `apps/common` surface remains for
declared shared/read-only data, not as the default writable home.

## First vertical slice

`hello_world_replicatable` establishes the first invariant: two Blueprint
instances cannot silently own the same TCP listener port. It demonstrates both
incrementing from a preferred port and asking TRUEOS for an ephemeral port.

`chatserver`, `monaco`, and `swarm` use the compact Axum boundary and opt into
the F2 table. Swarm folds the former standalone Texteditor proof into its Python
workspace: embedded templates and writable files can be opened, edited,
created, deleted, and uploaded to a selected node. Writable sources live under
`swarm/sketches`, while embedded sketches remain immutable templates that can
be saved into that workspace. Together these apps cover preserved in-memory
state, path-based file access, inbound listener recovery, and Swarm's UDP
discovery lease without making lifecycle support mandatory for unrelated
Blueprints.

The original `hello_world` and `gridpaper` apps also opt in. Gridpaper exercises
the managed-service form of the contract: pause detaches the owner's UI4
presentation, while its kernel-owned page, resident 3D scene, GPU allocations,
and last front buffer remain retained. Resume re-arms the same VM owner and
creates a new UI4 presentation session over that retained scene.

Snapshot format v3 preserves the live VMCS RIP/RSP, guest GPRs and RFLAGS, and
the restored stack is retained through relaunch. A same-slot F2 resume therefore
continues after the exact pause boundary instead of entering the Blueprint at
its default seed again. Formats v1 and v2 remain readable with their historical
checkpoint-and-restart semantics.

The lifecycle ABI now exposes operation-scoped `PreparePause`, an exact `Ready`
VMCALL checkpoint boundary, and post-resume identity. A successful Ready call
does not return to Blueprint code before the checkpoint; it returns after
Resume, when the app can rebuild resources. Requests expire after the host
deadline and leave the VM running when the app does not acknowledge.

`hello_world_replicatable` is the end-to-end implementation: its accept loop
polls the lifecycle request, drops the listener before Ready, and reacquires a
conflict-safe port after Resume. The generic listener adapter remains reactive
recovery and is not by itself a Ready implementation for other apps.

Cross-slot and cross-host memory cloning remains disabled. The current snapshot
retains the source VM's guest heap arena and allocator metadata; copying only
the CPU/stack/page-table image would alias or lose writable Rust state. The
remaining v-layer must serialize or copy every guest-writable backing, rebuild
allocator mappings for the destination VM principal, and then issue a new
instance identity while preserving lineage. No UI action should label a
same-slot Resume as a clone until that invariant is met.
