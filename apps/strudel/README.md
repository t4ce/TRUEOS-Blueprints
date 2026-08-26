# TRUEOS Strudel Core Blueprint — best-effort source archive

This archive wires the smallest useful Strudel-shaped system:

```text
persistent TRUEOS QuickJS
    → temporal Pattern.queryArc()
    → integer event rows
    → no_std Rust block renderer
    → trueos::audio::Stream
    → existing TRUEOS audio C ABI / PCM lane / Intel HDA
```

It deliberately omits the editor, browser, WebAudio, mini notation and transpiler.

## What is already real

The Rust app targets the existing Blueprint API, not a guessed HDA syscall. The inspected Blueprints tree already re-exports `v::vaudio` as `trueos::audio`, and its `Stream` exposes playback open/start/write, queued frames, buffer frames and drain controls for interleaved stereo `i16` at 48 kHz.

The JavaScript VM is the existing `trueos-qjs` persistent `Workbench`. The app embeds scripts with `include_str!`, evaluates them once, then only sends small integer pattern-query expressions through the VM.

The included emergency temporal kernel and demo are executable now. They prove the complete VM → scheduler → PCM path before upstream Strudel has been vendored. `tests/reference-demo-2s.wav` is a dependency-free host rendering of the same event and oscillator design for immediate audition.

## What is intentionally not claimed

`apps/strudel_core/js/vendor/strudel-core.bundle.js` starts as a placeholder. It is **not** secretly a copy of Strudel. Run the pinned vendor tool to replace it with a self-contained upstream bundle. Until then, the independently written fallback implements only `sequence`, nested subdivision, `stack`, `pure`, `silence`, `fast`, `slow`, `withValue` and `queryArc`.

No Cargo.toml is included, per request. The two dependencies needed by a new app are the same paths already used by the QJS Blueprint:

```toml
trueos-qjs = { path = "../qjs/crates/trueos-qjs", features = ["trueos"] }

[target.'cfg(any(target_os = "trueos", target_os = "zkvm"))'.dependencies]
trueos = { path = "../../api" }
```

## Drop-in steps

1. Copy `apps/strudel_core` into the root `apps/` directory of `TRUEOS-Blueprints`.
2. Add `"strudel_core"` to `apps.json`; `apps/strudel_core/patches/apps-json.patch` shows the insertion.
3. Add the two dependency lines above to the app manifest.
4. Build once with the placeholder bundle to validate QJS and HDA using the included fallback.
5. On a networked development host, vendor the real pattern engine:

```bash
cd tools
npm install --no-save esbuild @strudel/core@1.2.6 fraction.js@5.2.1 @kabelsalat/web@0.4.1
node vendor-strudel-core.mjs
node fallback-smoke.mjs
```

The vendor script imports `@strudel/core/pattern.mjs` directly, tree-shakes it into a non-minified IIFE, embeds `fraction.js`, pins the published `@kabelsalat/web` dependency without intentionally importing it, copies available upstream licenses, writes a SHA-256 lock, and runs the canonical `sequence("a", ["b", "c"])` smoke test.

## Composition surface

Edit only:

```text
apps/strudel_core/js/20_demo_pattern.js
```

Values accepted by the first renderer include:

```js
{ note: "c4", velocity: 100, wave: "triangle", pan: -0.2 }
```

`note` may be a MIDI number or a note name from C-1 through G9-ish; it is clamped to MIDI 0–127. Waveforms are `sine`, `square`, `saw`, `triangle`, and `noise`.

The adapter can consume either the real upstream Pattern or the fallback because it only assumes `queryArc(begin, end)` and the normal hap shape (`whole`, `part`, `value`).

## Why Rust renders the sound

At the “absolute minimum” level, Strudel is a temporal computation library. Keeping synthesis in Rust gives a narrow, inspectable boundary and immediately uses TRUEOS’s actual PCM queue. Later, a native synth, sample bank, or a QuickJS `trueos:audio` module can replace the renderer without changing the Pattern layer.

## Validation included

Run on a normal host:

```bash
./tools/check-archive.sh
```

This checks every JavaScript file, the canonical subdivision, cross-block event continuity, the generated oscillator tables, the offline vendoring-tool harness, and bit-for-bit regeneration of the two-second reference WAV.

See `TEST-REPORT.md`, `FACT_CHECK.md`, `docs/INTEGRATION_MAP.md`, and `docs/TODO_BARE_METAL.md` for the completed checks, inspected boundaries and remaining hardware tests.
