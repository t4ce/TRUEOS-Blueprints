# BIOS Blueprint drop-in

Source snapshot: `c0f04cf0518ea92e3770fbad69bcce00da91989d`

Copy these paths into the root of `TRUEOS-Blueprints`:

- `apps/bios/`
- `crates/trueos-v/src/vbios.rs`
- optionally `tools/check-bios-blueprint-boundary.py`

Then apply `integration.patch` from the repository root. It only:

1. adds `apps/bios` to the workspace members;
2. exports `pub mod vbios;` from `crates/trueos-v/src/lib.rs`.
3. registers `"bios"` in `apps.json` so it can be packed from the app catalog.
4. changes `apps/bios/server.rs` bind address to `0.0.0.0` (all interfaces).

Validation:

```sh
node --check apps/bios/app.js
python3 tools/check-bios-blueprint-boundary.py
cargo +nightly-2026-07-10 check -p bios
```

Runtime URL: `http://0.0.0.0:1012/`

The server is GET-only and consumes the existing immutable `trueos_vlayer_bios_schema_snapshot_read` ABI.
