# Ubuntu async convolution experiment

This experiment distributes the existing quantized 1D convolutions across
one, two or three compute workers. It retains all vocoder branches, Heart,
speed 1.0, the pinned model and the original integer/floating-point arithmetic.
The earlier quality-reduction audition was rejected after listening, including
the only borderline acceptable kernel-3+11 variant. No pruned graph is used.

## Work sharing

The optional `async-convolution` feature adds a host-only smol 2.0.2 executor
with a fixed number of pinned OS threads. The coordinator runs a separate
async driver. Both the work queue and each operation's reply channel have
capacity bounded by the worker count. There is no `unblock`, blocking receive,
thread join or nested `block_on` in the inference work-sharing path. Runtime
entry and pool startup/shutdown are outside that path.

The graph still honors its data dependencies. Dispatch stages an owned
convolution request and returns `DispatchResult::Pending`; the executor returns
`PendingWork` without committing the cursor or frame control. The job awaits
its operation through bounded async channels. Once all results are ready, the
same operation is retried, its complete output is copied to the arena, and the
normal cursor commit occurs once. A sync `synthesize` call with an installed
async pool is rejected rather than polling indefinitely.

Global activation quantization and bias preparation run once per operator.
Workers share an immutable activation snapshot and cached packed weights.
Each owns output and scratch buffers. Output-channel ranges do not overlap;
within a shard, the kernel computes at most 64 output positions before yielding
to the executor. A per-thread SIMD dispatcher follows the pinned OS thread,
so a future migrating between executor threads does not retain another core's
ISA choice. Results are reassembled in the original batch/channel/time order.

Dropping an operation waiter cancels its jobs at a window boundary. No guest or
arena pointer is sent to workers. Cancelled state reports a terminal error;
it cannot silently report readiness or resume a stale cursor. Tests check
snapshot release and subsequent reuse of the same pool.

This is an ownership-first prototype: copying activations and reassembling
outputs costs bandwidth and allocation. It is not a zero-copy native API.
The worker queue is bounded, but the complete model still requires its normal
activation arenas. The pool's immutable weight cache is shared across workers.

## Measurement

Calculation elapsed time includes parked time awaiting workers. Measuring
only coordinator polls would incorrectly hide the actual convolution cost.
The harness also measures full synthesis elapsed, worker tile elapsed, summed
worker elapsed, snapshot bytes and timer progress. Summed worker elapsed is not
CPU-cycle accounting. The benchmark checks calculation notices against full
synthesis elapsed and requires identical WAV hashes for every worker count
and repetition.

The coordinator yields between atomic operators in this feature. Remaining
serial operators, frontend/arena preparation and WAV formatting can still
delay its timer. Worker tile latency and coordinator timer gaps are reported
separately; this does not claim every TTS operation is nonblocking.

Use the async counters and full synthesis timings for this experiment. The
separate `profile-inference` counters currently measure synchronous dispatch
attempts; they exclude awaited work and count a pending operation's retry.
Combining those features does not give a complete parallel operator profile.

## Reproduction

From the Blueprint repository on Ubuntu:

```sh
cargo build --release --manifest-path apps/ttstt/Cargo.toml \
  --example async_convolution --features async-convolution
python3 apps/ttstt/tools/benchmark_async.py \
  --binary target/x86_64-unknown-linux-gnu/release/examples/async_convolution \
  --output target/ttstt-async-convolution/new-run \
  --coordinator 0 --compute-cpus 2,8,10 --repeats 2
```

Select CPU IDs appropriate to the machine. The measured workstation is an
i9-13900K: coordinator CPU 0 and compute CPUs 2, 8 and 10 are distinct physical
P cores. These are host measurements, not native i5-14500T results. The serial
control uses the same async driver with no installed compute pool; one worker
measures the ownership/scheduling overhead without parallel speedup. Two
interleaved passes reverse the order of the four configurations. Each process
warms the model and weight cache before running the fixed short/medium/long
corpus.

The feature explicitly rejects a TRUEOS target. Native placement, admission
and shared-memory ownership require a separate implementation and measurement;
this experiment performs no native publication or rig reset.

## Validation

The no-std numerical tests reconstruct complete outputs from two/three channel
shards and multiple time windows, including batch, tails, stride, dilation,
cache/fallback paths, bias, zero points and all available SIMD lanes. Invalid
ranges and short buffers leave outputs unchanged. Async tests cover exact
serial parity, timer/task progress, bounded queue pressure, cancellation,
release of owned snapshots and pool reuse. The executor test checks that a
pending resolver/output operation commits neither units nor control values
until its exact retry completes.

Measurements are saved under
`target/ttstt-async-convolution/final-paired/results.json`.

## Measured result — 2026-11-15

Two interleaved full-corpus passes, with warmed model and packed weights:

| Passage | Serial async driver | One compute worker | Two compute workers | Three compute workers |
|---|---:|---:|---:|---:|
| short | 1.583 s | 1.613 s | 1.315 s | 1.231 s |
| medium | 9.347 s | 9.592 s | 7.852 s | 7.306 s |
| long | 29.913 s | 31.113 s | 25.680 s | 23.761 s |

Two compute workers reduced calculation time by **14–17%**; three reduced it
by **21–22%**. One worker added **2–4%** over the serial control, measuring the
owned-copy/scheduling handoff without parallel benefit. This is whole-synthesis
elapsed calculation, not just a kernel microbenchmark.

All **24 WAV files** match each other per passage and the pre-experiment
serial oracle byte-for-byte. The full model and speaking duration are unchanged.
The async-feature suites passed 68 tests and the default suites passed 62.

The longest observed worker window was **3.039 ms**. The parallel coordinator's
largest observed timer gap was **108.405 ms**; remaining serial operations,
preparation and file formatting are still present. The result demonstrates
async work sharing for convolutions, not a fully latency-bounded TTS pipeline.
Worker timings are elapsed observations under the workstation workload, not
hard scheduling guarantees. These results do not establish native rig speeds.

The prototype still copies about 3.91 GB of activation data across the measured
corpus and retains separate owned outputs before arena commit. Copies and the
remaining serial operators are follow-up profiling targets. This test does not
identify their individual contributions to the remaining cost.

Evidence: `target/ttstt-async-convolution/final-paired/results.json`,
`summary.csv`, `validation.json`, `topology.json`, test logs and WAVs. The
recorded executable SHA-256 is
`ce0e917e17668d43fd005dc0877a45a4a365f9c783113e87acb1b3b8054e4049`.
