#!/usr/bin/env python3
"""Check Games carries the approved cube stages through its real ELF binding."""
from pathlib import Path
import hashlib
import json
import re
import struct
import subprocess
import tempfile


def main():
    app = Path(__file__).resolve().parents[1]
    repos = app.parents[2]
    kernel = repos / "TRUEOS"
    payload = (app / "assets/cube-patch.apshader").read_bytes()
    assert payload[:8] == b"APSHDR1\0"
    identity, size, count = struct.unpack_from("<III", payload, 8)
    assert identity == 4 and size + 16 == len(payload) and count == 4
    cursor = 20
    for _ in range(count):
        kind, length = struct.unpack_from("<II", payload, cursor)
        assert kind == 1 and length > 0 and length % 4 == 0
        cursor += 8 + length
        assert cursor <= len(payload)
    assert cursor == len(payload)
    trust = (kernel / "src/gpu/app_shader_trust.rs").read_text()
    expected_size, expected_digest = re.search(r"\(4, (\d+), \[([^\]]+)\]\)", trust).groups()
    digest = bytes(int(value.strip(), 16) for value in expected_digest.split(","))
    assert len(payload) == int(expected_size) and hashlib.sha256(payload).digest() == digest
    canonical = repos / "Cubes/shaders/trueos/cube-patch.apshader"
    if canonical.is_file():
        assert payload == canonical.read_bytes()
    with tempfile.TemporaryDirectory(prefix="games-shader-binding-") as directory:
        directory = Path(directory)
        source = directory / "binding.rs"
        source.write_text('#![no_std]\n#[path = ' + json.dumps(str(app / "src/app_shader_packages.rs")) + ']\nmod shader;\n')
        obj = directory / "binding.o"
        subprocess.run([
            "rustc", "--edition=2024", "--crate-type=lib", "--emit=obj", "-O",
            "--cfg", 'target_os="trueos"', "-Aexplicit_builtin_cfgs_in_flags",
            str(source), "-o", str(obj),
        ], check=True)
        output = directory / "shader.bin"
        subprocess.run(["objcopy", "--dump-section", f".trueos.shaders={output}", str(obj)], check=True)
        assert output.read_bytes() == payload
    print(f"Games cube shader id={identity}: four nonempty stages, trusted SHA-256, real ELF section preserved")


if __name__ == "__main__":
    main()
