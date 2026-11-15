# Native TTS benchmark

This finite loopback client requires an already-running `ttstt` service.
It checks `/health`, then submits the same three speech requests as
`apps/ttstt/tools/perf_texts.json` and exits. It does not start or replace TTS.
Each request plays audio normally; the long case can take several minutes.

Build and publish through the standard Blueprint workflow:

```sh
cargo bp probes/ttstt-bench
```

Launch `§§ttstt-bench` in Shell3. Per-case wall times and errors appear in its
Matrix slot and the global log (`blueprint/ttstt-bench`). Match those records
with the service's submitted/completed job IDs for calculation timings.

The local corpus copy is required because source-overlay builds stage only
this package. Keep it identical to `apps/ttstt/tools/perf_texts.json`.
