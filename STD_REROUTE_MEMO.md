# TRUEOS Std Reroutes

Pattern: keep normal Rust `std` spellings in app/vendor code, but reroute their TRUEOS implementation downward into our kernel/runtime ABI.

Time entry: `std::time::SystemTime::now()` on `target_os = "trueos"` should call the TRUEOS time ABI, which calls Chronos for wall/unix time; `Instant::now()` should call the monotonic Chronos path.

Kernel/no_std code keeps using `crate::time` / `crate::chronos` directly; std-capable code may use `std::time` once the forked Rust std port is wired to TRUEOS.

This is the preferred migration style: move platform complexity down once, instead of patching every crate that imports `std::time::{SystemTime, UNIX_EPOCH}`.
