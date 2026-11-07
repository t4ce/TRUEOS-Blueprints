#!/usr/bin/env python3
"""End-to-end tests against the actual standalone localhost service binary."""
import argparse
import concurrent.futures
import json
import os
from pathlib import Path
import select
import socket
import sqlite3
import struct
import subprocess
import tempfile
import threading
import time
import urllib.error
import urllib.parse
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


def scalar(endpoint, statement, database="state"):
    return request(endpoint, [sql(statement)], database=database)["results"][0]["rows"][0][0]["value"]


def session(endpoint, database):
    address = urllib.parse.urlsplit(endpoint)
    sock = socket.create_connection((address.hostname, address.port), timeout=15)
    try:
        sock.sendall(b"GET /v1/connect HTTP/1.1\r\nHost: localhost\r\nConnection: Upgrade\r\nUpgrade: trueos-sqlite-v1\r\n\r\n")
        header = b""
        while not header.endswith(b"\r\n\r\n"):
            part = sock.recv(1)
            assert part and len(header) < 8192, "incomplete session upgrade"
            header += part
        assert header.startswith(b"HTTP/1.1 101"), header
        command(sock, op="open", database=database, create=True, read_only=False)
        return sock
    except BaseException:
        sock.close()
        raise


def command(sock, expected_error=False, **operation):
    data = json.dumps(operation).encode()
    sock.sendall(struct.pack("!I", len(data)) + data)

    def read(size):
        data = b""
        while len(data) < size:
            part = sock.recv(size - len(data))
            assert part, "unexpected session EOF"
            data += part
        return data

    size = struct.unpack("!I", read(4))[0]
    assert size <= 8 * 1024 * 1024, size
    reply = json.loads(read(size))
    assert (reply["error"] is not None) == expected_error, reply
    return reply


def execute(sock, statement):
    return command(sock, op="execute", sql=statement, params=[], timeout_ms=5000)


def temporary_storage(endpoint):
    # Request FILE explicitly: the native memory-only engine must still execute
    # its temp B-tree and sorter without trying to open an OS temporary file.
    # Four MiB of keys exceed SQLite's usual sorter/PMA threshold. Returning
    # every ordered key through group_concat prevents a top-N shortcut.
    result = request(endpoint, [
        sql("PRAGMA temp_store=FILE"),
        sql("CREATE TEMP TABLE work(k INTEGER,payload TEXT)"),
        sql("WITH RECURSIVE n(x) AS (VALUES(1) UNION ALL SELECT x+1 FROM n WHERE x<4096) "
            "INSERT INTO work SELECT (x*37)%4096, printf('%01024d',x) FROM n"),
        sql("SELECT count(*),sum(length(payload)) FROM work"),
        sql("EXPLAIN QUERY PLAN SELECT k FROM work ORDER BY payload DESC"),
        sql("SELECT group_concat(k,',') FROM (SELECT k FROM work ORDER BY payload DESC)"),
        sql("DROP TABLE work"),
    ], database="temporary")
    assert result["results"][3]["rows"] == [[integer(4096), integer(4096 * 1024)]]
    plan = result["results"][4]["rows"]
    assert any("TEMP B-TREE FOR ORDER BY" in row[-1]["value"] for row in plan), plan
    ordered = result["results"][5]["rows"][0][0]["value"]
    assert list(map(int, ordered.split(","))) == [(n * 37) % 4096 for n in range(4096, 0, -1)]


def persistent_sessions(endpoint, directory):
    with session(endpoint, "sessions") as sock:
        execute(sock, "CREATE TABLE counter(value INTEGER); INSERT INTO counter VALUES(0)")
        assert command(sock, op="begin", sql=None)["depth"] == 1
        execute(sock, "UPDATE counter SET value=1")
        assert command(sock, op="begin", sql=None)["depth"] == 2
        execute(sock, "UPDATE counter SET value=999")
        assert command(sock, op="rollback")["depth"] == 1
        assert execute(sock, "SELECT value FROM counter")["results"][0]["rows"] == [[integer(1)]]
        assert command(sock, op="commit")["depth"] == 0
        interrupted = command(sock, expected_error=True, op="execute",
                              sql="WITH RECURSIVE n(x) AS (VALUES(0) UNION ALL SELECT x+1 FROM n WHERE x<1000000000) SELECT sum(x) FROM n",
                              params=[], timeout_ms=1)
        assert interrupted["error"]["code"] & 255 == 9, interrupted
        assert execute(sock, "SELECT value FROM counter")["results"][0]["rows"] == [[integer(1)]]
        assert command(sock, op="begin", sql=None)["depth"] == 1
        execute(sock, "UPDATE counter SET value=999")
        # Dropping the transport, without a Close/Rollback RPC, owns rollback.
    assert scalar(endpoint, "SELECT value FROM counter", database="sessions") == 1

    with session(endpoint, "sessions") as sock:
        before = (directory / "sessions.sqlite").read_bytes()
        command(sock, op="begin", sql=None)
        execute(sock, "UPDATE counter SET value=999")
        pending = directory / "sessions.pending"
        pending.mkdir()
        try:
            assert command(sock, expected_error=True, op="commit")["depth"] == 0
        finally:
            pending.rmdir()
        assert execute(sock, "SELECT value FROM counter")["results"][0]["rows"] == [[integer(1)]]
        assert (directory / "sessions.sqlite").read_bytes() == before
        command(sock, op="close")

    with session(endpoint, "sessions") as sock:
        command(sock, op="begin", sql=None)
        execute(sock, "UPDATE counter SET value=999")
        started = threading.Event()

        def waiting_writer():
            started.set()
            return request(endpoint, [sql("UPDATE counter SET value=value+1")], database="sessions")

        with concurrent.futures.ThreadPoolExecutor(max_workers=1) as pool:
            pending = pool.submit(waiting_writer)
            assert started.wait(5)
            # An unrelated database progresses while this database is owned.
            assert scalar(endpoint, "SELECT value FROM counter WHERE id=1") == 32
            try:
                pending.result(timeout=.15)
            except concurrent.futures.TimeoutError:
                pass
            else:
                raise AssertionError("writer bypassed a live session transaction")
            command(sock, op="rollback")
            pending.result(timeout=15)
        command(sock, op="close")
    assert scalar(endpoint, "SELECT value FROM counter", database="sessions") == 2


