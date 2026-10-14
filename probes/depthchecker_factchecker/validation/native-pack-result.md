# Native diagnostic repack with worker filesystem work

The cached diagnostic pack completed with exit status 0 on 2026-10-14. Publication was disabled. The native worker source now runs 32 parser iterations, checks parser checksum 736, then runs the 35-byte filesystem roundtrip before returning that parser checksum. File synchronization assertions are retained in both the worker and Hull lanes.

Command, from the TRUEOS-Blueprints root:

```sh
TRUEOS_BLUEPRINT_SKIP_APPS_PUBLISH=1 CARGO_BUILD_JOBS=2 \
  target/x86_64-unknown-linux-gnu/debug/trueos-blueprint \
  --probes depthchecker_factchecker \
  > probes/depthchecker_factchecker/target/depthchecker-pack-native-fs.log 2>&1
```

The shell environment had no `cargo bp` alias, so this uses the compiled Blueprint packer directly. The packer confirmed diagnostic flags, combined nine root codegen objects, verified the CABI pack guard and printed `skipping probes publish`.

| Artifact | Bytes | SHA-256 |
| --- | ---: | --- |
| `dist/depthchecker_factchecker.bp` | 3,733,757 | `0e7e445be67c7338e92f5e315a651fd873ee01b6a7276e0f9425db7693a4c7ce` |
| `dist/depthchecker_factchecker.debug.o` | 939,277,016 | `1c303f4ebfeb708c7e88aed7bcc5e0a147546bd893a5f54bfb82baa11444593a` |
| `dist/depthchecker_factchecker.packed.o` | 15,131,632 | `abb639b29ececf5bddcbcf23771bd2e6ec013112492686850d1ca7856372a16f` |

[`native-elf-symbols.json`](native-elf-symbols.json) binds these hashes and the current probe/staged-source hashes. [`refresh-native-inventory.py`](refresh-native-inventory.py) independently checked artifact sizes/hashes, every curated emitted symbol and the exact packer entry augmentation of staged `main.rs`.

This verifies native compilation and packing. Runtime completion, filesystem service behavior and physical frame capture require the matching rebuilt kernel and guest execution; they are not established by this result.
