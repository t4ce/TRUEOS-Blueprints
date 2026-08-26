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
`attack`, `decay`, `sustain`, `release`, `lpf`, `lpq`, `lpenv`, `lpd`, `lpa`, `ftype`, `rarely`, `room`,
`shape`, `postgain`, `superimpose`, `delay`, `bpf`, `gain`, `mask`, and `bank`.
Control Patterns are sampled at the event onset and converted to the fixed
31-column `NativeRenderCommandV3` row. The V2 tail is integer-only and V3
appends `filter_type` (`0=12db`, `1=ladder`, `2=24db`):
`attack_frames`, `decay_frames`, `release_frames`, `filter_attack_frames`,
`filter_decay_frames`, `sustain_q15`, and signed `filter_env_octaves_q8`.
`release` extends the submitted render span while gate duration remains
separate. `lpf/lpq`, `room`, `delay`, `shape`, and oscillator selection map
directly; `bpf` uses its center as the available low-pass cutoff. `clip`,
`ftype`, and bank selection remain limited native metadata/control behavior.
`mask` filters events at their whole-event onset. Numeric mini masks support
`<0@4 1@16>` as four off steps followed by sixteen on steps, and Pattern-like
numeric controls are sampled at that same onset. `rarely(transform)`
deterministically selects roughly one in eight events per committed revision;
it is stable across lookahead blocks and has no global random stream. It
supports value-preserving transforms such as `add(note(12))`; transforms that
create, remove, or retime events are not yet equivalent to upstream.

`s("sawtooth")` selects the native saw oscillator. `bd`, `hh`, `sd`, `rim`,
and `rd` select deterministic synthesized percussion presets. They are not
sample recordings and do not download or decode a sample bank.

The current queue target is approximately 300 ms. That is deliberate: it keeps
the already proven gapless behavior while the editor path is introduced.

## Exposure

The HTTP service is an unauthenticated local-development endpoint. Submitting a
Pattern executes JavaScript inside the Blueprint QuickJS environment. Keep port
`1012` confined to the trusted TRUEOS host/network boundary.
