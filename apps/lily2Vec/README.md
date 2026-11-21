# lily2Vec — VNNI vibe service

Lily is an on-demand Blueprint service in its own TRUEOS VMX/Hull. Codex
forwards one bounded assistant reply per completed turn over loopback. Lily
runs the AVX-VNNI classifier and submits at most one contextual action to
Spirit. The kernel sequencer owns variant selection, pose transitions and
frame timing.

The turn classifier and default CLI use the Contextual row's 16 names.
`Static` is the fallback for text without a cue and leaves ambient playback
running. The weighted phrase guide and its existing emoji normalization
provide the initial mapping. Custom CLI guides still accept explicit labels.
The label-only lookup retains the vocabulary in `tokens.txt`.

```sh
cargo run --release --manifest-path apps/lily2Vec/Cargo.toml -- "hello"
# Wave
```

## TRUEOS deployment

From `TRUEOS-Blueprints`:

```sh
cargo bp lily2Vec
cargo bp codex
```

The resolver is case-insensitive; the registered package is `lily2vec` at
`apps/lily2Vec`, producing `dist/lily2vec.bp`. Codex's AppDB bundle requires
Lily alongside SQLite and xerj-service. Apps fetches, verifies and atomically
installs every required archive before Codex launches. A missing/invalid Lily
archive prevents a partial Codex installation. Installing Lily alone does not
request Codex.

Codex starts or reuses Lily through the native Blueprint launch path. Bare
launch of Lily on TRUEOS starts the service on `127.0.0.1:9771`, rather than an
interactive test loop. This is a separate VMX execution boundary, not a Codex
thread running the model. Multiple Codex sessions share the ready service. The service uses its own
current-thread Tokio runtime and LocalSet under normal Hull scheduling. No
whole-Hull P-core affinity policy is imposed by this change.

The AppDB bundle metadata is embedded in the seed, so bundle changes require
the normal kernel/builtins preparation and boot/update cycle. The updated kernel
must export `trueos_cabi_spirit_contextual_play_v1`.

## Completed-turn transport

- `GET /health` reports `protocol: "lily2vec-v2"` and `backend: "avx-vnni"`.
- `POST /v1/turns` accepts `source`, a nonzero increasing `sequence`, and
  `text`. Each admitted sequence returns HTTP 202 and one contextual label
  or `Static`. Retried or older sequences receive a stale acknowledgment.
- `POST /v1/classify` is the read-only cue lookup. It accepts nonempty text
  up to 16 KiB and returns a label in the existing voice-cue vocabulary.
- `POST /v1/snapshots` is deprecated. It emits a warning and enqueues no action.

Turns allow up to 768 Unicode characters and 4096 UTF-8 bytes. The service
tracks up to 32 sources and expires inactive source records after 120 seconds.

## Matrix activity output

Lily appends compact activity lines to its own ShellIO Matrix slot through the
existing log stream. It never claims a terminal UI or switches focus. Each admitted turn gets a service-wide counter and a UTC timestamp:

```text
lily2vec #1 17:49:02.125Z turn seq=1 result=Happy spirit=dry-run
```

The result is one contextual label or `Static`. `spirit=queued` confirms
admission to the kernel sequencer. `spirit=idle` marks a turn with no action.
Host dry-run uses `spirit=dry-run` for an action. Input text stays out of these
logs.

## Legacy Spirit signaling

`play_emotion` and `play_expression` retain their ABI so older clients can
run. They log a deprecation warning and schedule no frames. Completed turns
use `play_contextual` for the new sequencer.

## The integer model

`vibes.json` holds the guided word/phrase associations and `tokens.txt` lists
the 22 read-only lookup labels. The service embeds the guide and loads a
contextual projection alongside the original lookup model. Both use signed
byte weights and small biases.
No pretrained embedding download, Python runtime, GPU inference, SQLite model
storage, or main-agent tool call participates.

Text becomes unsigned byte features, currently zero or one for each guided
whole-word/phrase match. Each label receives a signed-byte weight row. The
classifier uses **VEX AVX-VNNI `VPDPBUSD`** to multiply and accumulate the rows,
adds each label's bias, then chooses the highest score. Static's small bias
handles unmatched text. Phrase guidance can counter word evidence: `not angry`
subtracts `Angry` weight and adds `Static` weight.

Feature rows are padded to 32 bytes. Each 256-bit instruction computes 32
unsigned-byte × signed-byte products into eight i32 accumulation lanes. VNNI
computes the actual scores selecting the label; it is not a demonstration
executed beside another classifier. The default matrix occupies only a few
kilobytes. Custom guides support up to 4096 features and 256 labels. Native playback
accepts the 16 contextual names.

