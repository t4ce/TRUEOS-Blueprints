# strudel_core: add a host-owned Monaco live editor

## Summary

- split the persistent QuickJS/audio path into a UI-independent `#![no_std]` `StrudelCore` library
- add a small Axum server on lifecycle port 1012
- reuse the checked-in Monaco assets from `apps/monaco/static/monaco/vs`
- add a deliberately non-authoritative browser UI: JavaScript highlighting, one expression buffer, Submit/Ctrl-Enter, and host status
- serialize all VM/audio access through a bounded local Tokio channel so the non-Send QuickJS Workbench keeps one owner task
- make pattern replacement transactional: syntax/runtime/type errors and nested commits leave the previous Pattern, frame clock, and queued PCM active
- preserve the existing 50 ms renderer blocks and approximately 300 ms host PCM lookahead
- add the app to the root workspace so it uses the repository's patched Axum/Tokio/TRUEOS dependency graph

## Ownership boundary

```text
Monaco page
  -> POST /api/strudel/submit { source }
  -> bounded command channel
  -> local StrudelCore owner task
  -> persistent QuickJS Pattern/queryArc
  -> deterministic Rust renderer
  -> TRUEOS audio::Stream / PCM lane
  -> Intel HDA
```

The browser contains no WebAudio graph, scheduler, oscillator, audio clock, or authoritative Pattern state.

## HTTP surface

- `GET /`
- `GET /api/strudel/state`
- `POST /api/strudel/submit`
- `GET /healthz`

The endpoint is an unauthenticated local-development surface and should not be exposed to an untrusted network.

## Validation

Passed locally:

- JavaScript syntax checks for fallback core, adapter, demo expression, frontend, and smoke test
- live-expression commit smoke test
- invalid Pattern commit preserves active query output and revision
- nested commit attempt is rejected and preserves active query output and revision
- HTML ID/frontend binding checks
- TOML parse and CSS/static text checks
- unified patch whitespace and clean-apply checks
- regenerated two-second WAV is bit-for-bit identical to the existing reference:
  `96df96c82189dcb419869daa9a9f249d4a00373014c4a2923874b2cba0b51dd6`

Target evidence retained in the docs:

- uploaded pre-HDA target capture: 15 seconds, PCM16 stereo 48 kHz
- seven complete identical two-second PCM regions:
  `90826a4e4e64924daf4f7d80230e035f36cc1134d4f9ef46cd0eb28e177dd62b`
- no buffer-sized silent gaps; peak -4.182 dBFS

A Rust/TRUEOS build was not available in the execution container, so this PR intentionally relies on repository analysis/CI for the target compile.
