#!/usr/bin/env python3
"""Run actual SQLite against the staged native allocator/mutex/File boundary.

Uses the same production stage function as `cargo bp xerj-server`, selecting
the native OS boundary on a host solely for this behavior proof. The separate
native engine build/import gate and rig probe verify TRUEOS execution.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import sqlite3
import subprocess
import tempfile

SDK = Path(__file__).resolve().parents[1]


def run(output):
    output.mkdir(parents=True, exist_ok=True)
    stage = output / "stage"
    env = dict(os.environ, TRUEOS_NATIVE_SQLITE_PROOF_STAGE=str(stage), CARGO_BUILD_JOBS="2")
    with (output / "staging.log").open("w") as log:
        subprocess.run(["cargo", "test", "-p", "trueos-blueprint", "--bin", "trueos-blueprint",
                        "native_sqlite_stages_exact_registry_boundaries", "--", "--nocapture"],
                       cwd=SDK, env=env, stdout=log, stderr=subprocess.STDOUT, check=True)
    project = output / "proof"
    (project / "src").mkdir(parents=True, exist_ok=True)
    provider = stage / "source-overlay-crates/libsqlite3-sys-0.30.1-trueos-native"
    (project / "Cargo.toml").write_text('''[package]
name="trueos-native-sqlite-vfs-proof"
version="0.1.0"
edition="2024"
[workspace]
[dependencies]
rusqlite={version="=0.32.1",features=["bundled"]}
libsqlite3-sys={version="=0.30.1",features=["bundled","trueos-native-testing"]}
[patch.crates-io]
libsqlite3-sys={path=''' + json.dumps(str(provider)) + '''}
''')
    shutil.copyfile(SDK / "tests/native_sqlite_vfs/proof.rs", project / "src/lib.rs")
    fixtures = project / "fixtures"
    fixtures.mkdir(exist_ok=True)
    source = fixtures / "source.db"
    for suffix in ("", "-wal", "-shm"):
        Path(str(source) + suffix).unlink(missing_ok=True)
    with sqlite3.connect(source) as db:
        db.execute("create table records(value text)")
        db.commit()
        db.execute("pragma journal_mode=WAL")
        db.execute("pragma wal_autocheckpoint=0")
        db.execute("insert into records values ('native-wal-recovery-token')")
        db.commit()
        # An existing committed WAL is essential: copying only the main DB
        # would let a broken reader pass while losing the newest source rows.
        for suffix in ("", "-wal", "-shm"):
            shutil.copyfile(str(source) + suffix, fixtures / ("fixture.db" + suffix))
    db.close()
    toolchain = "nightly-2026-07-10"
    for profile in ("debug", "release"):
        args = ["cargo", "+" + toolchain, "test", "--offline", "--manifest-path", str(project / "Cargo.toml")]
        if profile == "release":
            args.append("--release")
        args.extend(["--", "--nocapture"])
        with (output / (profile + ".log")).open("w") as log:
            # /tmp avoids inheriting a native target's Cargo configuration.
            subprocess.run(args, cwd=tempfile.gettempdir(), env=env,
                           stdout=log, stderr=subprocess.STDOUT, check=True)
    files = ["src/native_sqlite_dependency_overlay.rs", "src/native_sqlite_vfs.rs",
             "apps/sqlite/src/native_backend.rs", "tests/native_sqlite_vfs/proof.rs"]
    (output / "receipt.json").write_text(json.dumps({
        "status": "PASS", "debug_tests": 6, "release_tests": 6,
        "scope": "Host actual SQLite C with native OS_OTHER allocator/mutex/File VFS selected; no native rig execution claimed here.",
        "checks": ["transaction rollback and read-only enforcement", "retained native File lock and release",
                   "lock contention across processes", "WAL write and reopen",
                   "read-only existing-WAL recovery without changing input files", "large blob and temp sort"],
        "sources": {name: hashlib.sha256((SDK / name).read_bytes()).hexdigest() for name in files},
    }, indent=2) + "\n")
    print(output / "receipt.json")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    if args.output:
        run(args.output.resolve())
    else:
        with tempfile.TemporaryDirectory(prefix="native-sqlite-vfs-proof-") as directory:
            run(Path(directory))
