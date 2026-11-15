#!/usr/bin/env python3
"""Prepare the ignored, pinned assets used to build the self-contained TTS Blueprint.

Host only: requires numpy and onnx. Downloads never run in the Blueprint.
Every source and generated runtime file must match its fixed byte count/hash.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import lzma
from pathlib import Path
import subprocess
import sys
import tempfile
import urllib.request
import zipfile

APP = Path(__file__).resolve().parents[1]
TOOLS = APP / "tools"
PINS = {
    "kokoro.kkaot": (124_081_360, "b7d4b9c62f3df01f71fc2585acd60190d1809dae9a6d916fe39c86d7dd4e3217"),
    "voices-v1.0.bin": (28_214_398, "bca610b8308e8d99f32e6fe4197e7ec01679264efed0cac9140fe9c29f1fbf7d"),
    "en.g2p": (6_691_149, "091347d375e494b5542202201a24a0f724738a3b18c38d56a87022970c70aa9c"),
    "kokoro-quant-convinteger.onnx": (92_361_271, "6e742170d309016e5891a994e1ce1559c702a2ccd0075e67ef7157974f6406cb"),
    "kokoro-rten.onnx": (124_604_222, "239d9f4df112a375bea52146570b97eb5c5af727c007761ee121ed123fd1ab29"),
    "g2p2_group_germanic-0.2.2-py3-none-any.whl": (20_585_622, "2b90141f4c5b3c9f9d1d9848eeb6ed2d784290bb311c569b64660b26f2b44219"),
}
URLS = {
    "kokoro-quant-convinteger.onnx": "https://github.com/taylorchu/kokoro-onnx/releases/download/v0.2.0/kokoro-quant-convinteger.onnx",
    "voices-v1.0.bin": "https://github.com/thewh1teagle/kokoro-onnx/releases/download/model-files-v1.1/voices-v1.0.bin",
    "g2p2_group_germanic-0.2.2-py3-none-any.whl": "https://files.pythonhosted.org/packages/aa/aa/2922b0dcb846d80ec1a873fcfad17859250c9b0a3ac4840d91153c298230/g2p2_group_germanic-0.2.2-py3-none-any.whl",
}
RUNTIME = ("kokoro.kkaot", "voices-v1.0.bin", "en.g2p")


def verify(path: Path, name: str | None = None) -> None:
    size, expected = PINS[name or path.name]
    with path.open("rb") as source:
        actual = hashlib.file_digest(source, "sha256").hexdigest()
    if path.stat().st_size != size or actual != expected:
        raise ValueError(f"pin mismatch: {path} ({path.stat().st_size} bytes, SHA-256 {actual})")


def download(path: Path) -> None:
    if path.exists():
        verify(path)
        return
    size, _ = PINS[path.name]
    url = URLS[path.name]
    temporary: Path | None = None
    try:
        with tempfile.NamedTemporaryFile(dir=path.parent, delete=False) as out:
            temporary = Path(out.name)
            with urllib.request.urlopen(url, timeout=60) as response:
                if not response.url.startswith("https://"):
                    raise ValueError("download redirected away from HTTPS")
                received = 0
                while chunk := response.read(1024 * 1024):
                    received += len(chunk)
                    if received > size:
                        raise ValueError(f"download exceeds pinned size: {url}")
                    out.write(chunk)
        verify(temporary, path.name)
        temporary.replace(path)
        print(f"downloaded and verified {path.name}", flush=True)
    finally:
        if temporary is not None:
            temporary.unlink(missing_ok=True)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--model-dir", type=Path, default=APP / "models/kokoro")
    parser.add_argument("--verify", action="store_true", help="verify runtime assets without downloads or compilation")
    args = parser.parse_args()
    root = args.model_dir.resolve()
    if args.verify:
        for name in RUNTIME:
            verify(root / name)
            print(f"verified {name}: {PINS[name][0]} bytes")
        return 0

    # Dependency failures happen before downloading any source model.
    import numpy
    import onnx

    source = root / "source"
    source.mkdir(parents=True, exist_ok=True)
    download(root / "voices-v1.0.bin")
    g2p = root / "en.g2p"
    if not g2p.exists():
        wheel = source / "g2p2_group_germanic-0.2.2-py3-none-any.whl"
        download(wheel)
        with zipfile.ZipFile(wheel) as archive:
            model = lzma.decompress(archive.read("g2p2_group_germanic/en.g2p.xz"))
            temporary = source / "en.g2p.tmp"
            temporary.write_bytes(model)
            verify(temporary, "en.g2p")
            temporary.replace(g2p)
            (source / "G2P2-GERMANIC-METADATA.txt").write_bytes(
                archive.read("g2p2_group_germanic-0.2.2.dist-info/METADATA")
            )
    verify(g2p)

    graph = source / "kokoro-rten.onnx"
    if not graph.exists():
        original = source / "kokoro-quant-convinteger.onnx"
        download(original)
        subprocess.run([sys.executable, str(TOOLS / "prepare_kokoro_rten.py"), str(original), str(graph)], check=True)
    verify(graph)
    program = root / "kokoro.kkaot"
    if not program.exists():
        subprocess.run([
            sys.executable, str(TOOLS / "compile_kokoro_aot.py"), "compile",
            str(graph), str(program), "--voices", str(root / "voices-v1.0.bin"),
            "--report", str(source / "kokoro_aot_analysis.json"), "--force",
        ], check=True)
    for name in RUNTIME:
        verify(root / name)
        print(f"verified {name}: {PINS[name][0]} bytes")
    (source / "preparation.json").write_text(json.dumps({
        "python": sys.version,
        "onnx": onnx.__version__,
        "numpy": numpy.__version__,
        "pins": {name: {"bytes": size, "sha256": digest} for name, (size, digest) in PINS.items()},
        "downloads": URLS,
    }, indent=2) + "\n")
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (OSError, ValueError, subprocess.CalledProcessError) as error:
        print(f"prepare_bundle: {error}", file=sys.stderr)
        raise SystemExit(1) from error
