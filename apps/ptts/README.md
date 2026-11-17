# ptts: English Azelma speech sidekick

`ptts` is an on-demand Blueprint for Pocket TTS on the Rust/XN CPU backend. The
package embeds English Azelma's voice state and its matching JSON tokenizer,
which total 7,423,060 bytes before compression. It loads the 219,029,196-byte
`english_2026-10` checkpoint from app storage and keeps the model warm while the
service runs. Text tokenization, FlowLM generation and Mimi audio decoding run
on the CPU. See [tools/BUNDLE_SOURCES.md](tools/BUNDLE_SOURCES.md) for exact pins.

The runtime uses one calling CPU thread and requests P-core carrier placement
with `vm-core-preference = "performance"`, with normal fallback.

## Seed the model on TRUEOS

On the developer host, start the checkpoint server in the foreground.

```sh
node '/run/media/t4ce/Data 1,0 TB/TrueOSSeedFs/serve-ptts-model.cjs'
```

Start `ptts` on TRUEOS, either directly or through Codex. When the checkpoint is
missing, `ptts` fetches `http://192.168.178.111:8787/model.safetensors` using
bounded native HTTP. Set `PTTS_MODEL_URL` in the `ptts` launch environment to
use another seed address or port. The app checks the byte count and pinned
SHA-256 before committing the file to its own
`userdata/models/english_2026-10/model.safetensors` path.

After `ptts` logs that it saved the model, stop the server with Ctrl+C. Subsequent
launches reuse the saved file, including after a reboot with persistent TRUEOSFS
storage. An existing invalid file stops startup with an error and is left in
place for inspection.

The Blueprint is about 10.4 MB compressed. On the native rig, the first fetch,
save, and model load took 3.4 seconds after app entry. A later launch loaded the
saved checkpoint in 7.1 seconds with the seed server stopped and passed the
speech and audio handoff probe. These timings exclude Blueprint installation.

## Codex test integration

TRUEOS Codex uses `ptts` for read-aloud. Codex starts the speech Blueprint
headlessly at TUI startup or reuses a healthy service with protocol `ptts-v1`.
The normal `cargo bp codex-cli` bundle requires SQLite, Lily and `ptts.bp`.

The existing `trueos_tts` read-aloud boolean remains the switch for enabling
speech. Only completed public final answers enter this path. History, reasoning,
tool output and realtime delegated voice replies are excluded. Codex feeds one
FIFO in completion order, with no skip or reset controls or age expiry. When
waiting text exceeds 1 KiB in UTF-8, the producer logs a warning to keep replies
small. This is a soft threshold: queued text is retained without truncation.

Codex currently sends only the speech text. It does not attach live Lily labels
yet, so ordinary Codex requests use the default temperature of 0.3.

## Local service contract

Bare native launch, `ptts serve` or `ptts --serve` starts a warm service at
`127.0.0.1:9773`. It binds only loopback. `GET /health` reports `protocol:
"ptts-v1"`, `ready: true`, `tts: true`, `stt: false`, `language: "en"`,
`voice: "azelma"`, `model: "english_2026-10"` and `bundled: false` after loading.

`POST /speech` accepts JSON:

```json
{"text":"TRUEOS uses the cpu.","label":"Happy","voice":"azelma","speed":1.0}
```

Only `text` is required. Decoded text is bounded to 16 KiB of UTF-8 and must be
nonempty. `voice`, if supplied, must be `azelma`. `speed` defaults to 1.0 and only
1.0 is currently supported; other values fail explicitly. `label` is optional,
limited to 64 bytes, and selects the persisted temperature rules below.
Unknown JSON fields are rejected. HTTP framing requires a bounded content length;
chunked requests and duplicate content lengths are rejected.

One service owner executes requests serially, with no separate application
synthesis queue. The 180-second request-work deadline covers synthesis,
conversion and waiting for the native audio sink. Clips are limited to 120
seconds. A response is sent after complete-clip handoff, without waiting for
playback to finish. Successful JSON includes `audio_ms`, `temperature`, `chunks`,
`first_pcm_ms` and `generation_ms`.

The frontend applies speech-only, case-insensitive whole-word aliases:
`TRUEOS`/`TrueOS`/`trueos` become `True O S`, and `CPU`/`cpu` become `C P U`.
Words such as `CPU2` and `my_cpu` are preserved. English terminal punctuation,
sentence preparation, decimal handling and the upstream 50-token chunking target
are retained. An unpunctuated clause beyond the engine's 512-token bound fails
rather than being truncated. All chunks are prepared before inference.

Emoji sequences are omitted from speech. Emoji-only requests succeed silently
without submitting a clip. The original response remains unchanged for display
and Lily. The local emoji table uses
[Unicode data](tools/licenses/LICENSE-UNICODE).

## Temperature policy

[tone-rules.json](tone-rules.json) persists the selected mapping:

| Lily label | Pocket temperature |
| --- | ---: |
| Tired, Cry | 0.2 |
| Happy, Angry, Idea | 0.4 |
| Other, unknown or missing | 0.3 |

The current label producer is `lily2Vec`. Labels match its exact spelling and
case. An explicit HTTP `label` or CLI `--label` exercises this policy, and
`--rules` prints it. Live Lily-to-speech label handoff is not wired yet.
Temperature controls sampling variation. The host auditions' external pitch,
tempo and gain effects are not part of this Blueprint.

