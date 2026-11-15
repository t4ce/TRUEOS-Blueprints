# ttstt

On-demand local Kokoro text to speech for TRUEOS. This application Blueprint
has no MCP server or plugin registration. STT is unavailable and never loads
Whisper or opens a microphone.

The app owns the Kokoro crates in `crates/`, native inference, pronunciation
lexicon, G2P frontend and model residency. The kernel supplies its existing
filesystem, execution and PCM playback APIs. The former kernel controller,
capture diagnostics and HWP policy remain uncompiled in `legacy/`.

Build from the Blueprint repository (skip the packer's automatic publish):

```sh
cargo bp ttstt --check
TRUEOS_BLUEPRINT_SKIP_APPS_PUBLISH=1 cargo bp ttstt
```

Launch `ttstt` through Apps to start the service at `127.0.0.1:9772`. It binds
before allocating models, preventing duplicate resident sets. `GET /health`
returns protocol `ttstt-v1`, `tts: true` and `stt: false`. `POST /speech` accepts:

```json
{"text":"Hello from TRUEOS.","voice":"af_heart","speed":1.0}
```

Voice and speed are optional. Text is limited to 16 KiB of UTF-8; speed must
be 0.5–2.0. Requests execute serially and return after playback drains. PCM
transfers are bounded to 250 ms with playback backpressure. Inference yields
between its existing bounded native graph slices.

Provide these files under `/models/kokoro`, or pass `--model-dir DIR`:

- `kokoro.kkaot`: existing authenticated native artifact
- `voices-v1.0.bin`: pinned Kokoro voice archive
- `en.g2p`: English frontend model
- `misaki-us.klex`: pronunciation overlay

Model provenance, alignment and waveform acceptance checks are preserved.
Models are not bundled or downloaded automatically. Neither an ONNX fallback
nor Whisper is needed. `tools/` contains the native artifact and lexicon
compilers. Model installation is separate from executable installation.

Host waveform verification uses the same inference and PCM conversion:

```sh
cargo run --manifest-path apps/ttstt/Cargo.toml -- \
  --model-dir /path/to/kokoro --text 'Hello.' --wav /tmp/hello.wav
cargo test --manifest-path apps/ttstt/Cargo.toml --workspace
```

Live host playback is unavailable; use WAV output on the host.

Codex opts in with `tui.trueos_tts = true` (default off), and launches ttstt
on its first eligible completed public answer. It sends visible prose without
code blocks or link destinations; pronunciation and lexicalization stay here.
Historical turns, reasoning, tools and realtime delegated voice replies are
excluded. Pending answers can be replaced or cleared. Interruption of an
already playing request is a future protocol step: that request finishes
before the service accepts the next one.
