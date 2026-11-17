#!/usr/bin/env python3
"""Prepare the embedded voice and tokenizer, with optional Ubuntu model weights."""
import argparse
import hashlib
import json
from pathlib import Path
import urllib.request

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("--with-model", action="store_true", help="also prepare the Ubuntu checkpoint")
args = parser.parse_args()

root = Path(__file__).resolve().parents[1]
sources = json.loads((root / "asset-sources.json").read_text())
(root / "models").mkdir(exist_ok=True)
for name, source in sources["assets"].items():
    if name == "model.safetensors" and not args.with_model:
        continue
    path = root / "models" / name
    if not path.exists():
        pending = path.with_suffix(path.suffix + ".partial")
        urllib.request.urlretrieve(source["url"], pending)
        pending.replace(path)
    assert path.stat().st_size == source["bytes"], f"{name}: wrong size"
    with path.open("rb") as stream:
        assert hashlib.file_digest(stream, "sha256").hexdigest() == source["sha256"], f"{name}: wrong hash"
    print(f"verified {name}: {source['bytes']} bytes")
