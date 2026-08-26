# Browser editor contract

The web page is a transport and highlighting surface, not an audio client.

## Submit

```http
POST /api/strudel/submit
content-type: application/json

{"source":"sequence({ note: 'c4' }, { note: 'g4' })"}
```

`source` is one JavaScript expression whose value is Pattern-like and provides
`queryArc`. Available pattern-engine exports are installed in QuickJS global
scope, so a namespace prefix is not required.

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

The current queue target is approximately 300 ms. That is deliberate: it keeps
the already proven gapless behavior while the editor path is introduced.

## Exposure

The HTTP service is an unauthenticated local-development endpoint. Submitting a
Pattern executes JavaScript inside the Blueprint QuickJS environment. Keep port
`1012` confined to the trusted TRUEOS host/network boundary.
