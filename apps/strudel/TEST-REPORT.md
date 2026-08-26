# Validation report

Run on 2026-08-26 in the archive-generation environment.

## Passed

`tools/check-archive.sh` completed successfully. It performs:

1. JavaScript syntax checking for every `.js` and `.mjs` file.
2. The canonical temporal subdivision check:
   `sequence("a", ["b", "c"])` → `a: 0–1/2`, `b: 1/2–3/4`, `c: 3/4–1`.
3. A 40-block scheduling regression over one full two-second cycle, checking exact
   onset, release, duration, block continuity and voice age.
4. A static audit of the generated MIDI phase table, sine table and the intended
   Workbench/audio integration markers.
5. An offline self-test of the vendoring pipeline using synthetic pinned packages,
   including bundle smoke, license copying, input-manifest generation and the
   guard that rejects browser/repl modules.
6. Bit-for-bit regeneration of the 48 kHz stereo reference WAV from the same
   temporal rows, waveform equations, envelope and panning rules used by Rust.

Observed regression summary:

```json
{
  "blocks": 40,
  "rows": 40,
  "spans": {
    "60": "0-48000",
    "61": "48000-72000",
    "62": "72000-96000"
  }
}
```

The lookup-table audit confirmed 128 exact MIDI phase increments and 256 exact
Q15 sine samples for 48 kHz rendering. The reference WAV is two seconds,
96,000 frames, stereo 16-bit PCM, with SHA-256
`96df96c82189dcb419869daa9a9f249d4a00373014c4a2923874b2cba0b51dd6`.

## Not run here

The Rust source was not compiled because this environment has no `rustc`,
`cargo`, TRUEOS target toolchain or full local TRUEOS/Blueprints checkout.
Consequently the final target-link and boot test remain unverified.

The generated upstream Strudel IIFE was also not produced here because npm
package installation did not complete in the network-restricted generation
environment. The checked-in placeholder is deliberate; the pinned vendor tool
and independent fallback are both included. Run `npm run vendor` on a networked
development host, then rerun `npm run check`.
