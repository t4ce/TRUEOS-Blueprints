# Shared preparation and complete residual branches

Both proposed approaches were implemented as Ubuntu experiments, using smol,
fixed pinned compute threads and bounded asynchronous request/reply channels.
The original complete model, English Heart, speed 1.0 and output arithmetic are
retained. There is no native publication, core-placement change or audio
transport redesign in this experiment.

## Comparison

Medians of two interleaved full-corpus passes from the same release binary:

| Configuration | Short | Medium | Long |
|---|---:|---:|---:|
| Serial | 1.942 s | 11.168 s | 35.093 s |
| Previous channel split, 3 workers | 1.471 s | 8.927 s | 29.198 s |
| Shared preparation, 3 workers | 1.179 s | 7.163 s | 22.613 s |
| Whole branches, 2 workers | 1.589 s | 9.170 s | 29.743 s |
| Whole branches, 3 workers | 1.469 s | 8.384 s | 27.312 s |

The shared cache is the strongest result: **35.6–39.3% less generation time
than serial**, and **19.8–22.6% less than the previous three-worker split**.
Three whole-branch workers reduce time by **22.2–24.9% versus serial**;
they are about equal to the previous split for the short passage and 6.1–6.5%
faster for medium/long. Two whole-branch workers are slower than three branch
workers and give 15.2–18.2% less time than serial.

| Long-passage configuration | Elapsed | Total process CPU time |
|---|---:|---:|
| Serial | 35.093 s | 35.050 CPU s |
| Previous channel split, 3 workers | 29.198 s | 50.734 CPU s |
| Shared preparation, 3 workers | 22.613 s | 30.689 CPU s |
| Whole branches, 2 workers | 29.743 s | 38.270 CPU s |
| Whole branches, 3 workers | 27.312 s | 39.085 CPU s |

Shared preparation reduces total CPU time as well as wall time: about 12.4%
less CPU time than serial and 39.5% less than the previous channel split for
the long passage. Its win includes eliminating repeated preparation, rather
than only putting more threads around unchanged work.

Long-passage elapsed ranges across the two passes:

- Serial: 34.753–35.434 s.
- Previous channel split, 3 workers: 28.674–29.722 s.
- Shared preparation, 3 workers: 22.413–22.813 s.
- Whole branches, 2 workers: 29.618–29.869 s.
- Whole branches, 3 workers: 27.069–27.556 s.

These are host measurements with two repetitions, not scheduling guarantees.
The earlier independent matrix below also showed a shared-cache gain, but
with different absolute timings. Host workload and clocks were not controlled;
compare configurations within their own interleaved matrix. The original model and
speaking durations remain unchanged. Both experiments were isolated; a
combination of branch scheduling and shared-cache scheduling was not tested.

## Shared preparation

The `shared` mode retains output-channel partitioning, but prepares one
immutable time-major u8 activation cache for each convolution. It includes
the original global quantization parameters and padding. Column sums are
computed once and reused to calculate every receptive-field sum once.
Workers read this shared data and execute the existing integer dot products
and float epilogue; they no longer independently quantize/gather the same
activation data or calculate the same patch sums. Output windows remain at
most 64 positions before an async yield.

This is a complete activation cache, bounded by the admitted tensor geometry;
it does not materialize a full im2col tensor. Preparation finishes before its
channel jobs are admitted. This tests shared preparation without adding an
overlapped tile-producer protocol.

An independent two-pass matrix, before the common graph build, also tested
two compute workers:

| Passage | Previous split, 2 workers | Shared, 2 workers | Extra time reduction |
|---|---:|---:|---:|
| short | 1.596 s | 1.300 s | 18.5% |
| medium | 9.250 s | 7.961 s | 13.9% |
| long | 30.227 s | 26.271 s | 13.1% |

In that matrix, three workers reduced long-passage time from 28.467 s to
24.023 s, a further 15.6%. All thirty WAVs matched the original oracle.
Cumulative owned activation/cache payload across the measured corpus fell
from 3,909,030,448 to 1,004,894,116 bytes. This is an allocation/copy payload
counter, not peak RSS or hardware memory-bus traffic. Cache preparation took
about two seconds across the entire corpus; the overall synthesis timings
include that cost.

