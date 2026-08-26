#!/usr/bin/env sh
set -eu

TOOL_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
TMP_DIR=$(mktemp -d)
trap 'rm -rf "$TMP_DIR"' EXIT HUP INT TERM
NODE_MODULES="$TMP_DIR/node_modules"
OUTPUT_DIR="$TMP_DIR/output"

mkdir -p \
  "$NODE_MODULES/@strudel/core" \
  "$NODE_MODULES/fraction.js" \
  "$NODE_MODULES/@kabelsalat/web" \
  "$NODE_MODULES/esbuild/lib" \
  "$OUTPUT_DIR"

printf '%s\n' '{"name":"@strudel/core","version":"1.2.6"}' \
  > "$NODE_MODULES/@strudel/core/package.json"
printf '%s\n' 'AGPL test fixture' > "$NODE_MODULES/@strudel/core/LICENSE"
printf '%s\n' '{"name":"fraction.js","version":"5.2.1"}' \
  > "$NODE_MODULES/fraction.js/package.json"
printf '%s\n' 'MIT test fixture' > "$NODE_MODULES/fraction.js/LICENSE"
printf '%s\n' '{"name":"@kabelsalat/web","version":"0.4.1"}' \
  > "$NODE_MODULES/@kabelsalat/web/package.json"

cat > "$NODE_MODULES/esbuild/lib/main.js" <<'JS'
const fs = require('fs');

exports.build = async function build(options) {
  fs.writeFileSync(
    options.outfile,
    `(function (G) {
      class Pattern {
        queryArc() {
          return [
            { value: 'a', whole: { begin: 0, end: 0.5 } },
            { value: 'b', whole: { begin: 0.5, end: 0.75 } },
            { value: 'c', whole: { begin: 0.75, end: 1 } }
          ];
        }
      }
      G.StrudelCore = {
        Pattern,
        pure() {},
        silence: {},
        sequence() { return new Pattern(); },
        seq() { return new Pattern(); },
        fastcat() { return new Pattern(); },
        slowcat() { return new Pattern(); },
        stack() { return new Pattern(); }
      };
    })(globalThis);\n`,
  );
  return {
    metafile: {
      inputs: {
        'node_modules/@strudel/core/pattern.mjs': {},
        'node_modules/fraction.js/dist/fraction.mjs': {}
      }
    }
  };
};
JS

node "$TOOL_DIR/vendor-strudel-core.mjs" \
  --node-modules "$NODE_MODULES" \
  --output "$OUTPUT_DIR/strudel-core.bundle.js" >/dev/null

node --check "$OUTPUT_DIR/strudel-core.bundle.js" >/dev/null
test -f "$OUTPUT_DIR/vendor-lock.json"
test -f "$OUTPUT_DIR/vendor-inputs.json"
test -f "$OUTPUT_DIR/licenses/Strudel-AGPL-3.0-or-later.txt"
test -f "$OUTPUT_DIR/licenses/fraction.js-MIT.txt"
node -e '
  const fs = require("fs");
  const lock = JSON.parse(fs.readFileSync(process.argv[1], "utf8"));
  if (lock.kabelsalat.actualInBundle !== false) process.exit(1);
  if (lock.bundledInputs !== 2) process.exit(1);
' "$OUTPUT_DIR/vendor-lock.json"

printf '%s\n' '{"vendorToolSelfTest":"passed"}'
