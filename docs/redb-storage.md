# redb storage

The Blueprint database paths use redb 4.2.0 with default features disabled and
`experimental-api-5` enabled, matching the kernel and `tredb`. The resolved
workspace dependency graph contains no `rusqlite` or `libsqlite3-sys` package.
PrismQ stores circuits and revisions in redb tables. `redb_probe` and
`redb_multirt` replace the retired SQLite probe selectors.

`crates/trueos-redb` supplies a shared RAM backend wrapper. It closes redb before
returning the image, refuses to publish an image while a write transaction still
retains the backend, and rejects corrupt nonempty images. Callers write completed
images through async filesystem I/O to a staging file and then rename it. A redb
commit in RAM is not a promise of disk durability before that filesystem step.
The multi-runtime probe owns one database per lane; it does not claim shared-file
writer concurrency. It verifies 512 rows across two lanes, distinct stable WLS
slots, reopened images, and every value in the persisted summary.

## Existing PrismQ circuits

PrismQ now uses `prismq.redb`. An existing `prismq.sqlite3` is preserved. If that
file exists and the redb file does not, PrismQ requires the conversion export
instead of silently starting an empty circuit library.

On the development host, run the one-time converter against a copied database:

```sh
python3 tools/export_prismq_circuits.py /path/to/prismq.sqlite3 /path/to/prismq-circuits-v1.json
```

Place `prismq-circuits-v1.json` in PrismQ's application filesystem root. Its next
database open imports all current circuits and numbered revisions in one redb
transaction, then persists `prismq.redb`. Later opens use that redb file. Keep the
old database and export until circuit counts and revision loads have been checked.
The older `circuits/index.json` layout remains importable when no previous
database or conversion export exists. The converter alone uses the host Python
SQLite module; no SQLite engine, binding, build flags or database fallback ships
in the Blueprint paths. Upstream C compiler fixtures in `badc` are unrelated to
application database storage.

Old `rusqlite_probe.bp` and `rusqlite_multirt.bp` files in dist, published catalogs,
or an installed app database do not become redb packages automatically. Build and
publish the current probe catalog and select `redb_probe` / `redb_multirt` for new
runs. No existing rig data is removed by this source migration.

Rust coverage includes image close/reopen, aborted transactions, corrupt images,
and PrismQ revision/delete behavior. Compilation and runtime execution of that
coverage remain required; syntax and metadata checks are not runtime acceptance.
