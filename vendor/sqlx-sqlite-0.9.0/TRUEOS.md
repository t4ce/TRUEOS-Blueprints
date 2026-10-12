# TRUEOS SQLx adapter

Based on crates.io sqlx-sqlite 0.9.0 (MIT/Apache-2.0). Native modules are retained;
TRUEOS selects the `src/remote` modules instead. `trueos-remote` enables the same
transport on a host for integration tests. The native libsqlite3-sys dependency
is target-gated, so execution of the Codex Blueprint contains no SQLite C engine.

The adapter uses the instantiable localhost SQLite Blueprint and the caller's
Tokio runtime. See `../../apps/sqlite/README.md` for discovery, protocol,
transaction guarantees, persistence, limits, and end-to-end verification.
