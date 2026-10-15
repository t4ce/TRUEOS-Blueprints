`file_replace_contract` exercises `tempfile` 3.27 `NamedTempFile::persist`
through the workspace rustix overlay. It replaces existing files while source
and destination descriptors remain open, checks detached destination writes,
checks failed replacement preserves both objects, and saves a second generation.
All files live under a unique probe-owned temporary directory; no real config or
authentication paths are used. Each case logs BEGIN/PASS/FAIL with monotonic
elapsed milliseconds. A watchdog exits with status 124 after 120 seconds.
Successful saves rename dirty temporary files before any fsync, matching Codex's
atomic-write order. Later writes through retained descriptors still call fsync.
The watchdog receives a stop signal and joins before the final suite result, so
a completed probe leaves no sleeping watchdog job.

Host baseline:

```sh
timeout 150s cargo run -p file_replace_contract --target x86_64-unknown-linux-gnu
```

Build without publication:

```sh
TRUEOS_BLUEPRINT_SKIP_APPS_PUBLISH=1 cargo bpp file_replace_contract
```

Native validation requires the resulting Blueprint to run on the updated kernel.
A successful host baseline or Blueprint build does not establish native success.
