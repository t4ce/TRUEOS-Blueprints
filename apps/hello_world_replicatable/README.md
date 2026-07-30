# hello_world_replicatable

This Blueprint is the first resource-reacquisition probe for F2 Apps. It owns a
TCP listener and keeps durable/logical state separate from the listener handle.
It opts in with:

```toml
[package.metadata.trueos-blueprint]
replicatable = true
```

Run two instances with the same preferred port:

```text
apps start <catalog-id> next 48080
apps start <catalog-id> next 48080
```

The first instance claims `48080`; the second observes the conflict and tries
`48081`. To let TRUEOS choose an unused port instead:

```text
apps start <catalog-id> auto
```

In the F2 Apps `pause` tab, submit an empty line to list only running or paused
Blueprint VMs carrying that tag. Enter the displayed VM slot ID to toggle that
instance between running and paused; there is no implicit VM0 fallback and no
separate `unpause` tab.

This Blueprint implements the cooperative lifecycle boundary. It polls for
`PreparePause`, stops accepting and drops its listener, acknowledges `Ready`,
and returns from that call only after `Resume`. It then logs the host-issued
instance/lineage identity and reacquires a conflict-safe listener.

Every fresh launch receives its own writable root:

```text
apps/hello_world_replicatable/<instance-guid>
```

The VM checkpoint owns in-memory `LogicalState`; the platform does not copy
application files into another instance root. A Blueprint that wants files in a
future clone must arrange that explicitly while handling `PreparePause`.
