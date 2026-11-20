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

The `trueos_tts` setting enables local read-aloud. Codex can submit a plain
opening sentence as soon as an item marked as a final answer starts streaming,
then submits the rest when the item completes. It also reads the first complete
sentence of a live reasoning summary when the model supplies one. History,
raw reasoning, tool output and realtime delegated voice replies are excluded.

Codex submits utterances in FIFO order. Waiting text above 1 KiB produces a
warning and stays queued. Pocket asks the local Lily service for the label of
each utterance before generating its audio.

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
seconds. A response is sent when generation and clip finalization complete.
Successful JSON includes `audio_ms`, `temperature`, `chunks`, `first_pcm_ms`,
`generation_ms` and `first_stream_ms`. The last field measures time from the
start of the request to the first successful native PCM write and is null on
Ubuntu or for silent input.

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

For an HTTP speech request without an explicit label, Pocket calls Lily's
`POST /v1/classify` on loopback for that utterance, with a 500 ms deadline.
If Lily is unavailable, Pocket uses the default temperature. An explicit HTTP
`label` takes precedence, and the direct CLI uses `--label` or the default.
`--rules` prints the policy. Labels match Lily's exact spelling and case.
Temperature controls sampling variation. The host auditions' external pitch,
tempo and gain effects are not part of this Blueprint.

## Native audio

The speech service converts each 24 kHz mono block to 48 kHz stereo signed
16-bit PCM and sends it as Pocket produces it. One mono sample is carried
across blocks to preserve interpolation. The first write reserves the native
sink. TRUEOS prebuffers 120 ms of audio to absorb generation jitter and then
feeds its live stream while the remainder is still generating. The complete
clip is available for WAV retrieval after generation finishes.

Listen through `http://TRUEOS-IP:82/`. The first generated PCM and first native
write timings under `blueprint/ptts` precede listener buffering and any quiet
model lead-in. The direct native `--sample` and `--text` commands still submit
a complete clip.

In a warm, idle native check, the same 4.24-second Lily Happy reply that
previously waited about 3.1 seconds for a complete clip reached the live
listener with nonzero PCM at 235 ms. The first 10 ms block above roughly
−60 dBFS arrived at 615 ms. Generation finished at 3,229 ms. Firefox
scheduled speech before the retained clip became available. These timings
measure delivery and scheduling rather than speaker output.

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
