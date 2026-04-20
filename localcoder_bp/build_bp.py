#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import struct
import subprocess
import sys
from pathlib import Path


def find_trueos_repo(start: Path) -> Path:
    for base in [start, *start.parents]:
        candidate = base / "TRUEOS"
        if (candidate / "86_64.json").is_file() and (candidate / "Cargo.toml").is_file():
            return candidate
        if (base / "86_64.json").is_file() and (base / "Cargo.toml").is_file() and (
            base / "crates" / "trueos-v" / "Cargo.toml"
        ).is_file():
            return base
    raise SystemExit("failed to locate TRUEOS repo root")


def capture_artifact(cmd: list[str], cwd: Path, profile: str) -> Path:
    proc = subprocess.run(
        cmd,
        cwd=cwd,
        text=True,
        capture_output=True,
        check=False,
    )
    sys.stdout.write(proc.stdout)
    sys.stderr.write(proc.stderr)
    if proc.returncode != 0:
        raise SystemExit(proc.returncode)

    artifact_path: Path | None = None
    for line in proc.stdout.splitlines():
        line = line.strip()
        if not line or not line.startswith("{"):
            continue
        try:
            message = json.loads(line)
        except json.JSONDecodeError:
            continue
        if message.get("reason") != "compiler-artifact":
            continue
        target = message.get("target") or {}
        if target.get("name") != "localcoder_bp":
            continue
        for filename in message.get("filenames", []):
            if filename.endswith(".o"):
                artifact_path = Path(filename)

    if artifact_path is None:
        cargo_profile = "debug" if profile == "dev" else profile
        deps_dir = cwd / "tgt" / "86_64" / cargo_profile / "deps"
        if deps_dir.is_dir():
            matches = sorted(
                (
                    path
                    for pattern in ("localcoder_bp-*.o", "liblocalcoder_bp-*.o")
                    for path in deps_dir.glob(pattern)
                ),
                key=lambda path: path.stat().st_mtime,
            )
            if matches:
                artifact_path = matches[-1]

    if artifact_path is None:
        raise SystemExit("failed to locate emitted .o artifact")
    return artifact_path


def write_blueprint(obj_path: Path, out_path: Path) -> None:
    payload = obj_path.read_bytes()
    header = b"TRBP" + struct.pack("<HHQII", 1, 1, 0, len(payload), len(payload))
    out_path.parent.mkdir(parents=True, exist_ok=True)
    out_path.write_bytes(header + payload)


def main() -> None:
    parser = argparse.ArgumentParser(description="Build localcoder_bp and wrap it as a TRUEOS .bp module")
    parser.add_argument(
        "--profile",
        choices=("dev", "release"),
        default="release",
        help="Cargo profile to build",
    )
    parser.add_argument(
        "--out",
        default="dist/localcoder_bp.bp",
        help="Output .bp path relative to localcoder_bp/",
    )
    args = parser.parse_args()

    here = Path(__file__).resolve().parent
    repo_root = find_trueos_repo(here.parent)

    cmd = [
        "cargo",
        "+nightly",
        "rustc",
        "--manifest-path",
        str(here / "Cargo.toml"),
        "--lib",
        "--message-format=json-render-diagnostics",
    ]
    if args.profile == "release":
        cmd.append("--release")
    cmd.extend(["--", "--emit=obj"])

    artifact = capture_artifact(cmd, repo_root, args.profile)
    out_path = (here / args.out).resolve()
    write_blueprint(artifact, out_path)
    print(f"wrote {out_path}")


if __name__ == "__main__":
    main()
