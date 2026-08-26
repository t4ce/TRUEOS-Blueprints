# Integration map

```text
20_demo_pattern.js
        │
        ▼
real StrudelCore OR fallback compatibility kernel
        │ Pattern.queryArc(cycleBegin, cycleEnd)
        ▼
10_trueos_adapter.js
        │ integer rows:
        │ [start,end,age,duration,midi,velocity,wave,pan]
        ▼
trueos_qjs::workbench::Workbench
        │ EvalResult.text (JSON integer matrix)
        ▼
json_rows.rs
        │ Vec<RenderEvent>
        ▼
renderer.rs
        │ Vec<i16>, interleaved stereo, 48 kHz
        ▼
trueos::audio::Stream::write_interleaved_i16
        ▼
trueos_cabi_audio_write_i16_interleaved
        ▼
aud::pcm_lane
        ▼
Intel HDA
```

## Deliberate boundary choices

The VM never receives raw pointers and does not call audio directly. Rust owns scheduling lookahead, synthesis, clipping and the playback handle.

The VM returns frame-relative integers rather than Fraction.js objects. That makes the boundary deterministic, small and easy to fuzz.

Event phase is derived from `age_frames`, so a sustained hap crossing block boundaries does not reset oscillator phase.

The first synth is intentionally simple. The temporal engine and audio backend remain independently replaceable.
