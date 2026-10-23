# VM recovery contract

The victim retains two Tokio runtimes with two workers each, an indefinitely
parked native channel receiver, a TCP listener on 32379, a published UI4 frame,
a 64 KiB vGPU buffer, and a real incomplete filesystem stream. The peer uses
the same runtime workload on 32380 and its own UI4 resources, with no filesystem
operations or retained disk lease. Every launch emits its host-issued instance
GUID in `vm_recovery: READY` after all resources exist.

`GET /status` returns the instance and both Tokio ticker counters. `GET /panic`
acknowledges the request, wakes the native receiver, and explicitly panics after
all channel and logging calls have returned. `GET /exit` is normal cleanup for
host smoke checks. Kill tests use Shell3 VME `kill` and deliberately omit normal
application cleanup. No worker spins or requires unsafe host-stack preemption.

The private `hull_stream_transport.rs` matches the existing guest CommPage
client and operations in `TRUEOS/crates/trueos-vm/src/vmcall.rs`. It runs only
on the Hull during setup or normal cleanup. `build.rs` checks the authoritative
layout, address and opcodes. This probe uses this low-level contract because
the SDK's `async_fs::TypedWrite` stages RAM until commit and cannot retain a
disk-root writer between chunks. Removed synchronous filesystem CABI imports
remain blocked by the Blueprint loader. The native panic worker never owns or
drops this Hull-only stream guard.

Build without publishing:

```sh
CARGO_NET_OFFLINE=true TRUEOS_BLUEPRINT_SKIP_APPS_PUBLISH=1 cargo bpp vm_recovery vm_recovery_peer
```

Publish through the existing SMB apps catalog:

```sh
CARGO_NET_OFFLINE=true ./probes/vm_recovery/build_publish.sh
```

After deploying the recovery kernel, run from the sibling TRUEOS checkout:

```sh
python3 tools/testpy/verify_vmx_recovery_baremetal.py \
  --log bld/vmx-recovery-20261023/boot1/LatestOfThree.logs \
  --output bld/vmx-recovery-20261023/run1
```

The runner uses SSH with real PTYs and `§§vm_recovery_peer` / `§§vm_recovery`.
It observes exact kernel generation cleanup markers, three victim cycles
(parked kill, native terminal, parked kill), new GUIDs on the same VMID, socket
rebinding, UI4 surface revocation, and a live independent peer across each
recovery. After retirement it discovers HTTP filesystem roots, reads the
committed per-instance witness and confirms the incomplete stream was not
committed. It saves the appended kernel log, SSH transcripts and `summary.json`.
It builds no kernel and performs no hardware reset.
