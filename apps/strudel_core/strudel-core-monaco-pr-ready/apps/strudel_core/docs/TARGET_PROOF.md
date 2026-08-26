# Target PCM proof

Capture analyzed: `blueprint-prehda.wav` (not committed).

- Container: RIFF/WAVE PCM s16le.
- Rate: 48,000 Hz.
- Channels: 2, interleaved stereo.
- Duration: 15.0 seconds / 720,000 frames.
- Whole-file SHA-256: `e7d9c7be59a49b1a6f978296681fe0b1cfa11d430ef6960998c0af7311e46e7f`.
- Peak samples: minimum `-20246`, maximum `20215`; absolute peak `20246`
  (`-4.182 dBFS`).
- Longest all-zero stereo-frame run: one frame (20.8 microseconds), occurring
  only at deterministic waveform/event boundaries; there is no buffer-sized or
  audible silent gap.

Every complete two-second region from 0–14 seconds has the same SHA-256 over the
decoded interleaved PCM bytes:

```text
90826a4e4e64924daf4f7d80230e035f36cc1134d4f9ef46cd0eb28e177dd62b
```

That proves the target produced the same two-second cycle seven consecutive
times through:

```text
persistent QuickJS
→ fallback Strudel-compatible Pattern/queryArc
→ integer event rows
→ Rust oscillator renderer
→ interleaved i16 PCM
→ Blueprint PCM lane
```

This is a pre-HDA capture. It establishes the app/VM/renderer/Blueprint-audio
path bit-for-bit; it does not by itself assert analog speaker output or codec
routing.

The Monaco/Axum change preserves the no-edit baseline exactly. The regenerated
two-second reference WAV remains bit-identical with SHA-256
`96df96c82189dcb419869daa9a9f249d4a00373014c4a2923874b2cba0b51dd6`.
