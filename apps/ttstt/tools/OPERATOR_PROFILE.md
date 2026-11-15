# Warm TTS operator profile — 2026-11-15

The dominant cost is the waveform generator's quantized 1D convolutions.
This is measured from the active optimized Blueprint, after model warmup.

## Native hardware measurements

The original capture's three requests reported an **efficiency core**, CPUID
core type `0x20`, APIC ID **50**, at both request boundaries. AVX-VNNI was
selected. At that point the VM lane picker used round-robin selection across
eligible APs without a core-type preference (`TRUEOS/src/hv/lane.rs`). The earlier native before/after
experiments did not control AP identity; their observed differences cannot be
fully attributed to individual code changes. The pinned host comparisons
remain separate evidence.

| Passage | Calculation | Audio duration | Quantized Conv share of operator time |
|---|---:|---:|---:|
| Short, 18 bytes | 7.592 s | 1.875 s | 64.88% |
| Medium, 133 bytes | 43.506 s | 10.675 s | 64.09% |
| Long, 460 bytes | 140.084 s | 33.975 s | 63.89% |

The long request's 139.029 seconds inside operator dispatch break down as:

| Family | Elapsed | Share |
|---|---:|---:|
| Quantized 1D convolutions | 88.823 s | 63.89% |
| Floating-point transposed convolutions / upsampling | 10.319 s | 7.42% |
| Bidirectional LSTM | 3.352 s | 2.41% |
| Floating-point Conv1d | 2.139 s | 1.54% |
| Quantized GEMM | 1.862 s | 1.34% |
| Resize | 1.070 s | 0.77% |
| Floating-point MatMul | 0.628 s | 0.45% |
| Remaining arithmetic, normalization and layout operations | 30.836 s | 22.18% |

Dispatch bookkeeping accounts for 0.167 seconds **within** those totals,
about 0.12%; it is not an additional row. Text processing took 29 ms and
chunk preparation 26 ms. PCM emission included 28.231 seconds of sink
backpressure, measured separately from calculation. Total request wall time
was 168.354 seconds. These are elapsed timings, not CPU-cycle accounting;
the native clock has approximately millisecond resolution in this capture.

The two highest-cost quantized nodes were generator
`resblocks.5/convs1.2` (op 2127, 6.564 s) and `noise_res.1/convs1.2`
(op 1572, 6.303 s). Both mix 128 input/output channels with kernel width 11
and dilation 5. They are dense vocoder convolutions, not text processing.

## Soft P-core placement comparison

The Blueprint now sets `vm-core-preference = "performance"` in package
metadata. Packaging carries this as TRBP flag `0x1000`. The kernel applies
the hint before spawning the VMX carrier, trying eligible P-core leases first,
then falling back to E/unknown cores. Other packages default to ordinary
selection. This is VM placement, independent of userspace thread scheduling.

After `cargo rigreal` and a fresh boot, the same warmed profiled service
selected **slot 9 / P core / APIC 33**, with no fallback. Every request reported
CPUID core type `0x40` at both boundaries and `core_changed=0`.

| Passage | E-core calculation | P-core calculation | Less calculation time | P-core request wall time |
|---|---:|---:|---:|---:|
| Short | 7.592 s | 4.988 s | 34.30% | 5.693 s |
| Medium | 43.506 s | 28.604 s | 34.25% | 37.026 s |
| Long | 140.084 s | 91.725 s | 34.52% | 120.018 s |

This is one pass per core type on the same rig and corpus, about **1.52x**
calculation throughput. Clearing only the new hint bit in the P-core package
reproduces the E-core package's SHA-256 exactly: executable, model and
compressed payload bytes are identical. Generated audio durations/frame
counts also match; this native probe does not capture PCM hashes.

On the P core, long-request quantized Conv1d still accounts for **55.048 s /
60.57%** of operator time. Floating-point transposed convolution takes
8.844 s / 9.73%. Sink backpressure adds 28.239 s outside calculation.
P-core placement helps, but calculation remains about 2.70 times audio duration.

[Intel's datasheet](https://cdrdv2-public.intel.com/655258/655258-011.pdf)
documents possible AVX frequency reductions. These elapsed measurements show
that core type matters here; no APERF/MPERF frequency counters were sampled,
so they do not isolate clock speed, throttling or instruction throughput.

The kernel's six carrier-selection regression tests cover preferred/free,
busy/absent P cores, tier fairness, existing default placement, and leases.
Three Blueprint tests cover metadata validation and actual flag serialization
without changing payload bytes. `cargo rigreal` and both normal/profiled
`cargo bp ttstt` builds passed and published through their normal workflows.

P-core evidence is in `/tmp/ttstt-pcore-profile/native/`, including
`records.log`, `comparison.json`, `profile-summary.json`, placement/warmup and
artifact metadata. The profiled hash is
`3a8b53810656742f78db0b688a5ffd213c54c311413036c1587750383fb8918d`.
The normal build has profiling disabled and retains the placement hint:
`19651313b7d5fd6759d2580448fdd5ebdca228ef1130aa68509bf30b25e83a8f`.

## What the convolution measurement includes

VNNI is active in the integer dot kernel. The reported convolution time also
includes activation quantization, gathering receptive fields, zero-point
correction and output scaling. The current counters do not separate those
internal costs.

Several hot dilated nodes miss the time-major quantization-cache path. The
adapter supplies `4 * input_channels * kernel_width` scratch bytes, while
the cached path requires `(kernel_width - 1) * dilation + 1 + 3 * stride`
source columns. For kernel 11 / dilation 5, this is 44 supplied versus 54
required columns. Those nodes gather and scalar-quantize four patches,
then execute AVX-VNNI dots. This is a concrete next profiling target; it
does not establish that gathering dominates the convolution time or that
enlarging scratch would improve it.

## Diagnostic scope and reproduction

The optional Cargo feature `profile-inference` records per-operation dispatch
and kernel times, job-stage totals, and actual core identity. It is excluded
from normal builds. Native reports use the structured global log; the Matrix
slot retains its short submitted/completed notices. Host reports use stderr.

Host builds can enable it with `cargo build --release --manifest-path
apps/ttstt/Cargo.toml --features profile-inference`. A temporary native build
uses the existing entry-source directive
`// trueos-blueprint: features = ["profile-inference"]` and normal `cargo bp
ttstt`; remove the directive and rebuild after measurement.

The host profile on a pinned i9 performance core attributed 57–62% of
operator time to quantized convolutions. All three profiled host WAV hashes
match the unprofiled baseline exactly. Host percentages are not substitutes
for native measurements; the CPUs and some scalar-math build features differ.

Native source data is in
`/tmp/ttstt-operator-profile/native/{records.log,results.json,profile-summary.json}`;
host profiles and WAVs are in `/tmp/ttstt-operator-profile/`. The profiled
Blueprint hash was `c294537ebb46bd547fed1744dc40f6aaa8bdd3ba45db83b2eaaab428f984f7c9`.
