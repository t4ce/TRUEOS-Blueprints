# Where the three workers spend their time

The proposed shared-preparation and whole-branch experiments were subsequently
implemented and measured: [follow-up results](PARALLEL_FOLLOWUP.md). This page
records the preceding channel-split profile.

The workers are well occupied inside the dispatched convolution windows, but
those windows occupy only about 38% of the complete TTS request. Channel
partitioning also increases total convolution CPU time. Queue starvation is
not the main explanation for the modest whole-request speedup.

This investigation retains the complete original model and identical output.
It adds optional Ubuntu tracing, not a new scheduling algorithm or native port.

## What was measured

The optional `occupancy-trace` feature records per-thread intervals around
the numerical convolution call, separately from tile copying, allocation,
global quantization preparation, activation snapshots, arena commits and
coordinator dispatch. Per-thread buffers avoid a global trace lock between
workers. Each interval records monotonic wall time and Linux thread CPU time.
The graph remains on the pinned coordinator, with two/three pinned compute CPUs.

Kernel-region overlap is integrated from exact nanosecond endpoints. It is
not a sampled utilization estimate. A thread cannot have two overlapping
kernel intervals; all worker kernels must fall within an awaited operation.
The analysis validates both properties and the conservation identity between
the overlap histogram and summed kernel interval durations.

Thread CPU time excludes descheduling. It still includes memory/cache stalls,
gathering and quantization inside the kernel: these figures do not measure
VNNI execution-unit occupancy or arithmetic instruction counts. Hardware perf
counters were unavailable under the host's existing policy; it was not changed.

## Long-passage result

Means of two interleaved traced passes on the Ubuntu i9-13900K:

| Three compute workers | Elapsed | Fraction of whole request |
|---|---:|---:|
| Complete request | 24.664 s | 100% |
| Outside awaited convolution operations | 15.320 s | 62.1% |
| Awaited convolution operations | 9.344 s | 37.9% |
| All three workers inside numerical kernels | 8.420 s | 34.1% |
| Exactly two inside kernels | 0.338 s | 1.4% |
| Exactly one inside a kernel | 0.244 s | 1.0% |
| Awaited window with no worker inside a kernel | 0.342 s | 1.4% |

The last four rows partition the awaited operation window. All three overlap
for about **90.1% of that window**, averaging **2.80 active kernel workers**.
Their measured kernel CPU time is about **99.8%** of their summed kernel wall
time, so descheduling within these calls is small. Average queue-to-start
latency is about **135 microseconds**. The roughly 62% outside these windows
contains coordinator work; it does not mean the machine is globally idle.

The coordinator spends about **12.72 CPU seconds** in other operators on this
passage. The largest families include floating-point transposed convolutions,
Mul, Add, ReduceMean, Sin and LSTM. They currently remain serial.

## Extra CPU time in the channel split

| Convolution CPU time, including global preparation | Serial | One compute worker | Two | Three |
|---|---:|---:|---:|---:|
| Long passage | 17.906 s | 17.253 s | 22.346 s | 27.707 s |

The three-worker path uses about **55% more convolution CPU time** than the
serial path (about 61% more than the one-worker control). This is CPU time,
not proof of an exact extra instruction count: clocks and memory contention
can also affect it.

There is a concrete source of duplicated work in the implementation. Channel
shards each have a private patch cache and traverse the same input time range.
For each shard, the current f32 kernel gathers and quantizes those input
patches again. Output dot products are divided across workers, but activation
preparation inside those kernels is repeated. More channel workers therefore
do not divide a fixed amount of total CPU work evenly.

Global quantization preparation on the coordinator takes about **1.57 CPU
seconds**. For comparison, activation snapshot and arena commit copies take
about **0.27 and 0.24 CPU seconds** respectively. Result allocation/assembly
and worker-local copies have their own categories. These categories nest
inside dispatch/await spans; they must not all be added to the request wall time.

## Pipeline and other parallel boundaries

The first proposed experiment is a **shared activation/patch preparation
pipeline** within convolution. The current global quantization parameters and
rounding rules permit quantizing the activation once without changing its
bytes. A shared u8 activation is one-quarter the size of the f32 snapshot.
A producer could instead prepare bounded shared patch tiles once, feeding
channel workers that perform the dot products and unchanged epilogue. This
targets repeated work without changing graph arenas. It requires a new fused
u8/patch adapter, bounded ownership and exact-output tests; speedup is not yet
measured. A shared u8 activation alone still leaves byte gathering per shard.

There are also larger graph boundaries:

- Each MRF stage has seven shared input operations, three independent 86-op
  residual branches, then the original two Adds and Div. Whole-branch tasks
  would include normalization/elementwise work as well as six convolutions
  per branch. All three original branches must run; this is not pruning.
- Two APs can execute K3 then K7 on one side and K11 on the other. Their
  convolution MAC ratio is 3:7:11; complete branch costs also include unequal
  non-convolution work. Three one-branch tasks would have a tail imbalance.
- Noise branch 1 can overlap earlier decoder/MRF work: its input is ready
  at op 1344, its output finishes at 1621 and is only needed at join 1910.
- Independent utterance chunks could overlap, but require multiple invocation
  arenas, ordered emission and changes to shared warm-assets locking.

The existing arena reuses memory under the original serial lifetimes. For
example, ops 1716 and 1667 reuse one 2.5 MiB region at F128. Concurrent branches
cannot simply use those same storage bindings. They need private branch
storage or a newly verified concurrent lifetime plan, immutable shared inputs,
independent mutable execution state and an explicit original-order merge.
The audited branch storage totals about 37.5 MiB for stage 0 and 112.5 MiB for
stage 1 at F128; these would be additional allocations if the original full
arena were also retained. Neither graph alternative has been benchmarked.

## Evidence and reproduction

The original untraced 14–17% / 21–22% speed measurements remain in
[ASYNC_CONVOLUTION.md](ASYNC_CONVOLUTION.md). Instrumented runs are for occupancy
and CPU attribution. They are not substituted for the untraced speed table.
A separate trace-disabled control varied slower than the traced run, so an
instrumentation overhead correction cannot be isolated from host variability.

```sh
cargo build --release --manifest-path apps/ttstt/Cargo.toml \
  --example async_convolution --features occupancy-trace
python3 apps/ttstt/tools/benchmark_async.py \
  --binary target/x86_64-unknown-linux-gnu/release/examples/async_convolution \
  --output target/ttstt-async-convolution/occupancy/new-run --occupancy
python3 apps/ttstt/tools/analyze_occupancy.py \
  --input target/ttstt-async-convolution/occupancy/new-run/results.json \
  --output target/ttstt-async-convolution/occupancy/new-report
```

Optional `--plan` supplies compiler operation names for per-family/branch
attribution; overlap and thread-CPU calculations do not require it.

Evidence is under `target/ttstt-async-convolution/occupancy/`: the paired
benchmark, raw per-case traces, `report/index.html`, `report/analysis.json`,
`graph-analysis.json`, `analyze_graph.py`, test logs and the perf-policy check.
The interactive timeline is a binned overview; its numeric table uses exact
unbinned intervals. All generated WAVs are checked against the serial oracle.