## Native audio

Generation emits 24 kHz mono PCM. The service collects the entire utterance,
converts it to 48 kHz stereo signed 16-bit PCM, waits for the current audio sink
to become idle within the same request deadline, then submits one complete clip.
This avoids overlap between jobs owned by this serial service. The current kernel
ABI cannot reserve the sink: another producer can still win between the idle
check and submission, whose success code can mean a busy discard. There is no
incremental audio producer in this test integration.

Listen through `http://TRUEOS-IP:82/`. Diagnostics report model loading,
first generated PCM, chunk counts, temperature, generation time and clip
submission under `blueprint/ptts`. Submission alone does not prove physical
speaker output; a new audio clip ID and exact-ID WAV retrieval provide the
runtime handoff evidence.

## Prepare, build and host checks

Prepare authenticated assets explicitly on the developer host:

```sh
python3 /home/t4ce/Repositories/TRUEOS-Blueprints/apps/ptts/tools/prepare_bundle.py
```

The host-only Python script downloads missing voice and tokenizer inputs under
ignored `models/` and verifies reused files. Cargo verifies those embedded inputs
again. For Ubuntu synthesis, run the script with `--with-model` to prepare the
checkpoint as well. Ubuntu loads `models/model.safetensors` in the `ptts` source
directory, or uses the file specified by `PTTS_MODEL_PATH`.

Run Ubuntu checks from `/tmp` with the absolute manifest path to avoid inheriting
the Blueprint repository's native `build-std` configuration:

```sh
cd /tmp
CARGO_TARGET_DIR=/home/t4ce/.cache/ptts-host-target cargo test --release \
  --manifest-path /home/t4ce/Repositories/TRUEOS-Blueprints/apps/ptts/Cargo.toml
CARGO_TARGET_DIR=/home/t4ce/.cache/ptts-host-target cargo run --release \
  --manifest-path /home/t4ce/Repositories/TRUEOS-Blueprints/apps/ptts/Cargo.toml -- \
  --text 'Hello from TRUEOS.' --wav /tmp/ptts-azelma.wav
```

Host service testing writes each successful utterance to a WAV directory instead
of attempting native playback:

```sh
ptts --serve --wav-dir /tmp/ptts-wavs --port 9773
```

`--port` is optional. On TRUEOS, bare launch starts the service; `--sample`
retains the original fixed-sentence runtime demo from [sample.txt](sample.txt).
`--text TEXT` can synthesize another sentence directly. `--wav FILE` and
`--wav-dir DIR` are host-only; native output uses the audio sink.

Package and publish normally from the Blueprint repository:

```sh
cargo bp ptts
cargo bp codex-cli
```

## Verification history for the embedded-model build

The original fixed-sample proof passed on the physical TRUEOS rig on 2026-11-17.
It downloaded through the normal Apps path after the archive-limit upgrade and
generated 4.24 seconds of audio on one CPU thread: 129 ms model load, first PCM
191 ms after synthesis began (322 ms after launch), and 3,038 ms generation.
These are historical single-run measurements, not a warm-service benchmark.

The sink clip ID advanced from 0 to 1, and the exact-ID download contained
203,520 stereo frames at 48 kHz without clipping. See
[tools/NATIVE_RUNTIME.json](tools/NATIVE_RUNTIME.json). That fixed-sample receipt
does not validate the subsequent arbitrary-text service or Codex swap.

The subsequent warm service with embedded weights passed native checks on
2026-11-17. Model, voice state and tokenizer initialization took 142 ms after
Blueprint invocation;
this excludes fetching and loading the Blueprint itself. The runtime selected
performance-core slot 9 with no fallback for this run.

| Native request | Temperature | Audio duration | Generation | First generated PCM |
| --- | ---: | ---: | ---: | ---: |
| Codex-shaped text only | 0.3 | 4.24 s | 3,120 ms | 167 ms |
| Happy | 0.4 | 5.60 s | 4,108 ms | 185 ms |
| Tired | 0.2 | 5.60 s | 4,083 ms | 168 ms |

Each spoken request advanced the sink clip ID. Emoji-only text stayed silent,
and an unsupported voice was rejected while the service remained ready. The
exact-ID native WAV was retrieved as 48 kHz stereo PCM. First generated PCM is
not playback-start latency: this adapter still submits complete utterances.
See [tools/SERVICE_RUNTIME.json](tools/SERVICE_RUNTIME.json) for the service
receipt.

The Pocket-only FIFO Codex build also passed an end-to-end native check. Starting
Codex cold launched `ptts` headlessly on performance-core slot 7 with no fallback;
the sidekick loaded in 144 ms. One controlled public final answer entered the
text-only speech API at temperature 0.3 and advanced the audio clip ID from 5 to
6. It produced 5.68 seconds of audio in 4,195 ms of generation, with first PCM at
223 ms and complete-clip handoff at 4,207 ms. The exact-ID 48 kHz stereo WAV was
retrieved without clipping. Six focused Codex contract tests cover the fixed
Pocket service, final-text filtering, FIFO order, de-duplication, byte accounting,
and preservation above the 1 KiB warning threshold.

The copied upstream software notice is
[tools/licenses/POCKET-TTS-MIT.txt](tools/licenses/POCKET-TTS-MIT.txt). Asset
provenance is recorded separately in [asset-sources.json](asset-sources.json);
the software license does not replace upstream model or voice terms.
