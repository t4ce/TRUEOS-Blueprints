#!/usr/bin/env python3
"""Exercise both native SDK copies against a recording, host-only session CABI."""

import json
from pathlib import Path
import subprocess
import tempfile


HERE = Path(__file__).resolve().parent
API = HERE.parents[1]
REPOSITORIES = API.parent.parent
SOURCES = [API / "src/command_session.rs",
           REPOSITORIES / "TRUEOS-Buildins/vendor/trueos/src/command_session.rs"]

with tempfile.TemporaryDirectory(prefix="trueos-command-session-wire-") as temporary:
    directory = Path(temporary)
    for index, source in enumerate(SOURCES):
        harness = (HERE / "harness.rs").read_text().replace(
            '"../../src/command_session.rs"', json.dumps(str(source)))
        path = directory / f"wire-{index}.rs"
        binary = directory / f"wire-{index}"
        path.write_text(harness)
        subprocess.run(["rustc", "+nightly-2026-07-10", "--edition=2024", "--test",
                        "--cfg", 'target_os="trueos"',
                        "-Aexplicit_builtin_cfgs_in_flags", "-Adead_code",
                        str(path), "-o", str(binary)], check=True)
        print(f"Native session SDK: {source}", flush=True)
        subprocess.run([str(binary), "--test-threads=1"], check=True)
