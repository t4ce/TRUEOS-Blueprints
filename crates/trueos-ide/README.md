# TRUEOS IDE platform reference

The IDE owns the compact, versioned TRUEOS API reference. Edit embeds it with
this crate's `api-reference` feature and includes the overview, API ID, source
fingerprint and topic catalog in the workspace `context` response. Codex attaches
that summary alongside the active file and open tabs. The reference is available
for Odin and Rust projects and does not enter compiler arguments or source
snapshots.

Retrieve a full topic through the existing `trueos_ide` tool:

```json
{"action":"api_reference","topic":"terminal"}
```

The request also takes the tool's normal workspace endpoint. Topic names come
from the context catalog. Replies contain the API ID, topic, content SHA-256 and
reference text. The operation reads embedded documentation and changes no files,
project revision, build or runtime capability. Full topics are fetched on demand
to keep automatic context small; each reply fits the broker's 64 KiB limit.

## Generating the snapshot

From the sibling TRUEOS checkout:

```bash
python3 tools/build_api_prm.py
python3 tools/build_api_prm.py --check
```

The generator exports the public Blueprint C ABI from Cargo/rustdoc JSON,
including signatures, C layouts and constants, and combines it with reviewed
usage notes from `tools/api-prm/usage.json`. It excludes retired imports. It is a
platform ABI reference, not the complete Rust SDK or Odin core library.

`api-prm/` is the checked-in compact snapshot. Kernel source and raw rustdoc JSON
are build inputs only. When built with `api-reference`, this crate's build script
refreshes the snapshot if the sibling TRUEOS generator is available and source
fingerprints changed. `TRUEOS_API_SOURCE_ROOT` selects another kernel checkout.
A standalone checkout uses the bundled snapshot; all builds with this feature
verify its hashes using Python 3. The manifest records the generation inputs.
Kernel and other protocol-only clients can leave the feature disabled.

A reference fingerprint identifies its build inputs. It does not attest that the
running kernel exposes every documented symbol; runtime imports and capabilities
remain governed by the deployed kernel.