## Complete residual branches

The `branches` mode parallelizes both MRF stages. All three original branches
run: two workers balance K3 then K7 against K11; three workers receive one
branch each. They include their normalization and elementwise processing as
well as convolutions. This covers 36 of the model's 87 convolution operations
per invocation. The other 51 remain serial. Channel/shared modes instead
distribute all 87 convolutions. Thus the table compares practical complete
configurations; the difference is not solely the partition size of one kernel.

The branch operation lists are bound to the pinned artifact SHA-256. Each
runner has a separate aligned arena, shape table, workspace and mutable CPU
execution state. Immutable prepared weights are shared through Arc. Only
live input slots are copied into the zeroed private arenas. Workers execute
the original operations in their branch order and yield between operators.
Individual convolutions within these workers remain serial, so nested
convolution workers cannot oversubscribe the requested compute CPUs.

The coordinator retains its original cursor. It replays shape declarations
and copies each completed endpoint at its original producer operation.
The original two Adds and Div therefore retain their operand and evaluation
order. A barrier prevents execution past a fork before its results exist.

The prototype retains full phase-one arena capacity for every private branch,
rather than repacking branch lifetimes. Allocation/zeroing and live-input
copying are included in end-to-end time and recorded separately. Cumulative
`private_arena_bytes` is not simultaneous memory use and excludes workspaces.
These allocations have not been validated against the native 2 GiB budget.
Cancellation takes effect between whole operators; dropping an awaited batch
cancels its owned work and a subsequent wait reports a terminal error.

## Measurement and validation

The common release binary runs on the Ubuntu i9-13900K: coordinator CPU 0;
compute CPUs 2, 8 and 10 are separate physical P cores. Every configuration
warms the model and packed weights before the fixed short/medium/long corpus.
Two interleaved passes reverse configuration order. The coordinator is an
additional thread/core beyond the listed compute-worker count.

Full synthesis wall time includes preparation, allocation, coordinator work,
awaited computation and collection of PCM; warmup and WAV-file writing are
excluded. Per-case Linux process CPU time sums all process threads over that
same interval. It excludes descheduling but includes memory stalls and runtime
overhead, and does not measure pure arithmetic, VNNI-unit occupancy or energy.
Summed `worker_us` is elapsed dispatch time and is a different metric.

Every measured WAV is hashed against the original pre-experiment serial
oracle, not merely against another experimental configuration. Branch runs
also require 16 completed MRF batches and 48 completed branches across the
eight model invocations in this corpus. Channel/shared runs require 696
completed convolution operations. Calculation notices are checked against
full elapsed synthesis so awaiting workers cannot disappear from the metric.

The combined feature suites passed 97 tests: 13 AOT, 20 dispatch, 11 executor,
8 memory, 34 CPU and 11 application tests. Tests cover exact shared-cache output
on scalar/AVX2/VNNI lanes, stride/dilation, batching, tails, bias and zero
points; invalid buffers/windows; async timer progress, bounded concurrent
work, cancellation, ownership release and pool reuse; and original producer
ordering. The ordinary library configuration also compiles.

## Reproduction

From the Blueprint repository:

```sh
cargo build --release --manifest-path apps/ttstt/Cargo.toml \
  --example async_convolution --features async-graph
python3 apps/ttstt/tools/benchmark_parallel.py \
  --binary target/x86_64-unknown-linux-gnu/release/examples/async_convolution \
  --output target/ttstt-parallel-followup/new-run \
  --variants serial:0,channels:3,shared:3,branches:2,branches:3 --repeats 2
```

Choose suitable distinct physical cores with `--coordinator` and
`--compute-cpus` on another host. `async-convolution` alone also builds the
shared-cache experiment. Both features remain optional and explicitly reject
a TRUEOS target.

Evidence is under `target/ttstt-parallel-followup/`: `final-paired/results.json`,
`shared-paired/results.json`, per-process logs/WAVs, `branch-smoke/`,
`combined-tests.log`, `shared-tests.log`, `branch-tests.log`,
`branch-default-check.log` and `host-topology.json`. The preserved
`parallel-bench` executable is the common comparison binary. None of these
host measurements establishes a native-rig speedup or a streaming-audio path.
