# TTS CPU experiments — 2026-11-15

The selected default retains convolution preparation, prepared quantized/LSTM
weights and four-row VNNI GEMM. The combined native variant completed the
corpus faster than baseline; the lean variant without the weight cache was
slower and timed out on the long case. AP placement remains an uncontrolled
variable, so this does not establish the cause of the lean regression.

The fixed corpus is [perf_texts.json](perf_texts.json): short/medium/long contain
18/133/460 UTF-8 bytes and generate 1.875/10.675/33.975 seconds of audio.
`compute_ms` is elapsed work inside inference slices, excluding the audio or
WAV sink, playback drain and yielding between slices. It is not CPU-cycle
accounting. Process/request wall time includes additional work.

## Host results

Linux i9-13900K, CPU 0 affinity, AVX-VNNI, release host binaries writing WAVs.
Each utterance starts a fresh process. Baseline is the mean of two runs per
case; each subsequent exploratory variant has one run per case. These initial
runs were sequential, so small differences can reflect order or clock changes.

| Variant | Short compute ms | Medium compute ms | Long compute ms |
|---|---:|---:|---:|
| Baseline | 2,025.5 | 12,373 | 40,969 |
| Retained convolution input/validation | 1,833 | 10,560 | 34,696 |
| Above + prepared quantized/LSTM weights | 1,805 | 10,886 | 35,045 |
| Above + four-row VNNI GEMM | 1,758 | 10,644 | 35,000 |
| Lean: retained convolution + four-row GEMM, cache off | 1,811 | 10,693 | 34,793 |

Retained convolution alone reduced medium/long calculation time by
14.7%/15.3%. Adding the approximately 110 MB retained weight cache did not
improve those host measurements. Four-row GEMM is faster in its isolated
kernel benchmark, but the full pipeline remained close to convolution alone.
No whole-pipeline gain is established for either additional change here.

The final configuration was then compared against the saved baseline with
two pairs per case, alternating execution order on CPU 0. Every paired WAV
matched exactly, and both executable hashes stayed unchanged throughout:

| Case | Baseline mean compute ms | Final mean compute ms | Reduction |
|---|---:|---:|---:|
| Short | 2,081 | 1,708.5 | 17.9% |
| Medium | 12,440.5 | 10,233.5 | 17.7% |
| Long | 39,864.5 | 30,783.5 | 22.8% |

These repeated results establish the combined host improvement; they do not
separately attribute gains to each optimization. Detailed paired evidence is
`/tmp/ttstt-perf-experiment/final-paired/results.json`.

All saved host WAV SHA-256 values match across baseline repeats and variants:

| Case | WAV SHA-256 |
|---|---|
| Short | `091da6e08944ad69065b19c1fb8e755818eac390bc027d91803894665c9e5599` |
| Medium | `ab6579b1b17f6bb7619fad42180e27dfe86e19e3c5708916214022cf99c1b5ab` |
| Long | `55167877128099b9ec8acc368d27982ba8eb10f1c5f3d9941a6e228dec7b1552` |

## Native TRUEOS results

The finite `ttstt-bench` Blueprint sent the same corpus to the running loopback
service. The kernel stayed running; the service was replaced between variants.
Each service ran inference serially on one AP. AP identity across service
restarts was not confirmed, and these are single passes, not repeated trials.

| Case | Baseline compute ms | All three changes compute ms | Reduction | Baseline request ms | Candidate request ms |
|---|---:|---:|---:|---:|---:|
| Short | 6,115 | 5,252 | 14.1% | 7,773 | 6,390 |
| Medium | 36,167 | 29,893 | 17.3% | 47,801 | 40,889 |
| Long | 118,714 | 95,305 | 19.7% | 154,823 | 129,151 |

Baseline and combined-variant requests all succeeded. Their output frame
counts and audio durations match; native playback did not export WAVs, so
native sample hashes were not measured. The observed improvement applies to
the combined variant and does not isolate the weight cache or GEMM change.

The subsequent lean variant retained convolution preparation and four-row GEMM
but disabled the prepared weight cache:

| Case | Lean compute ms | Request result |
|---|---:|---|
| Short | 8,381 | Success, 9,113 ms wall |
| Medium | 47,497 | Success, 55,933 ms wall |
| Long | Incomplete | `speech-inference-timeout` |

The failed long request reported 180,693 ms elapsed in the service and
155,581 ms of calculation work. It emitted 29.500 seconds of audio instead of
the expected 33.975 seconds; this partial calculation time is not a completed
long-case measurement.

The default therefore restores all three changes, including **110,385,966
bytes** of prepared weights (about 105.3 MiB). This selects the configuration
that completed the native corpus with lower observed times. These single
passes cannot attribute the large lean regression specifically to removal of
the cache, because the AP identity across restarts was not confirmed. The
repeat native check of the restored final configuration was interrupted by a
rig reboot. The restored Blueprint had reported ready before that reboot;
the subsequent probe could not connect. That attempt is excluded from the
timing comparison. The reboot is visible in the capture at
`trueos-baremetal.2.log:539289` (`bsp-taskmon: seq=1 now_ms=2086`).

Source evidence: `/tmp/ttstt-perf-experiment/{baseline,conv-retained,prepared-weights,blocked-gemm,lean}.json`
and `native-{baseline,all,lean}/results.json`, with WAVs and native log extracts in
the same experiment directory. These are local session artifacts.

## Repeating a host comparison

Use existing binaries; this tool performs no builds or deployment:

```sh
python3 apps/ttstt/tools/benchmark_cpu.py \
  --baseline /path/to/baseline --candidate /path/to/candidate \
  --output-dir /tmp/ttstt-comparison --cpu 0 --repeats 3
```

The output directory must be empty. Runs alternate baseline/candidate order,
retain per-run logs and WAVs, and record binary/corpus hashes, calculation and
wall times, audio duration and exact paired WAV hashes in `results.json`.
Any process failure, missing calculation metric or hash mismatch fails the
comparison. Absolute host and TRUEOS times come from different machines and
runtime paths and should not be used to infer operating-system overhead.
