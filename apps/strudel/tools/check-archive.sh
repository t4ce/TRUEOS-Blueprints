#!/usr/bin/env sh
set -eu
cd "$(dirname "$0")/.."

find apps/strudel_core/js tools -type f \( -name '*.js' -o -name '*.mjs' \) -print \
  | sort \
  | while IFS= read -r file; do node --check "$file" >/dev/null; done

node tools/fallback-smoke.mjs
node tools/regression-smoke.mjs
node tools/static-audit.mjs
sh tools/vendor-tool-selftest.sh

REFERENCE_TMP=$(mktemp)
trap 'rm -f "$REFERENCE_TMP"' EXIT HUP INT TERM
node tools/render-reference.mjs "$REFERENCE_TMP"
cmp "$REFERENCE_TMP" tests/reference-demo-2s.wav
