# Shared preparation on three native AP workers

Measured on the physical TRUEOS i5-14500T rig on 2026-11-16. The normal
`cargo bp ttstt` build now enables `native-convolution`; the named Codex bundle
build also picks up that default. No runtime flags or launch script are
required. The kernel and TTSTT Blueprint were built, published and run on the
rig through the ordinary workflows.

## Result

Two interleaved passes, same warmed model and coordinator:

| Passage | Serial | Three logical AP workers | Less time |
|---|---:|---:|---:|
| short, 18 text bytes | 5.133 s | 4.351 s | 15.2% |
| medium, 133 text bytes | 28.970 s | 24.419 s | 15.7% |
| long, 460 text bytes | 92.780 s | 78.600 s | 15.3% |

Individual long runs were 92.394/93.165 s serial and 78.653/78.547 s parallel.
All twelve WAVs match the original Ubuntu serial oracle byte-for-byte. The
initial two-WAV native smoke test also matches it. All six parallel cases
reached three active remote jobs with no local-admission fallbacks; every
observed worker reported Performance and AVX-VNNI. These peak counters count
active job lifetimes, including runtime setup and yields, rather than exact
simultaneous arithmetic intervals.

The tested coordinator was logical slot 3, classified Performance with
AVX-VNNI. Serial mode disables dispatch to native workers while retaining the
same async coordinator, model, text, output and timing path. Parallel mode
uses the shared u8 activation cache, complete model, English Heart, speed 1.0
and original integer dot products/float epilogues.

For the long parallel passage, awaited convolution work averages 33.189 s;
45.411 s remains outside those windows. Shared-cache preparation takes about
3.127 s and is included in that outside time. These are wall measurements,
not native CPU-cycle or VNNI-unit occupancy measurements.

## Placement limit

This is a test of **three logical AP workers**, not three guaranteed separate
physical cores. The kernel service-lane selector leases different logical
slots, but its preference does not exclude SMT siblings. The first parallel
convolution in the paired run submitted its three shards to slots 4, 5 and 6;
slots 4 and 5 are SMT siblings of the same physical P core. The coordinator's
sibling, slot 2, is also eligible for worker placement.

The existing `trueos/pci/tlb.txt` report has no timestamp, but its CPUID 1F
SMT_Bits=1 and all twenty slot/APIC mappings match current boot records.
P-core sibling pairs are (0,1), (2,3), (4,5), (6,7), (8,9), (10,11).
The saved topology evidence distinguishes the existing report from a fresh
capture. Ubuntu used three distinct physical P cores. This placement
difference and the native dispatch mechanism prevent interpreting the Ubuntu
gain as a native guarantee; this experiment does not isolate their individual
effects. Physical-core-aware placement remains a separate experiment.

## Native execution and lifetime

`trueos::worker::spawn_with_preference(PreferPerformance, closure)` reuses
native service-lane admission. The kernel tries P slots first and can fall
back to other eligible slots. Existing ordinary `spawn` calls retain their
previous policy. `worker::placement()` reports actual logical slot/core kind.
This is a generic Blueprint API, not a TTSTT builtin or special kernel TTS
service. The kernel's existing guest generation, allocation realm, worker
identity and lifetime reservation remain responsible for accepted closures.

Each convolution submits at most three finite, owned channel shards. Jobs
share immutable weights, prepared u8 activation data, patch sums, scale and
bias metadata; each owns its output. No coordinator arena/stack borrow crosses
the native boundary. Each job drives a current-thread runtime, computes at
most 64 output positions before a cooperative poll/yield, then returns its
result through the native asynchronous JoinHandle. The coordinator submits
the shards before awaiting results and commits the original operation only
after assembly.

There are no idle native worker loops or permanent AP claims. This allows RAM
pause to drain finite accepted work without waiting for a coordinator to send
another job. Dropping an operation waiter sets cancellation; accepted workers
retain their own data until the next bounded window and normal retirement.
Rejected submissions compute only the missing shard on the coordinator and
increment the fallback counter. No fallback occurred in this paired test.

Normal speech requests use this path by default, retain the short submitted/
done timing notices and hand the complete successful clip to the existing
exclusive HTTP-forward playback path. Benchmark WAV writing happens after
the measured synthesis and does not exercise or alter audio transport.

## Validation and evidence

- Native selector/ownership harness: 15 tests passed.
- Worker SDK regression harness: 7 tests passed.
- TTSTT/default dependency suites: 88 tests passed, including exact two/three
  native-worker results and cancellation/cache release/pool reuse.
- One native smoke pair and twelve paired-run WAVs match the original oracle.
- Native compilation/publication: `cargo bp ttstt` succeeded.
- Kernel compilation/publication/reset: `cargo rigreal` succeeded; the
  published Blueprint was fetched and launched on the running rig.

The actual measured package SHA-256 is
`201355b34ae51c83885e39484e505dfe739bb3dc68f99add5a0d056aa4244457`.
Kernel ELF SHA-256 is
`eb8f4d99c3811d288d184f6d5d98bf0c9e0b1897b1ca178b552110ccdddeb6ca`.
It includes the native preference/placement ABI; older kernels without those
exports do not support this Blueprint build.

Evidence is under sibling `TRUEOS/target/ttstt-native-three-ap/`: `paired/`
contains downloaded WAVs, annotated `result.json` and `summary.csv`;
`provenance.json`, `validation.json`, `rigreal.log`, `ttstt-rig.log`, `collection.log`, `smoke/`,
`topology-evidence.json`, `topology-excerpt.txt` and `existing-tlb.txt` record
the deployment and placement. API tests are under
`TRUEOS/target/native-worker-preference/`. Blueprint compilation and test logs
are `TRUEOS-Blueprints/target/ttstt-native-{bp,unit-tests,regression-tests}.log`.

The one-shot diagnostic request was stored in
`apps/ttstt/benchmark.request.json`, consumed on explicit test launch and
preserved under its nonce. Results are in
`apps/ttstt/benchmarks/native3-paired-20261116/`. Completion is the
`result.json` field, not the consumed request filename. The service restored
three-worker mode before returning to normal `/health` and `/speech` handling.
The listener remains loopback-only.

To recollect this existing result after discovering its mounted root ID:

```sh
python3 apps/ttstt/tools/collect_native.py --root 2 \
  --nonce native3-paired-20261116 --output target/ttstt-native-collected
```

The collector is read-only and verifies hashes, operation counts, zero
fallbacks and the three-job peak. It does not start or reconfigure the rig.
