#!/usr/bin/env python3
"""Run production adapter unit tests outside the parent Blueprint workspace."""
from pathlib import Path
import shutil
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[3]


def main():
    vendor = ROOT / "vendor/sqlx-sqlite-0.9.0"
    with tempfile.TemporaryDirectory(prefix="sqlx-remote-setup-") as folder:
        package = Path(folder) / "sqlx-sqlite"
        shutil.copytree(vendor, package)
        manifest = (package / "Cargo.toml").read_text()
        manifest = manifest.replace('path = "../../../TRUEOS-Buildins/vendor/reqwest-0.13.3"', f'path = "{ROOT / "../TRUEOS-Buildins/vendor/reqwest-0.13.3"}"')
        manifest = manifest.replace('path = "../../api"', f'path = "{ROOT / "api"}"')
        (package / "Cargo.toml").write_text(manifest + "\n[workspace]\n")
        subprocess.run([
            "cargo", "test", "--offline", "--manifest-path", str(package / "Cargo.toml"),
            "--target", "x86_64-unknown-linux-gnu", "--no-default-features", "--features", "trueos-remote",
            "--lib", "--target-dir", str(ROOT / "probes/sqlx_client/host/target"),
            "remote_setup_tests", "--", "--test-threads=1",
        ], cwd=ROOT, check=True, timeout=180)


if __name__ == "__main__":
    main()
