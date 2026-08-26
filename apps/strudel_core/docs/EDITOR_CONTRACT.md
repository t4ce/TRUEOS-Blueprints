# Browser editor contract

The web page is a transport and highlighting surface, not an audio client.

## Submit

```http
POST /api/strudel/submit
content-type: application/json

{"source":"sequence({ note: 'c4' }, { note: 'g4' })"}
```

`source` is a JavaScript pattern program whose final completion value is
Pattern-like and provides `queryArc`. Available pattern-engine exports are
installed in QuickJS global scope, so a namespace prefix is not required.
This permits ordinary Strudel form with a top-level tempo statement:

```js
setcps(1)
stack(note("c4").s("sawtooth"), s("bd*2"))
```

On success, the response contains the new state and revision. On evaluation
failure the server returns HTTP 422 with the error and unchanged active state.

## Ownership rules

- QuickJS and `trueos::audio::Stream` remain on the local engine task.
- Axum handlers send commands through a bounded channel and never hold the VM.
- Browser code never produces PCM, sets host time, or directly mutates audio.
- A rejected expression never replaces the active Pattern.
- Nested calls to the internal commit bridge are rejected before evaluation can replace playback.
- Normal commits never reset `absolute_frame` or flush queued PCM.
- Monaco assets are reused from `apps/monaco`; there is no second npm payload.

## Native shorthand layer

The temporal bundle is paired with a small native-audio compatibility layer:
`note`, `s`, `add`, `perlin`, `sine`, `cosine`, and `saw`, plus `clip`,
`release`, `lpf`, `lpq`, `lpenv`, `lpd`, `lpa`, `ftype`, `rarely`, `room`,
`shape`, `postgain`, `superimpose`, `delay`, `bpf`, `gain`, `mask`, and `bank`.
Control Patterns are sampled at the event onset and converted to the fixed
23-column `NativeRenderCommandV1` row. `lpf/lpq`, `room`, `delay`, `shape`,
and oscillator selection map directly. `bpf` uses its center as the available
low-pass cutoff. `clip`, `release`, `lpenv`, `lpd`, `lpa`, `ftype`, `mask`,
`bank`, and `rarely` are retained as deterministic metadata/no-ops because the
native v1 renderer has no corresponding field or random scheduler.

`s("sawtooth")` selects the native saw oscillator. `bd`, `hh`, `sd`, `rim`,
and `rd` select deterministic synthesized percussion presets. They are not
sample recordings and do not download or decode a sample bank.

The current queue target is approximately 300 ms. That is deliberate: it keeps
the already proven gapless behavior while the editor path is introduced.

## Exposure

The HTTP service is an unauthenticated local-development endpoint. Submitting a
Pattern executes JavaScript inside the Blueprint QuickJS environment. Keep port
`1012` confined to the trusted TRUEOS host/network boundary.
