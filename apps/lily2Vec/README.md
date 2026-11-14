# lily2Vec — VNNI vibe service

Lily is an on-demand Blueprint service in its own TRUEOS VMX/Hull. Codex tees
recent assistant-response text to its loopback input queue. Lily's VM owns the
warm integer model, AVX-VNNI inference, and native Spirit signaling.

```text
Codex response stream ──→ terminal UI
                    └──→ snapshot mailbox ──→ Lily VMX
                                                 ├─ VNNI classification
                                                 └─ Spirit clip queue
```

The terminal path does not wait for inference or Spirit. Codex contains the
text sampler and asynchronous transport client; it does not contain the Lily
model, VNNI classifier, or calls that choose and signal avatar expressions.

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
must also support the reserved Spirit presentation vocabulary below. No app was
promoted to a buildin by this integration.

## Snapshot transport

The service accepts HTTP only on loopback:

- `GET /health` reports `protocol: "lily2vec-v1"`, `backend: "avx-vnni"`, and
  `ready: true` only after the model and required CPU features are ready.
- `POST /v1/snapshots` enqueues work and returns HTTP 202. Its JSON body contains
  `source`, monotonically increasing `sequence`, and `text`. A unique source
  identifies the initiating Codex presentation stream.
- `{ "source": "...", "sequence": 42, "clear": true }` advances the source
  sequence and discards that source's pending text.

The ingress is bounded to 32 source records, 8192 bytes per HTTP body, source
IDs of at most 128 bytes without whitespace/control characters, nonzero
sequence numbers, and nonempty snapshots of at most 4096 UTF-8 bytes
and 768 Unicode characters. Each source has one pending slot: a newer snapshot
replaces its unprocessed predecessor. Older/retried sequences are acknowledged
with `accepted: false` and `stale: true`; capacity pressure returns 429 rather
than growing an unbounded queue. Invalid input returns 400; oversized HTTP
bodies return 413.

The worker checks queued snapshots on a 50 ms tick and presents at most one
cue globally every 2.5 seconds. The same label is suppressed for five seconds.
Unprocessed snapshots expire after ten seconds; inactive source records expire
after 120 seconds. Cancellation clears pending text. These are bounded
presentation policies, not a new sprite-transition state machine.

Codex samples only live assistant text being sent to its terminal UI, including
its original Markdown. User input, reasoning, tool output, and replayed history
do not enter the tee. The sampler targets 384 characters and retains at most
768, preferring recent contiguous verbatim sections. Fast output coalesces into
recent text; it is not a backlog of every token or terminal redraw.

## Matrix activity output

Lily appends compact activity lines to its own ShellIO Matrix slot through the
existing log stream. It never claims a terminal UI or switches focus. Each
decoded snapshot request gets a service-wide counter and a UTC timestamp:

```text
lily2vec #1 17:49:02.123Z recv seq=1 chars=125 queued
lily2vec #1 17:49:02.125Z result=happy spirit=queued
```

The matching counter connects receipt to the single winning label. `result`
is exactly one of the ten supported words. `spirit=queued` confirms admission
to Spirit; `spirit=held-repeat` shows classification succeeded but the existing
repeat limit held the animation. Host dry-run uses `spirit=dry-run`. UTC times
have a `Z` suffix; an unavailable native wall clock is shown explicitly as
`time=unavailable`.

Receipt lines also show `clear`, `stale`, or `rejected` as appropriate. A queued
snapshot replaced by newer text or cleared before classification has no result
line. Input text is not printed. `/health` exposes `received`, counting requests
that reached JSON admission, including clears, stale sequences, and semantic
rejections. Malformed JSON and oversized bodies rejected before admission do
not increment it.

## Spirit signaling

Inside Lily's VM, the classifier scores the snapshot and calls
`trueos::spirit::play_expression(label)`. This crosses the native Spirit ABI
using a reserved `vibe:` word, then enters the existing clip queue. The kernel
maps the ten supported labels to already resident clips:

| Label | Existing clip |
| --- | --- |
| neutral | idle.crossed.neutral_breath |
| wave | wave.neutral |
| happy | happy.cheer |
| sad | cry.wipe |
| affection | flirt.heart_hands |
| disapprove | disapprove.head_shake |
| agree | agree.firm_nod |
| confused | confused.question |
| idea | idea.finger_up |
| angry | angry.fists_clenched |

Frame sequences, pose transitions, playback priorities, and idle behavior stay
with Spirit's existing sequencer. Ordinary unprefixed emotion requests retain
their existing randomized variants. Lily records successful native admission,
not proof that a GPU frame has reached the display.

## The integer model

`vibes.json` is the model: guided word/phrase associations, ten labels, signed
byte weights, and small biases. It is embedded and loaded once by the service.
No pretrained embedding download, Python runtime, GPU inference, SQLite model
storage, or main-agent tool call participates.

Text becomes unsigned byte features, currently zero or one for each guided
whole-word/phrase match. Each label receives a signed-byte weight row. The
classifier uses **VEX AVX-VNNI `VPDPBUSD`** to multiply and accumulate the rows,
adds each label's bias, then chooses the highest score. Neutral's small bias
handles unmatched text. Phrase guidance can counter word evidence: `not angry`
subtracts anger weight and adds neutral weight.

Feature rows are padded to 32 bytes. Each 256-bit instruction computes 32
unsigned-byte × signed-byte products into eight i32 accumulation lanes. VNNI
computes the actual scores selecting the label; it is not a demonstration
executed beside another classifier. The default matrix occupies only a few
kilobytes. The guide supports up to 4096 features and 256 labels, although the
native Spirit mapping currently exposes the ten above.

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

The host defaults to the original text-in, vibe-out CLI. Use Cargo from the
Blueprint repository so the target directory is unambiguous:

```sh
cargo run --release --manifest-path apps/lily2Vec/Cargo.toml -- "i love you"
# affection
cargo run --release --manifest-path apps/lily2Vec/Cargo.toml -- --scores "not angry, just explaining"
# neutral, with backend/scores on stderr
```

Without text arguments the host CLI accepts one line at a time until Ctrl-D.
`--guide FILE` loads adjusted associations. Production inputs need no training
step merely to edit this guide.

Run the host service without live Spirit side effects:

```sh
cargo run --release --manifest-path apps/lily2Vec/Cargo.toml -- \
  --serve --dry-run --port 9771
```

Dry-run still uses the production HTTP ingress, queue, cadence and VNNI
classifier. It substitutes only the final Spirit call and writes one JSON line
per presented classification to stdout:

```json
{"source":"example","sequence":1,"label":"affection","accepted":true}
```

Diagnostics go to stderr. Test it with Python's standard library:

```sh
cargo build --release --manifest-path apps/lily2Vec/Cargo.toml
python3 apps/lily2Vec/tests/service_http.py
# Or pass --binary /absolute/path/to/the/host/lily2vec
```

The HTTP harness checks actual VNNI results, latest-wins coalescing, stale
sequence rejection, cancellation, malformed/oversized ingress, and timestamped
receipt/result correlation including held repeats. It does not claim to verify
native Sprite/GPU display completion.

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


## Native verification for the separated service

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
