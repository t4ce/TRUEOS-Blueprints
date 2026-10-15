#!/usr/bin/env python3
"""Run the real tempfile host baseline with an independent process deadline."""
from pathlib import Path
import re
import subprocess

ROOT = Path(__file__).resolve().parents[2]


def main():
    subprocess.run(
        ["cargo", "build", "-p", "file_replace_contract", "--target", "x86_64-unknown-linux-gnu"],
        cwd=ROOT, check=True, timeout=150,
    )
    result = subprocess.run(
        [str(ROOT / "target/x86_64-unknown-linux-gnu/debug/file_replace_contract")],
        cwd=ROOT, capture_output=True, text=True, timeout=130,
    )
    print(result.stdout, end="")
    if result.stderr:
        print(result.stderr, end="")
    if result.returncode or not re.search(
        r"file_replace_contract: PASS case=suite elapsed_ms=\d+ passed=5 failed=0", result.stdout,
    ):
        raise RuntimeError(f"probe failed or incomplete: exit={result.returncode}")


if __name__ == "__main__":
    main()
