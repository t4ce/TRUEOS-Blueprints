# Portable host check, 2026-10-14

Compiler: `rustc +1.95.0 --edition=2024 --test src/main.rs -C debuginfo=2 -C force-frame-pointers=yes`, with the current probe source and read-only cached Codex dependency artifacts. This is source-level host validation, not a freshly rebuilt Cargo dependency graph or TRUEOS runtime check.

The executable `/tmp/depthchecker_factchecker-host-tests --nocapture` exited 0:

```text
running 1 test
depthchecker_factchecker: BEGIN id=1 scenario=text_stream
depthchecker_factchecker: PASS id=1 scenario=text_stream checksum=23
depthchecker_factchecker: BEGIN id=2 scenario=path_resolution
depthchecker_factchecker: PASS id=2 scenario=path_resolution checksum=56
depthchecker_factchecker: BEGIN id=3 scenario=protocol_permissions
depthchecker_factchecker: PASS id=3 scenario=protocol_permissions checksum=68
depthchecker_factchecker: BEGIN id=4 scenario=transport_zstd
depthchecker_factchecker: PASS id=4 scenario=transport_zstd checksum=84
depthchecker_factchecker: SKIP id=5 scenario=cabi_clock reason=host_has_no_TRUEOS_CABI
depthchecker_factchecker: BEGIN id=6 scenario=native_thread
depthchecker_factchecker: PASS id=6 scenario=native_thread checksum=736
depthchecker_factchecker: BEGIN id=7 scenario=filesystem
depthchecker_factchecker: PASS id=7 scenario=filesystem checksum=35
depthchecker_factchecker: COMPLETE pass_mask=0x6f skip_mask=0x10
test tests::all_portable_entry_paths_complete_with_assertions ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
```

Explicit dependency artifacts under `/home/t4ce/Repos/codex/codex-rs/target/debug/deps`:

```text
libcodex_utils_text-816f6749d3d398ad.rlib
libcodex_utils_paths-c228802035d7bf5a.rlib
libcodex_protocol-9609b447aa464c6e.rlib
libcodex_transport-c9e3fb9b9b047f42.rlib
libhttp-bd32a997f828e330.rlib
libserde_json-4363f95da25bbc2a.rlib
libzstd-f94617985ed1a205.rlib
```

The compiler also searched that directory for transitive dependency artifacts. The standalone manifest separately resolved successfully with `cargo +1.95.0 metadata --offline --format-version 1`, using its own lockfile. The resolved metadata included 642 packages across platforms/build roles; that is not an executed runtime call count.

Host `nm -C` verified emitted candidate symbols including `<core::str::pattern::StrSearcher as core::str::pattern::Searcher>::next_match`, `url::parser::Parser::parse_url`, `FileSystemPermissions::as_legacy_permissions`, `ZSTD_compressStream2` and `ZSTD_decompressStream`. This symbol presence is not a trace proving every candidate was called, and host addresses are not reusable as Blueprint addresses.
