# hello_world_replicatable

This Blueprint is the first resource-reacquisition probe for F2 Apps. It owns a
TCP listener and keeps durable/logical state separate from the listener handle.

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

This probe demonstrates that a fresh or resumed instance can reacquire an
external resource. It does **not** make the current VM snapshot format safe to
clone. A real F2 pause/replicate path must first ask the app to quiesce and drop
external handles, then checkpoint logical state, create a new instance identity,
and finally let each instance reacquire its own resources.