The raw instruction wrappers are shared with the kernel in
`../../../TRUEOS/src/turbo/vnni.rs`. The build script emits a module reference
to that authored file, including during private SDK staging; it does not keep
a copied helper implementation. Set `LILY2VEC_VNNI_SOURCE` for another checkout
layout. All four AVX-VNNI families have 128/256-bit wrapping/saturating wrappers.
The original historical file was not recovered; this wrapper was reconstructed.

AVX2 + AVX-VNNI and OS-enabled XMM/YMM state are required. Unsupported CPUs do
not receive a scalar inference fallback or a ready service advertisement.
`score_reference` is for correctness tests and benchmarks only.

## Host CLI and service checks

The host defaults to the text-in, contextual-label-out CLI. Use Cargo from the
Blueprint repository so the target directory is unambiguous:

```sh
cargo run --release --manifest-path apps/lily2Vec/Cargo.toml -- "i love you"
# Flirt
cargo run --release --manifest-path apps/lily2Vec/Cargo.toml -- --scores "not angry, just explaining"
# Static, with backend/scores on stderr
```

Without text arguments the host CLI accepts one line at a time until Ctrl-D.
`--guide FILE` loads adjusted associations. Production inputs need no training
step merely to edit this guide.

Run the host service without live Spirit side effects:

```sh
cargo run --release --manifest-path apps/lily2Vec/Cargo.toml -- \
  --serve --dry-run --port 9771
```

Dry-run uses the production HTTP ingress, sequence deduplication and VNNI
classifier. It substitutes the final Spirit call and writes one JSON line
per accepted turn to stdout:

```json
{"source":"example","sequence":1,"label":"Flirt","accepted":true}
```

Diagnostics go to stderr. Test it with Python's standard library:

```sh
cargo build --release --manifest-path apps/lily2Vec/Cargo.toml
python3 apps/lily2Vec/tests/service_http.py
# Or pass --binary /absolute/path/to/the/host/lily2vec
```

The HTTP harness checks VNNI results, completed-turn deduplication, bounds
and legacy-signal warnings. GPU presentation is verified separately.

Additional focused checks:

```sh
cargo test --manifest-path apps/lily2Vec/Cargo.toml
cargo clippy --manifest-path apps/lily2Vec/Cargo.toml --all-targets -- -D warnings
cargo run --release --manifest-path apps/lily2Vec/Cargo.toml -- --bench 1000000
```

Earlier host classifier measurements on the i9-13900K found 215.4 ns for
reference scoring, 29.0 ns for VNNI scoring, and 1406.8 ns including text
features and label selection. Those are historical in-process measurements,
not end-to-end HTTP/VMX latency or throughput guarantees. Full-range signed
weights, unsigned features through 255, saturation/wrapping operations, and
integer-reference equivalence have dedicated hardware tests.

The earlier Potion embedding experiment remains behind the optional `potion`
feature, documented in `POTION.md`. Its assets and the separate `emomodel`
installer are not required by this service.


The host HTTP integration harness passed all five checks against the real
AVX-VNNI backend. Its final native Spirit submission was dry-run, so this does
not establish native VMX-to-sprite display completion or end-to-end latency.


## Historical native verification of the v1 service

Both native Blueprints built and published successfully:

- `lily2vec.bp`: `3ce8af56b123a0c39b2cddee5621ca24c5bbc058bd9fdd06fcc080d0d3c06a7b`
- `codex.bp`: `2765e3ec041b14ae3b3c78adf1d100a7badf35f783841fd9c879c1e3fe977495`

The service was launched over SSH on `192.168.178.94` and reported
`READY http://127.0.0.1:9771 protocol=lily2vec-v1 backend=avx-vnni` in its own
native VM. Startup keeps the warmup scoring result observable before readiness.
No kernel reboot or explicit core placement was performed. Physical sprite
playback from a new Codex response has not yet been observed in this check.

Final focused checks passed: classifier/shared VNNI tests, queue admission and
expiry tests, five HTTP tests using the real VNNI service through the native
socket-adapter pattern on the host (only Spirit dry-run), and seven production
Codex client sampling/transport/cancellation tests. Both focused Clippy runs
passed. Native loader/CABI compatibility checks passed for both packages.

The updated native Codex archive was also launched through `§§codex` on `.94`;
its terminal UI identified build `2765e3ec` and reached the normal idle prompt.
No AI prompt was submitted during this startup check.

The subsequent Matrix activity notification build (`0ad0e368`) was published
and reloaded on `.94`, again reaching native VNNI readiness. Six host HTTP
checks passed, including receipt/result correlation and visible repeat
suppression. Native request notification lines still await a fresh Codex reply;
only the Lily service was restarted for this update.