def concurrent_databases(endpoint):
    # Same-database updates above are serialized by the service. Distinct
    # connections exercise SQLite's shared allocator and static mutexes on
    # several blocking carriers, rather than accidentally testing a no-op lock.
    def write(number):
        name = f"parallel-{number}"
        request(endpoint, [sql("CREATE TABLE n(value INTEGER)"),
                           sql("INSERT INTO n VALUES(?)", [integer(number)])], database=name)
        return scalar(endpoint, "SELECT value FROM n", database=name)

    with concurrent.futures.ThreadPoolExecutor(max_workers=8) as pool:
        assert list(pool.map(write, range(16))) == list(range(16))


def native_engine(endpoint):
    options = ("THREADSAFE=1", "TEMP_STORE=3", "OMIT_LOAD_EXTENSION", "OMIT_LOCALTIME")
    result = request(endpoint, [sql("SELECT " + ",".join(
        f"sqlite_compileoption_used('{option}')" for option in options))])
    assert result["results"][0]["rows"] == [[integer(1) for _ in options]], result
    before = int(time.time())
    result = request(endpoint, [sql(
        "SELECT CAST(strftime('%s','now') AS INTEGER),date('2000-02-29'),"
        "datetime(0,'unixepoch'),datetime('2000-01-01','localtime'),datetime('2000-01-01','utc'),"
        "hex(randomblob(64)),hex(randomblob(64))")])
    row = result["results"][0]["rows"][0]
    assert before - 1 <= row[0]["value"] <= int(time.time()) + 1, row
    assert row[1] == {"type": "text", "value": "2000-02-29"}, row
    assert row[2] == {"type": "text", "value": "1970-01-01 00:00:00"}, row
    # UTC timestamps remain available; the two locale-dependent conversions
    # explicitly yield NULL in the memory-only engine's no-timezone contract.
    assert row[3:5] == [{"type": "null"}, {"type": "null"}], row
    assert len(row[5]["value"]) == len(row[6]["value"]) == 128, row
    assert row[5] != row[6] and row[5]["value"] != "0" * 128, row


def stop(process):
    process.terminate()
    process.wait(timeout=10)


def run(binary, native=False):
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
            for target in ("/tmp/not-service-owned.sqlite", ":memory:", "file:outside?mode=memory&cache=shared"):
                # Denial must roll back earlier writes in the same batch too.
                request(endpoint, [sql("UPDATE counter SET value=999 WHERE id=1"),
                                   sql(f"ATTACH DATABASE '{target}' AS other")], expected=400)
                assert scalar(endpoint, "SELECT value FROM counter WHERE id=1") == 32
                assert image.read_bytes() == before
            request(endpoint, [sql("SELECT 1; SELECT 2")], expected=400)
            temporary_storage(endpoint)
            persistent_sessions(endpoint, root / first_name)
            concurrent_databases(endpoint)
            if native:
                native_engine(endpoint)

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
            assert scalar(endpoint, "SELECT value FROM counter", database="sessions") == 2
            assert scalar(endpoint, "SELECT count(*) FROM sqlite_temp_schema", database="temporary") == 0
            for persisted in (root / first_name).glob("*.sqlite"):
                with sqlite3.connect(f"file:{persisted}?mode=ro", uri=True) as db:
                    assert db.execute("PRAGMA integrity_check").fetchone() == ("ok",), persisted
            print("PASS localhost typed_parameters concurrent_writers rollback publication_rollback attach_isolation "
                  "temp_sort nested_transactions disconnect_rollback transaction_lock independent_databases "
                  "interruption session_publication_rollback instances duplicate_owner restart integrity" +
                  (" native_engine utc_clock entropy" if native else ""))
        finally:
            for process in processes:
                if process.poll() is None:
                    stop(process)


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--binary", required=True, type=Path)
    parser.add_argument("--native", action="store_true", help="also require the memory-only native SQLite engine")
    args = parser.parse_args()
    run(args.binary.resolve(), native=args.native)
