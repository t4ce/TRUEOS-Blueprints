# ttstt: English + Heart

Self-contained Kokoro text to speech for the TRUEOS Codex integration test.
`ttstt.bp` embeds the native synthesis model, the English G2P frontend and
only the `af_heart` voice. Launch it and speak: no setup, downloader, model
folder, plugin or MCP is needed. STT remains unavailable.

The Blueprint borrows aligned static model bytes directly. It does not copy
or install models into TRUEOSFS. The large Misaki dictionary and the other
53 voices are excluded; English lexicalization/pronunciation uses the existing
G2P frontend. The unchanged neural model weights remain necessary.

From Codex, enable read-aloud with `codex -c tui.trueos_tts=true`. Completed
public final answers launch the on-demand Blueprint. History, reasoning, tool
output and realtime delegated voice replies are excluded. Pending speech can
be replaced or cleared; already playing speech finishes normally.

The local service listens at `127.0.0.1:9772`. `GET /health` advertises
`ttstt-v1`, `language: en`, `voice: af_heart`, `bundled: true`, `tts: true`,
and `stt: false`. `POST /speech` accepts:

```json
{"text":"Hello from TRUEOS.","speed":1.0}
```

Speed defaults to 1.0 and must be 0.5–2.0. Text is bounded to 16 KiB.
The optional legacy `voice` field accepts only `af_heart`; other voices are
rejected explicitly. Requests run serially with bounded PCM handoffs and
playback backpressure through the existing Blueprint audio ABI.

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
TRUEOS_BLUEPRINT_SKIP_APPS_PUBLISH=1 cargo bp ttstt
```

The embedded payload is 131,294,877 bytes before executable code and package
compression: 124,081,360 model bytes, 6,691,149 English G2P bytes and 522,368
Heart voice bytes. The full upstream voice archive is a build input only.
The archived kernel controller/capture/HWP files in `legacy/` are uncompiled
migration references.
