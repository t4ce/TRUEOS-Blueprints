# SQLite memory-image probe

Restored from `a1120e17:apps/rusqlite_probe`, using the original pinned
`t4ce/rusqlite` fork. The filesystem adapter drives the current async TRUEOSFS
API. The isolated test database is `/common/rusqlite-probe.db` in the Blueprint
filesystem scope.

The probe creates tables, enables foreign keys, upserts sample rows, checks a
join and row counts, serializes SQLite into TRUEOSFS, compares the saved bytes,
closes the connection, and deserializes the persisted image into a new connection
to verify the rows again. It does not test WAL, shared file-backed SQLx pools,
concurrent writers, or crash-safe whole-image publication.

Pack with `cargo bpp rusqlite_probe`. When other builds are clearing shared
artifacts, set `TRUEOS_BLUEPRINT_CARGO_CACHE_DIR` to an isolated cache directory.
The packer disables SQLite's optional POSIX sort-worker threads with
`SQLITE_MAX_WORKER_THREADS=0`, retaining SQLite mutex/thread safety.

On 2026-10-12 the restored Blueprint passed twice on the physical devrig with
SQLite 3.53.2. The first invocation created a 12,288-byte image; the second loaded
that image (`existed_before_open=true`, `loaded_bytes=12288`). Both reported:

```text
rusqlite-probe: PASS join=true persisted=true reopened=true
```

The downloaded TRUEOSFS image also passed `PRAGMA integrity_check` on the host
and returned `(1, 'blueprint-user', 1, 'en')` for the user/settings join.
The tested Blueprint SHA-256 was
`e53bf8bebe7d150400366ffab39b90ab07a58c0b60b856c1460fe9e106a62de8`.
It was published to the probe catalog and mirrored into the app catalog for the
currently installed App Store launcher (`§§rusqlite_probe`).
