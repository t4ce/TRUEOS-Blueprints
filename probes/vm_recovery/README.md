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
host smoke checks. `GET /oom` requests a real infallible 4 GiB `Vec` allocation
on the native worker, beyond the guest heap; it never fills or touches that
buffer. It exercises the allocation-error terminal path with a distinct marker.
Kill tests use Shell3 VME `kill` and deliberately omit normal
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

The primary rig recipe embeds these local packages in volatile AppDB. From
the sibling TRUEOS checkout, prepare a new overlay from the prepacked normal
built-ins and the two probes:

```sh
python3 tools/testpy/prepare_vmx_recovery_buildins.py \
  --output bld/vmx-recovery-local/blueprints
```

The helper makes independent package copies, adds `vm_recovery.bp`, and aliases
the peer bytes as `vmpeer.bp`. It writes package hashes and the exact build
arguments to `metadata.json`. Run those arguments from the TRUEOS checkout so
Make uses the snapshots without invoking the ordinary built-in packer:

```sh
python3 - <<'PY'
import json
from pathlib import Path
import subprocess

metadata = json.loads(Path("bld/vmx-recovery-local/blueprints/metadata.json").read_text())
subprocess.run(metadata["make_argv"] + [
    "BUILD_MODE=release", "CARGO_BUILD_FLAGS=--release",
    "PUBLISH_RELEASE_SMB=0", "START_BAREMETAL_LOG=0",
], check=True)
PY
```

Deploy the resulting recovery test image through the normal verified physical
rig procedure and start a fresh capture. Then run from TRUEOS, selecting that
capture and a new output directory:

```sh
python3 tools/testpy/verify_vmx_recovery_baremetal.py --appdb \
  --log bld/baremetal-logs/LatestOfThree.logs \
  --output bld/vmx-recovery-local/run1
```

`--appdb` uses SSH with real PTYs and launches bare `vmpeer` / `vm_recovery`
names. Shell3 recognizes those names immediately. The short peer alias avoids
`vm_recovery` matching prematurely while typing `vm_recovery_peer`. This local
overlay route does not contact or publish to an external application catalog.

The runner observes exact kernel generation cleanup markers, four victim
cycles, new GUIDs on the same VMID, socket rebinding, UI4 surface revocation,
and a live independent peer across each
recovery. The default sequence is parked kill, native panic, native allocation
failure, and parked kill. `--actions kill panic kill` selects a smaller subset.
After retirement it discovers HTTP filesystem roots and reads the committed
per-instance witness. It directly requests the exact abandoned stream URL on
that same root and requires HTTP 404 with the exact body `not found\n`; absence
from the size-limited HTML tree alone is insufficient. It saves the appended
kernel log, SSH transcripts, completed cases and `summary.json`. The runner
builds no kernel and performs no hardware reset.

An optional catalog route remains available when publishing to that destination
is authorized:

```sh
CARGO_NET_OFFLINE=true ./probes/vm_recovery/build_publish.sh
```

That script uses the existing SMB apps catalog convention. For catalog testing,
omit `--appdb`; the runner launches `§§vm_recovery_peer` / `§§vm_recovery` through
Appstore. Publication is unnecessary for the local rig recipe above.
