# Fact check and confidence map

Inspected on 2026-08-26.

## High confidence: existing TRUEOS audio path

`TRUEOS/src/aud/cabi.rs` already provides a playback C ABI and routes interleaved stereo `i16` buffers into `aud::pcm_lane::submit_i16_stereo_48k`. It supports both host and hull-guest paths, bounded-queue `EBUSY`, start/drop/pause/state, queued frames, and monitor functions.

`TRUEOS-Blueprints/crates/trueos-v/src/vaudio.rs` wraps that ABI as `Stream`, and `TRUEOS-Blueprints/api/src/lib.rs` re-exports it as `trueos::audio`.

Therefore the archive calls the existing public Blueprint surface:

```rust
let stream = trueos::audio::Stream::open_playback(
    trueos::audio::PlaybackParams::s16le_stereo_48k()
)?;
stream.start()?;
stream.write_interleaved_i16(&samples)?;
```

No kernel audio patch is required for the first proof.

## High confidence: persistent QuickJS host

The QJS app already exposes `trueos_qjs::workbench::Workbench`. It creates a QuickJS runtime/context lazily, installs the TRUEOS Node-style module loader and globals, persists the context across evaluations, and has a `poll()` method for jobs/timers/async operations.

The new app reuses this API directly instead of duplicating raw QuickJS FFI.

## High confidence: minimal Strudel query contract

The current published `@strudel/core` version inspected was 1.2.6. Its documented minimal example is `sequence("a", ["b", "c"]).queryArc(0, 1)`. The core source implements `sequence` as `fastcat`, nested arrays as subdivisions, `stack`, and `Pattern.queryArc`.

The generated bundle entry imports `@strudel/core/pattern.mjs` directly. This is intentionally narrower than importing the package root.

## Medium confidence: upstream bundle under TRUEOS QuickJS

The upstream pattern slice depends on Fraction.js, whose 5.x build uses BigInt. QuickJS supports BigInt, and TRUEOS already builds QuickJS, but the generated bundle still needs an on-target startup test for:

- peak parse/compile memory;
- exact QuickJS revision compatibility;
- startup time;
- any unexpected browser/global side effect retained by tree shaking.

The fallback avoids Fraction.js and exists specifically to separate those questions from the PCM/audio proof.

## Medium confidence: sustained queue behavior

The wrapper exposes `queued_frames()` and returns `ERR_BUSY` for a full queue. The app handles both. The exact stable block size and lookahead should be tuned on the target HDA controller. Current defaults are 2,400 frames (50 ms) and 300 ms target queue.

## Licensing boundary

The scaffold and fallback are independently written. The generated upstream bundle is AGPL-3.0-or-later because Strudel is. The vendor tool copies upstream license files when present. A distributed image containing that generated bundle must preserve the relevant source and license obligations.
