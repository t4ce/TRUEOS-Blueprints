# `strudel_core`

`strudel_core` is the host-owned TRUEOS temporal pattern engine, PCM renderer,
and minimal browser editor.

The audio path stays entirely on the target:

```text
persistent QuickJS Pattern/queryArc
→ integer event rows
→ deterministic Rust oscillator renderer
→ interleaved 48 kHz stereo i16
→ TRUEOS Blueprint PCM lane
→ Intel HDA
```

The browser is intentionally “dead”: it serves Monaco JavaScript highlighting,
one expression buffer, a Submit button, and status text. It does not contain a
scheduler, WebAudio graph, oscillator, audio clock, or authoritative pattern
state.

## Core/UI split

`src/lib.rs` owns the UI-independent `StrudelCore` object: QuickJS, active
Pattern, continuously advancing frame clock, renderer, and `trueos::audio`
stream. `src/main.rs` and `src/server.rs` are an Axum shell around that core.
They pass source and snapshots through a bounded Tokio channel so the non-Send
QuickJS Workbench remains on one local owner task.

The server embeds the already proven Monaco files from
`apps/monaco/static/monaco/vs`; this app does not carry a second Monaco package
or asset tree. The default lifecycle HTTP port is `1012`.

The endpoint evaluates JavaScript in the Blueprint's QuickJS sandbox and is
intended as a local development surface. It has no authentication layer; do not
publish port `1012` onto an untrusted network.

## HTTP surface

- `GET /` — minimal Monaco expression editor.
- `GET /api/strudel/state` — current source, revision, runtime and audio queue
  geometry.
- `POST /api/strudel/submit` — `{ "source": "sequence(...)" }`.
- `GET /healthz` — service and engine state.

Ctrl-Enter and the Submit button call the same endpoint. A submission is one
JavaScript expression whose value implements `queryArc`.

```js
stack(
  sequence({ note: "c4", wave: "triangle" }, ["g4", "bb4"]),
  sequence({ note: "c2", wave: "square" }, null),
).fast(2)
```

Evaluation is transactional for the active Pattern. Syntax, runtime, or type
failure leaves the previous pattern sounding, does not reset the QuickJS VM,
does not rewind `absolute_frame`, and does not flush the PCM queue. The new
pattern therefore becomes audible after the already queued lookahead drains.

## Proven baseline

The initial expression still renders the exact two-second reference WAV:

```text
96df96c82189dcb419869daa9a9f249d4a00373014c4a2923874b2cba0b51dd6
```

The uploaded 15-second target pre-HDA capture contained seven identical complete
two-second decoded PCM regions with SHA-256
`90826a4e4e64924daf4f7d80230e035f36cc1134d4f9ef46cd0eb28e177dd62b`,
no buffer-sized silent gap, and a peak of -4.18 dBFS. See
`docs/TARGET_PROOF.md`.

`js/vendor/strudel-core.bundle.js` is a checked-in upstream Strudel 1.2.6
QuickJS IIFE covering temporal/core, mini notation, tonal transforms, and
voicing exports. It deliberately excludes WebAudio/SuperDough/browser audio;
the fallback remains available if the upstream bundle cannot load. Rebuild and
license/provenance details are in `js/vendor/`.
