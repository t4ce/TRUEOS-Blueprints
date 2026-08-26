# Strudel vendor provenance

`strudel-core.bundle.js` is a generated, no-module IIFE for the target
QuickJS evaluator. It was built with `tools/vendor-strudel-core.mjs` from the
locked npm graph in `tools/strudel-upstream/package-lock.json`.

- `@strudel/core` 1.2.6, git head `0e26d4e741500f5bae35b023608f062a794905c2`
- `@strudel/mini` 1.2.6
- `@strudel/tonal` 1.2.6
- esbuild 0.28.2
- generated bundle SHA-256: `fe63a143f5caf42258105c5322105c132785b81c9405d6f1036c6264650c07cc`

The build aliases the packages' `@strudel/core` barrel to a small private
export shim. That retains the temporal Pattern API required by mini and tonal,
but excludes the core UI/clock modules. It intentionally does not import
Strudel's WebAudio/SuperDough/browser audio runtime.

## License

The Strudel source included in this generated work is licensed
AGPL-3.0-or-later. See the upstream source license at
<https://codeberg.org/uzu/strudel/src/branch/main/LICENSE> and the package
metadata locked above. Downstream distribution must comply with that license.

The transitive dependencies and their exact integrity hashes are recorded in
the committed npm lockfile.
