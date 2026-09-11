#!/usr/bin/env python3
"""Host codec roundtrips and frame-selection regressions, without VM imports."""
import json
from pathlib import Path
import subprocess
import tempfile

root = Path(__file__).resolve().parents[1]
with tempfile.TemporaryDirectory(prefix="img-convert-tests-") as temp:
    manifest = Path(temp) / "Cargo.toml"
    manifest.write_text(f'''[package]
name = "img-convert-tests"
version = "0.0.0"
edition = "2024"
[workspace]
[[test]]
name = "convert"
path = {json.dumps(str(root / "buildins/img/src/convert_tests.rs"))}
[dependencies]
miniz_oxide = {{ version = "0.9.1", default-features = false, features = ["with-alloc"] }}
crc32fast = {{ path = {json.dumps(str(root.parent / "TRUEOS/vendor/crc32fast-1.5.0"))}, default-features = false }}
jpeg-encoder = {{ version = "=0.7.1", default-features = false }}
png = "=0.18.1"
zune-jpeg = "=0.5.15"
zune-core = "=0.5.1"
''')
    subprocess.run(["cargo", "test", "--manifest-path", str(manifest)], cwd=root, check=True)
