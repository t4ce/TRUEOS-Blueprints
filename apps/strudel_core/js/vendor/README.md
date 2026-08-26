# Generated vendor area

`strudel-core.bundle.js` is the checked-in upstream Strudel 1.2.6 temporal,
mini, tonal, and voicing bundle. It is an IIFE so QuickJS can evaluate it
without module loading.

Rebuild it with `node tools/vendor-strudel-core.mjs`. The reproducible npm
inputs are in `tools/strudel-upstream/package.json` and `package-lock.json`.
The build fails if browser audio symbols enter the output. See `PROVENANCE.md`
for upstream versions and licensing.
