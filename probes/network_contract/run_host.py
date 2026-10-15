#!/usr/bin/env python3
"""Build and run all cases with the real vendor stack and a managed loopback peer."""
import argparse
import json
from pathlib import Path
import time
import subprocess
import sys
import tempfile

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--port", type=int, default=32360)
    args = parser.parse_args()
    subprocess.run(["cargo", "build", "-p", "network_contract", "--target", "x86_64-unknown-linux-gnu"], cwd=ROOT, check=True)
    with tempfile.TemporaryFile(mode="w+") as peer_log:
        peer = subprocess.Popen([sys.executable, "-u", str(HERE / "fixture/server.py"), "--port", str(args.port), "--seconds", "180"], stdout=peer_log, stderr=peer_log, text=True)
        try:
            deadline = time.monotonic() + 5
            while True:
                peer_log.seek(0)
                ready = peer_log.readline()
                if ready and json.loads(ready).get("event") == "fixture_ready":
                    break
                if peer.poll() is not None or time.monotonic() >= deadline:
                    raise RuntimeError("fixture did not report readiness")
                time.sleep(0.01)
            result = subprocess.run([str(ROOT / "target/x86_64-unknown-linux-gnu/debug/network_contract"), "--host=127.0.0.1", f"--port={args.port}"], cwd=ROOT, text=True, capture_output=True, timeout=150)
            print(result.stdout, end="")
            print(result.stderr, end="", file=sys.stderr)
            if result.returncode or "network_contract: PASS case=suite passed=28 failed=0" not in result.stdout:
                raise RuntimeError(f"probe failed or incomplete: exit={result.returncode}")
        finally:
            peer.terminate()
            try:
                peer.wait(timeout=5)
            except subprocess.TimeoutExpired:
                peer.kill()
                peer.wait()
            peer_log.seek(0)
            events = [json.loads(line) for line in peer_log if line.startswith("{")]
            assert sum(event.get("event") == "fin_observed" and event.get("received_bytes") == 300*1024 for event in events) == 1, "missing independent FIN after complete 300 KiB payload"
            assert sum(event.get("event") == "pong_observed" for event in events) == 2, "missing independent Pong witnesses"
            assert sum(event.get("event") == "ws_frame" and event.get("opcode") == 2 and event.get("payload_bytes") == 300*1024 for event in events) == 4, "missing independent300KiB frame witnesses"
            print("network_contract fixture: FIN=1 Pong=2 binary_300k=4 verified")


if __name__ == "__main__":
    main()
