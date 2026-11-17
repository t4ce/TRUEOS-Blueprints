#!/usr/bin/env python3
"""Fetch only the pinned English model, tokenizer and Azelma build inputs."""
import hashlib
import json
from pathlib import Path
import urllib.request

root = Path(__file__).resolve().parents[1]
sources = json.loads((root / "asset-sources.json").read_text())
(root / "models").mkdir(exist_ok=True)
for name, source in sources["assets"].items():
    path = root / "models" / name
    if not path.exists():
        pending = path.with_suffix(path.suffix + ".partial")
        urllib.request.urlretrieve(source["url"], pending)
        pending.replace(path)
    assert path.stat().st_size == source["bytes"], f"{name}: wrong size"
    with path.open("rb") as stream:
        assert hashlib.file_digest(stream, "sha256").hexdigest() == source["sha256"], f"{name}: wrong hash"
    print(f"verified {name}: {source['bytes']} bytes")
