#!/usr/bin/env bash
set -euo pipefail

TOOLCHAIN="${1:-nightly-x86_64-unknown-linux-gnu}"
SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
BLUEPRINT_ROOT="$(cd -- "$SCRIPT_DIR/.." && pwd)"
TRUEOS_REPO_ROOT="${TRUEOS_REPO_ROOT:-$BLUEPRINT_ROOT}"
RUST_SRC="$(rustup run "$TOOLCHAIN" rustc --print sysroot)/lib/rustlib/src/rust"
PATCH_FILE="$SCRIPT_DIR/trueos-nightly-rust-src.patch"
LIBC_SRC="$TRUEOS_REPO_ROOT/vendor/libc-0.2.186"
LIBC_DST="$RUST_SRC/library/vendor/libc-0.2.186"

if [ ! -d "$RUST_SRC/library/std/src" ]; then
    echo "rust-src not found for toolchain: $TOOLCHAIN" >&2
    echo "Try: rustup component add rust-src --toolchain $TOOLCHAIN" >&2
    exit 1
fi

if [ ! -d "$LIBC_SRC" ]; then
    echo "TRUEOS libc vendor source missing: $LIBC_SRC" >&2
    exit 1
fi

if patch --dry-run -N -p0 -d "$RUST_SRC" < "$PATCH_FILE" >/dev/null 2>&1; then
    patch -N -p0 -d "$RUST_SRC" < "$PATCH_FILE"
else
    echo "rust-src patch already applied or no longer matches exactly; continuing with libc pin."
fi

rm -rf "$LIBC_DST"
cp -a "$LIBC_SRC" "$LIBC_DST"

python3 - "$LIBC_DST" <<'PY'
import hashlib
import json
import os
import sys

root = sys.argv[1]
files = {}
for dirpath, dirnames, filenames in os.walk(root):
    dirnames.sort()
    for name in sorted(filenames):
        rel = os.path.relpath(os.path.join(dirpath, name), root)
        if rel == ".cargo-checksum.json":
            continue
        with open(os.path.join(root, rel), "rb") as fh:
            files[rel.replace(os.sep, "/")] = hashlib.sha256(fh.read()).hexdigest()

checksum = {
    "files": files,
    "package": "68ab91017fe16c622486840e4c83c9a37afeff978bd239b5293d61ece587de66",
}
with open(os.path.join(root, ".cargo-checksum.json"), "w", encoding="utf-8") as fh:
    json.dump(checksum, fh, sort_keys=True, separators=(",", ":"))
PY

(cd "$RUST_SRC/library" && rustup run "$TOOLCHAIN" cargo update -p libc --precise 0.2.186)

rm -rf "$SCRIPT_DIR/../target/trueos-blueprint/cargo-cache/x86_64-unknown-trueos"

echo "TRUEOS rust-src toolchain patch restored from $TRUEOS_REPO_ROOT for $TOOLCHAIN"
