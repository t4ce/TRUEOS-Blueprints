#!/usr/bin/env python3
"""Run the real SQLx adapter, migrations, restart, and session publication rollback."""
import argparse
import json
import os
from pathlib import Path
import socket
import sqlite3
import struct
import subprocess
import tempfile
import time
import urllib.request


def connect():
    sock = socket.create_connection(("127.0.0.1", 40894), timeout=10)
    sock.sendall(b"GET /v1/connect HTTP/1.1\r\nHost: localhost\r\nConnection: Upgrade\r\nUpgrade: trueos-sqlite-v1\r\n\r\n")
    header = b""
    while not header.endswith(b"\r\n\r\n"):
        header += sock.recv(1)
    assert header.startswith(b"HTTP/1.1 101"), header
    return sock


def request(sock, **command):
    data = json.dumps(command).encode()
    sock.sendall(struct.pack("!I", len(data)) + data)
    def read(size):
        data = b""
        while len(data) < size:
            part = sock.recv(size - len(data))
            assert part, "unexpected session EOF"
            data += part
        return data
    return json.loads(read(struct.unpack("!I", read(4))[0]))


def run(service_binary, client_binary):
    with tempfile.TemporaryDirectory(prefix="sqlx-blueprint-") as root:
        root = Path(root)
        env = os.environ | {"TRUEOS_SQLITE_ENDPOINT": "http://localhost:40894"}
        for attempt in range(2):
            with (root / "server.log").open("a") as log:
                service = subprocess.Popen([service_binary, "--data-dir", str(root)], stdout=log, stderr=log)
                try:
                    for _ in range(100):
                        assert service.poll() is None, (root / "server.log").read_text()
                        try:
                            urllib.request.urlopen("http://127.0.0.1:40894/healthz", timeout=1)
                            break
                        except OSError:
                            time.sleep(.05)
                    completed = subprocess.run([client_binary], env=env, capture_output=True, text=True, timeout=60)
                    assert completed.returncode == 0, completed.stdout + completed.stderr
                    assert "sqlx-client: PASS" in completed.stdout
                    if attempt == 0:
                        with connect() as sock:
                            assert request(sock, op="open", database="publication", create=True, read_only=False)["error"] is None
                            assert request(sock, op="execute", sql="CREATE TABLE n(value INTEGER); INSERT INTO n VALUES(1)", params=[], timeout_ms=5000)["error"] is None
                            assert request(sock, op="begin", sql=None)["depth"] == 1
                            assert request(sock, op="execute", sql="UPDATE n SET value=2", params=[], timeout_ms=5000)["error"] is None
                            pending = root / "default/publication.pending"
                            pending.mkdir()
                            reply = request(sock, op="commit")
                            assert reply["error"] and reply["depth"] == 0, reply
                            reply = request(sock, op="execute", sql="SELECT value FROM n", params=[], timeout_ms=5000)
                            assert reply["results"][0]["rows"][0][0]["value"] == 1, reply
                            pending.rmdir()
                            assert request(sock, op="close")["error"] is None
                finally:
                    service.terminate()
                    service.wait(timeout=10)
            for image in (root / "default").glob("*.sqlite"):
                with sqlite3.connect(f"file:{image}?mode=ro", uri=True) as conn:
                    assert conn.execute("PRAGMA integrity_check").fetchone() == ("ok",), image
            with sqlite3.connect(root / "default/publication.sqlite") as conn:
                assert conn.execute("SELECT value FROM n").fetchone() == (1,)
        print("PASS SQLx typed_values nested_transactions drop_rollback disconnect_rollback interruption concurrent_updates six_Codex_migration_sets publication_rollback restart integrity")


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--service", required=True)
    parser.add_argument("--client", required=True)
    args = parser.parse_args()
    run(str(Path(args.service).resolve()), str(Path(args.client).resolve()))
