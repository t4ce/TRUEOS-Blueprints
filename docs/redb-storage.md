# redb storage

PrismQ stores circuits and revision history exclusively in `prismq.redb`. A
missing database starts a fresh library. There is no SQLite dependency,
converter, legacy JSON import, or fallback storage path. Existing empty or
corrupt redb files return an error rather than resetting the library.

The TRUEOS Blueprint database paths use redb 4.2.0 with default features disabled
and `experimental-api-5` enabled, matching the kernel and `tredb`. PrismQ host
builds enable redb's `std` feature; TRUEOS builds keep the no-std backend.
`crates/trueos-redb` supplies a shared RAM backend wrapper. It closes redb before
returning the image and refuses to publish an image while a transaction still
retains the backend. Callers write the completed image through async filesystem
I/O. RAM transactions do not imply atomic or crash-safe whole-file persistence.

`redb_probe` and `redb_multirt` replace the retired database probe selectors.
Both read back and reopen the persisted file. The multi-runtime probe owns one
database per lane, verifies 512 rows across two lanes, checks distinct stable WLS
slots and reopened images, and verifies every value in the persisted summary.
It does not test concurrent writers to the same file.

Rebuild and publish the current probe catalog before running the new selectors;
previously packed or installed Blueprints retain their previous implementation.
Upstream C compiler fixtures in `badc` are unrelated to application storage.

Rust regression cases cover image close/reopen, aborted transactions, corrupt
images, and PrismQ revision/delete behavior. Compilation and runtime execution
remain required; syntax and dependency checks do not establish runtime acceptance.

For host execution of the image backend regression cases, enable the wrapper's
`std` feature (`cargo test --manifest-path crates/trueos-redb/Cargo.toml --features std`).
