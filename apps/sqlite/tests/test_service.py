#!/usr/bin/env python3
"""End-to-end tests against the actual standalone localhost service binary."""
import argparse
import concurrent.futures
import json
import os
from pathlib import Path
import select
import sqlite3
import subprocess
import tempfile
import urllib.error
import urllib.request


def start(binary, name, directory):
    process = subprocess.Popen(
        [str(binary), "--instance", name, "--data-dir", str(directory)],
        stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True,
    )
    if not select.select([process.stdout], [], [], 15)[0]:
        process.kill()
        raise AssertionError("service did not announce readiness")
    line = process.stdout.readline().strip()
    assert line.startswith("sqlite-service: READY "), line
    advertisement = json.loads(line.removeprefix("sqlite-service: READY "))
    assert advertisement["instance"] == name
    assert advertisement["endpoint"].startswith("http://localhost:")
    assert advertisement["ipv4_endpoint"].startswith("http://127.0.0.1:")
    return process, advertisement["ipv4_endpoint"]


def request(endpoint, statements, database="state", expected=200):
    body = json.dumps({"database": database, "statements": statements}).encode()
    req = urllib.request.Request(endpoint + "/v1/batch", body, {"Content-Type": "application/json"})
    try:
        with urllib.request.urlopen(req, timeout=15) as response:
            assert response.status == expected
            return json.load(response)
    except urllib.error.HTTPError as error:
        assert error.code == expected, error.read().decode()
        return json.load(error)


def sql(statement, params=None):
    return {"sql": statement, "params": params or []}


def integer(value):
    return {"type": "integer", "value": value}


def scalar(endpoint, statement):
    return request(endpoint, [sql(statement)])["results"][0]["rows"][0][0]["value"]


def stop(process):
    process.terminate()
    process.wait(timeout=10)


def run(binary):
    processes = []
    with tempfile.TemporaryDirectory(prefix="sqlite-service-") as root:
        root = Path(root)
        first_name = f"test-{os.getpid()}-a"
        second_name = f"test-{os.getpid()}-b"
        try:
            first, endpoint = start(binary, first_name, root)
            processes.append(first)
            request(endpoint, [sql("CREATE TABLE counter(id INTEGER PRIMARY KEY,value INTEGER NOT NULL)"),
                               sql("INSERT INTO counter VALUES (?,?)", [integer(1), integer(0)])])
            assert scalar(endpoint, "SELECT value FROM counter WHERE id=1") == 0
            typed = [integer(-9223372036854775807), {"type":"real","value":1.25},
                     {"type":"text","value":"SQLite ä"}, {"type":"blob","value":[0,255,7]}, {"type":"null"}]
            result = request(endpoint, [sql("SELECT ?,?,?,?,?", typed)])
            assert result["results"][0]["rows"][0] == typed
            with concurrent.futures.ThreadPoolExecutor(max_workers=8) as pool:
                list(pool.map(lambda _: request(endpoint, [sql("UPDATE counter SET value=value+1 WHERE id=1")]), range(32)))
            assert scalar(endpoint, "SELECT value FROM counter WHERE id=1") == 32

            image = root / first_name / "state.sqlite"
            before = image.read_bytes()
            request(endpoint, [sql("INSERT INTO counter VALUES(2,99)"), sql("INSERT INTO nonexistent VALUES(1)")], expected=400)
            assert scalar(endpoint, "SELECT count(*) FROM counter") == 1
            assert image.read_bytes() == before
            pending = image.with_suffix(".pending")
            pending.mkdir()
            request(endpoint, [sql("UPDATE counter SET value=100 WHERE id=1")], expected=500)
            pending.rmdir()
            assert scalar(endpoint, "SELECT value FROM counter WHERE id=1") == 32
            assert image.read_bytes() == before
            request(endpoint, [sql("ATTACH DATABASE '/tmp/not-service-owned.sqlite' AS other")], expected=400)
            request(endpoint, [sql("SELECT 1; SELECT 2")], expected=400)

            second, other = start(binary, second_name, root)
            processes.append(second)
            assert other != endpoint
            request(other, [sql("CREATE TABLE counter(value INTEGER)"), sql("INSERT INTO counter VALUES(999)")])
            assert scalar(other, "SELECT value FROM counter") == 999
            assert scalar(endpoint, "SELECT value FROM counter") == 32

            # A second owner of the same instance must fail before touching its image.
            duplicate = subprocess.run([str(binary), "--instance", first_name, "--data-dir", str(root)],
                                       capture_output=True, text=True, timeout=10)
            assert "sqlite-service: FAIL" in duplicate.stdout
            assert scalar(endpoint, "SELECT value FROM counter") == 32

            stop(first)
            processes.remove(first)
            reopened, new_endpoint = start(binary, first_name, root)
            processes.append(reopened)
            assert new_endpoint == endpoint
            assert scalar(endpoint, "SELECT value FROM counter") == 32
            db = sqlite3.connect(f"file:{image}?mode=ro", uri=True)
            assert db.execute("PRAGMA integrity_check").fetchone() == ("ok",)
            db.close()
            print("PASS localhost typed_parameters concurrent_writers rollback instances duplicate_owner restart integrity")
        finally:
            for process in processes:
                if process.poll() is None:
                    stop(process)


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--binary", required=True, type=Path)
    run(parser.parse_args().binary.resolve())
