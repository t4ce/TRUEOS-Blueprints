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
admitted batch continues if its HTTP client disconnects. Persistent sessions
roll back an uncommitted transaction when their socket closes or is idle for
60 seconds. There is no
client-side WAL or filesystem maintenance lock.

Codex's SQLx SQLite driver connects to this service on TRUEOS. It retains the
existing SQLx query, row decoding, pool, and migration interfaces. The native
host driver still uses the original SQLx implementation.

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

## Codex / SQLx connections

The platform vendor `vendor/sqlx-sqlite-0.9.0` uses `GET /v1/connect` with an
HTTP/1.1 upgrade to `trueos-sqlite-v1`. Each connection then carries sequential
JSON frames with a four-byte big-endian length. Operations are `open`, `execute`,
`prepare`, `begin`, `commit`, `rollback`, `ping`, and `close`. Replies include
result sets, transaction depth, and SQLite extended error codes. Scripts use
SQLite's own statement iterator, including migration DDL and numbered parameters.
An explicit transaction retains the database guard across requests; nested
transactions use savepoints. Only the outer commit publishes a new image.
A failed publication restores the previous committed image before releasing the
guard. The service reclaims free pages in bounded batches with Codex's reserve
policy; no client file lock is needed.

Codex defaults to instance `codex` and automatically asks the installed App Store
to start `sqlite` when its loopback endpoint is unavailable. Set
`TRUEOS_SQLITE_INSTANCE` to choose another named instance, or
`TRUEOS_SQLITE_ENDPOINT=http://localhost:PORT` to connect to an already running
one. An explicitly configured endpoint is never launched automatically. Clients
use IPv4 loopback directly and bypass HTTP proxies. Each client database path
maps to `db-FNV64.sqlite` within the service instance's directory; database files
are owned by the server rather than created in Codex's filesystem scope.

The adapter queues commands on the caller's existing Tokio runtime, including
rollback when a SQLx transaction is dropped. It creates no runtime or SQLite
worker thread. `libsqlite3-sys` is absent from its TRUEOS dependencies. Client C
handles, collations using Rust closures, database deletion, and filesystem-based
corruption recovery are unavailable through this service. A lost commit response
can have an unknown outcome; the adapter does not retry SQL automatically.
Result limits remain 10,000 rows / 8 MiB, and transaction lock waits and SQL work
are bounded at five seconds. Query budgets are enforced inside the SQLite server.

Connection setup has one 30-second deadline covering service-launch lock wait,
service readiness, HTTP upgrade headers, the upgrade itself, and the initial
Open reply. Upgrade header attempts use at most five seconds; health probes use
at most 750 ms and require a 2xx status. Readiness retries every 300 ms within
the remaining deadline, rather than counting probes as elapsed time. This is a
setup budget, not a global reqwest timeout on the persistent upgraded stream.
As with Tokio timers generally, an executor blocked in synchronous code must
resume polling before it can observe a deadline; late completions are rejected.

The `sqlx_sqlite::remote_timing` tracing target records fixed setup phase names,
numeric connection/request IDs and elapsed milliseconds. Initial Open and slow
RPC exchanges (wire or actor-queue time at least 100 ms) emit one summary
separating queue, serialization, write, reply header, body and parse time.
Unfinished stage durations are absent; `completed` describes transport phase
completion, not SQLite command success. No SQL, database name, endpoint,
parameters, response content or error messages are included. Run the real
TCP/HTTP deadline regressions with:

```sh
python3 apps/sqlite/tests/test_remote_setup.py
```

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

The SQLx integration probe embeds all six actual Codex migration sets with SQLx's
migration resolver. Its isolated workspace keeps the host SQLx C dependency
separate from the service's pinned rusqlite engine during Cargo resolution.
Build `probes/sqlx_client/host/Cargo.toml`, then run:

```sh
python3 apps/sqlite/tests/test_sqlx.py --service apps/sqlite/host/target/debug/sqlite --client probes/sqlx_client/host/target/debug/sqlx_client
```

This covers typed values, affected rows with RETURNING, nested savepoints,
dropped transactions, socket-disconnect rollback, concurrent pooled updates,
SQLite error codes and interruption, all migrations twice, failed commit image
publication, server restart, and integrity of the persisted images. Pack the
same client with `cargo bpp sqlx_client` for devrig verification.

On 2026-10-13 the SQLx client Blueprint automatically launched the `codex` SQLite
instance at `http://localhost:42116` and passed all six Codex migration sets,
typed values, 16 concurrent updates, nested/dropped transactions, disconnect
rollback and query interruption. The downloaded database passed integrity_check
and contained the expected integer, Unicode text, blob and NULL values.
`sqlx-verification.json` records the loaded Blueprint hashes and persisted image.
Codex packing then compiled `codex-state` and reached four unrelated Tokio
filesystem adapter errors in `codex-rollout`.
