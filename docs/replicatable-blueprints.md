# Replicatable Blueprint lifecycle exploration

F2 Apps should own orchestration. A Blueprint may opt into a cooperative
lifecycle contract when it has external resources that cannot safely be copied.
Blueprints without lifecycle support continue to start and stop normally.

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
4. Resume or replicate assigns a fresh instance identity and sends
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

## Instance identity

Replication must create a new instance ID while preserving a lineage ID. App
filesystem roots should default to per-instance writable overlays with an
explicit read-only/common mount. This prevents two resumed copies from silently
writing the same state directory.

## First vertical slice

`hello_world_replicatable` establishes the first invariant: two Blueprint
instances cannot silently own the same TCP listener port. It demonstrates both
incrementing from a preferred port and asking TRUEOS for an ephemeral port.

The next slice should add an opt-in lifecycle poll/ack ABI and route F2 `pause`
through it. Live replication must remain disabled until the VM checkpoint covers
all required CPU and writable-memory state; until then, checkpoint-and-restart is
the honest implementation model.
