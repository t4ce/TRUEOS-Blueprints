# TRUEOS Std Reroutes

Pattern: keep normal Rust `std` spellings in app/vendor code, but reroute their TRUEOS implementation downward into our kernel/runtime ABI.

Time entry: `std::time::SystemTime::now()` on `target_os = "trueos"` should call the TRUEOS time ABI, which calls Chronos for wall/unix time; `Instant::now()` should call the monotonic Chronos path.

Kernel/no_std code keeps using `crate::time` / `crate::chronos` directly; std-capable code may use `std::time` once the forked Rust std port is wired to TRUEOS.

This is the preferred migration style: move platform complexity down once, instead of patching every crate that imports `std::time::{SystemTime, UNIX_EPOCH}`.

Filesystem/path entry: app and vendor code should keep normal Rust spellings such as `use std::fs;` and `use std::path::{Path, PathBuf};`.

`Path`/`PathBuf` stay the generic std lexical path types. TRUEOS behavior belongs below them: `std::env::current_dir()` should resolve to the blueprint app root, and `std::fs` operations should route through the TRUEOS FS/CABI backend for that app root. In other words, we do not replace `Path`; we patch or validate the std platform backend behind `env` and `fs`, the same way time is routed behind `std::time`.

For Tokio-capable apps, prefer `tokio::fs` at async call sites. Our vendored Tokio has a TRUEOS FS shim that already lowers common async file operations into the CABI path; std FS remains the compatibility surface for sync helpers and crates that cannot naturally move into async code.

Metadata note: `std::fs::Metadata` is an opaque std platform type, so Tokio cannot honestly fabricate it from a small CABI result. Until the TRUEOS std backend exposes real platform metadata, Tokio `metadata` should fail clearly instead of pretending.

Blueprint proof surface: `trueos_blueprint::fs::stat(path)` is the intentional metadata-lite CABI today. It returns only `FsStat { kind, len }`, enough to prove file-vs-directory and byte length across VMX. `tokio_fs.bp` exercises this through `fs.stat.file` and `fs.stat.dir`.
