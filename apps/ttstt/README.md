# ttstt: English + Heart

Self-contained Kokoro text to speech for the TRUEOS Codex integration test.
`ttstt.bp` embeds the native synthesis model, the English G2P frontend and
only the `af_heart` voice. Launch it and speak: no setup, downloader, model
folder, plugin or MCP is needed. STT remains unavailable.

The Blueprint borrows aligned static model bytes directly. It does not copy
or install models into TRUEOSFS. The large Misaki dictionary and the other
53 voices are excluded; English lexicalization/pronunciation uses the existing
G2P frontend. The unchanged neural model weights remain necessary.

The standard TRUEOS Codex build enables read-aloud and starts this Blueprint
at TUI startup, reusing a healthy existing service. No launch arguments are
needed. The normal `cargo bp codex-cli` build includes SQLite, Lily and this
TTS Blueprint. Completed public final answers enter the speech service.
History, reasoning, tool output and realtime delegated voice replies are excluded. Pending speech can
be replaced or cleared; already playing speech finishes normally.

The local service listens at `127.0.0.1:9772`. `GET /health` advertises
`ttstt-v1`, `language: en`, `voice: af_heart`, `bundled: true`, `tts: true`,
and `stt: false`. `POST /speech` accepts:

```json
{"text":"Hello from TRUEOS.","speed":1.0}
```

Speed defaults to 1.0 and must be 0.5–2.0. Text is bounded to 16 KiB.
The optional legacy `voice` field accepts only `af_heart`; other voices are
rejected explicitly. Requests run serially, collecting each complete utterance before one kernel
HTTP-sink handoff.
Synthesis finishes before playback starts. Listen at `http://TRUEOS-IP:82/`;
`POST /speech` returns after the clip is copied, without waiting for playback.
The clip limit is 120 seconds (23,040,000 bytes of stereo s16le/48 kHz PCM);
longer utterances fail without publishing a partial clip. The first active clip
plays to completion. Concurrent handoffs succeed but are discarded, with no
mixing or waiting queue. No microphone or audio-upload HTTP route is exposed.

For host testing, the same embedded backend can write a WAV:

```sh
cargo run --release --manifest-path apps/ttstt/Cargo.toml -- \
  --text 'Hello from TRUEOS.' --wav /tmp/heart.wav
```

Host live playback is unavailable. `ttstt --licenses` prints the bundled
model provenance and third-party notices.

## Building

Build inputs are prepared once on the developer's host, not on TRUEOS.
See [tools/BUNDLE_SOURCES.md](tools/BUNDLE_SOURCES.md) for the pinned upstream
sources and reproducible preparation command. Cargo verifies every input and
extracts Heart from the authenticated upstream voice archive at build time.
Missing or mismatched inputs fail the build; Cargo never downloads assets.

After preparing `models/kokoro/`, build from the Blueprint repository:

```sh
cargo test --manifest-path apps/ttstt/Cargo.toml -p ttstt -p trueos-kokoro-voice
cargo bp ttstt
```

The embedded payload is 131,294,877 bytes before executable code and package
compression: 124,081,360 model bytes, 6,691,149 English G2P bytes and 522,368
Heart voice bytes. The full upstream voice archive is a build input only.
The archived kernel controller/capture/HWP files in `legacy/` are uncompiled
migration references.

## Runtime diagnostics

The Matrix slot shows a short ready notice, then two lines per speech job:

```text
ttstt: job=1 submitted bytes=29
ttstt: job=1 done compute_ms=6370
```

`compute_ms` measures elapsed calculation work inside inference slices,
excluding PCM collection, clip handoff and yielding between slices.
Failures remain visible. Text contents are not logged.

Detailed startup and timing counters stay in the structured global log
(`blueprint/ttstt`). Sparse global summaries use
the Important level so the rig's Apps=Warn filter retains them. Warmup reports
the selected quantized SIMD lane and elapsed time. Synthesis reports time to
first PCM emission to the collector, work inside inference slices excluding the audio
collector, collection time, audio duration, frame/slice counts and status; HTTP
playback also reports completed-clip handoff and total request time.

The kernel uses its ordinary Blueprint memory admission policy; TTSTT has no
name-specific speech or audio memory exception.

## CPU performance

The Ubuntu-only [async convolution experiment](tools/ASYNC_CONVOLUTION.md)
uses smol with bounded compute workers and awaited operator completion. It is
opt-in and does not enable native worker placement. The quality audition's
pruned vocoder variants were rejected after listening; the full three-branch
Heart graph remains the reference for this experiment.

The [shared preparation and whole-branch follow-up](tools/PARALLEL_FOLLOWUP.md)
compares a shared quantized activation cache with complete residual branches
on private arenas. Both use the same host async driver and preserve the full
model. They remain optional Ubuntu experiments.

The package requests a performance core for the VMX carrier with
`vm-core-preference = "performance"` in its Blueprint metadata. The kernel
tries eligible P cores first and falls back to another eligible core when
none is available. This applies during VM admission, before the carrier task
is spawned, and needs no launch arguments. Other packages retain their usual
placement unless they opt in. See [the metadata contract](../../BLUEPRINT_METADATA.md).

The normal build enables retained convolution validation, prepared constant
weights (including the packed AVX2 LSTM layout), and four-row AVX-VNNI GEMM.
Convolution owns an immutable input snapshot until its operation completes;
the existing numerical checks and cooperative scheduling boundaries remain.
Prepared weights use 110,385,966 additional resident bytes for the pinned model,
within the existing 2 GiB VM budget; the embedded model bundle is unchanged.

See [the experiment results](tools/PERFORMANCE.md) and
[the host comparator](tools/benchmark_cpu.py) for exact-output checks and
reproduction. The Cargo features `retained-convolution`, `prepared-weights`
and `blocked-gemm` permit separate host experiments; all three are enabled by
default. Codex and TRUEOS launches need no additional arguments.

The [warm operator profile](tools/OPERATOR_PROFILE.md) identifies the dominant
generation costs and records actual core placement. Its optional
`profile-inference` diagnostic is disabled in normal builds.
