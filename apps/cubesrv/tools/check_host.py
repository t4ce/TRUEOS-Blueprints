#!/usr/bin/env python3
"""Check the actual CubeSrv sources/build script with upstream host dependencies.

The workspace patches target TRUEOS; several vendored dependencies do not build
on Linux. This temporary manifest checks the same server without those patches.
"""
from pathlib import Path
import subprocess
import tempfile
APP = Path(__file__).resolve().parents[1]
BLUEPRINTS = APP.parents[1]
with tempfile.TemporaryDirectory(prefix='cubesrv-host-') as directory:
    root = Path(directory)
    for name in ('slides','assets'): (root/name).symlink_to(APP/name, target_is_directory=True)
    (root/'Cargo.toml').write_text(f'''[package]
name = "cubesrv-host-check"
version = "0.0.0"
edition = "2024"
build = "{APP}/build.rs"
[[bin]]
name = "cubesrv"
path = "{APP}/server.rs"
[dependencies]
axum = {{ version = "=0.8.9", default-features = false, features = ["http1", "json", "tokio"] }}
cubes-protocol = {{ path = "{BLUEPRINTS}/crates/cubes-protocol" }}
serde = {{ version = "=1.0.228", features = ["derive"] }}
serde_json = "=1.0.150"
trueos = {{ path = "{BLUEPRINTS}/api", features = ["lifecycle-net"] }}
trueos-redb = {{ path = "{BLUEPRINTS}/crates/trueos-redb", features = ["std"] }}
[build-dependencies]
serde_json = "=1.0.150"
sha2 = "0.10"
''')
    subprocess.run(['cargo','check','--offline','--manifest-path',str(root/'Cargo.toml'),
                    '--target-dir',str(APP/'target/host-check')],cwd=root,check=True)
