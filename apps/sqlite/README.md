# SQLite Blueprint service

`sqlite` is a separate, instantiable Blueprint bound exclusively to IPv4
loopback. It owns SQLite connections and database images; callers send SQL over
HTTP rather than opening the database themselves. There is one existing Tokio
runtime per service instance. CPU work runs on Tokio's existing blocking
carriers, and each database has a Tokio mutex spanning execution and persistence.

The original pinned `t4ce/rusqlite` engine is reused. Each database is an
in-memory SQLite database, deserialized on first use and serialized into TRUEOSFS
before an HTTP transaction is acknowledged. Failed SQL batches roll back. A
failed image write also restores the service's previous in-memory image. An
admitted transaction continues if its HTTP client disconnects. There is no
client-side WAL or filesystem maintenance lock.

This service is not yet connected to Codex's SQLx client. Its HTTP batch API is
the tested transport boundary for that integration.

## Instances and advertisement

Pack with `cargo bp sqlite`; `replicatable = true` permits separate Blueprint
instances. Default instance `default` advertises `http://localhost:40894`.
The service also advertises the equivalent `127.0.0.1` endpoint, protocol
`trueos-sqlite/v1`, and SQLite version through startup output, `GET /healthz`, and
`/common/sqlite-service/INSTANCE/endpoint.json` in its filesystem scope.
Use `/healthz` to check liveness; the persisted announcement may outlive a server.

Instances accept `--instance NAME` or a Blueprint start script `instance NAME`.
For an App Store launch script:

```text
launch --headless sqlite -- instance codex
```

Names contain 1–64 ASCII letters, digits, hyphens or underscores. Each name
deterministically selects a loopback port in 20000–59999 and its own data
directory. The listener binds before storage is opened. A duplicate instance,
or a port collision between two names, fails admission rather than sharing data.
Host builds accept `--data-dir ROOT`; the instance name is always appended.

## HTTP API

`POST /v1/batch` executes one atomic batch against a named database:

```json
{
  "database": "state",
  "statements": [
    {"sql": "CREATE TABLE IF NOT EXISTS messages(id INTEGER PRIMARY KEY, body TEXT)"},
    {"sql": "INSERT INTO messages(body) VALUES(?)", "params": [{"type":"text", "value":"hello"}]},
    {"sql": "SELECT id, body FROM messages ORDER BY id"}
  ]
}
```

Each entry contains one SQL statement. Values are tagged `null`, `integer`,
`real`, `text`, or `blob` (byte array). Results contain column names, typed rows,
affected-row counts, and the last inserted row ID. HTTP 400 rejects SQL/parameter
errors and rolls back the batch; HTTP 500 reports storage failures. SQL cannot
attach files outside the service. Batches are limited to 128 statements, 10,000
result rows, an 8 MiB request/result payload, and five seconds of SQL work.
Images are limited to 128 MiB. Network retry after a lost response requires the
caller to use idempotent SQL; this protocol does not deduplicate requests.

TRUEOSFS persistence uses the same completed whole-file write as the restored
probe. Host persistence uses sync plus rename. Power-loss durability of the
TRUEOSFS image publication is not established by these tests.

## Verification

The isolated `host/Cargo.toml` builds the exact service source without the
workspace's platform-only dependency patches. Use the repository's pinned Rust
toolchain to build it, then run:

```sh
python3 apps/sqlite/tests/test_service.py --binary apps/sqlite/host/target/debug/sqlite
```

Host tests cover typed parameters, 32 concurrent writes, rollback, failed
publication, separate instances, duplicate-owner rejection, restart, and image
integrity. `cargo bpp sqlite_client` packs the separate Blueprint client probe.

On 2026-10-12 the physical devrig passed eight concurrent requests, rollback, and
two isolated named instances. `default` advertised port 40894; the second
instance's endpoint was separately announced. Downloaded images passed host
`PRAGMA integrity_check` and contained counters 8 and 999 respectively. Evidence
is recorded in `verification.json`.
