# emomodel

On-demand Blueprint that prepares Model2Vec data for Lily's expression selector.
It reports installed bundles and explicitly downloads one when asked. It exits
after setup; no resident service, inference runtime, SQLite dependency, or xerj
dependency is added. Registered in `apps.json`; this is not a buildin.

## TRUEOS

Build from the Blueprint repository:

```sh
cargo bp apps/emomodel
```

The existing SDK build command also publishes the resulting `dist/emomodel.bp`
to its configured app catalog. Install/launch using the normal Blueprint flow.
The Blueprint uses argv-v1 and TRUEOSFS scope. Commands:

```sh
emomodel
emomodel status
emomodel setup
emomodel setup 8m
emomodel setup 32m
emomodel setup 4m
```

On TRUEOS, bare launch (including `§§emomodel`) runs setup automatically because
Shell3 bare-name launches do not carry argv. On the host, bare launch defaults
to status. Explicit status performs no network requests and creates no files. Setup
defaults to `auto`: reuse a valid installed bundle, otherwise try 8m, 32m, 4m
in that order. Explicit IDs attempt only that choice. Every operation reports
which bundles are absent, incomplete/invalid, or ready, and marks the selected
bundle. `setup` downloads on demand, not during boot or Blueprint compilation.

| ID | Pinned model | Complete bundle bytes | Role |
| --- | --- | ---: | --- |
| 8m | minishlab/potion-base-8M | 30,920,628 | Original working default |
| 32m | minishlab/potion-base-32M | 130,703,808 | Larger alternative |
| 4m | minishlab/potion-base-4M | 15,802,292 | Smaller fallback |

These are compatible static embedding models; larger does not guarantee a
better Lily choice, and 4m is not claimed to have equal general accuracy.
All three returned the same expected five labels in the small host check below.

## Files and consumer handoff

Data lives under the app's own TRUEOSFS root:

```text
/apps/emomodel/
  active.json
  setup.lock
  8m/<pinned-revision>/
    config.json
    tokenizer.json
    model.safetensors
    ready
```

The other options have their own revision directories. `active.json` contains
the selected ID, repository, pinned revision, directory, and the three filenames.
Consumers read this manifest and load that directory, without fetching from
Hugging Face or relying on a host cache. Each model requires all three files;
weights alone are insufficient. The existing selector still separately needs
its expression examples (`lily2Vec/emotions.json`).

`models.json` pins upstream revisions, file sizes, and SHA256 digests. Native HTTPS uses the kernel bounded HTTP/TLS fetch service (90 seconds,
a body limit of at least 1 MiB for redirect responses, with exact artifact
size and SHA256 verification afterward), matching Apps. Host HTTPS uses reqwest/Hyper/rustls
with bounded redirects.
Each response is limited to its expected size, then checked before publication.
Verified files can be reused after interruption; the `ready` marker is written
last. Status rechecks the file hashes rather than accepting file existence.
An exclusive setup lock prevents concurrent installers. TRUEOSFS uses its
whole-file write path, without depending on replacement rename semantics.
The old active manifest is only updated after a complete bundle succeeds.

Downloads buffer one artifact at a time: allow memory for the largest weights
file (about 129 MB for 32m), plus network/runtime overhead. This installer is
short lived. It is not the resident inference footprint.

## Host verification

Build without publishing, and use a host directory instead of `/apps`:

```sh
cargo build --release --manifest-path apps/emomodel/Cargo.toml
target/x86_64-unknown-linux-gnu/release/emomodel --root /tmp/emomodel status
target/x86_64-unknown-linux-gnu/release/emomodel --root /tmp/emomodel setup 8m
cargo test --manifest-path apps/emomodel/Cargo.toml
cargo clippy --manifest-path apps/emomodel/Cargo.toml -- -D warnings
```

Use the printed directory with the original experiment:

```sh
/home/t4ce/Repositories/lily2Vec/target/release/lily2vec \
  --model /tmp/emomodel/8m/bf8b056651a2c21b8d2565580b8569da283cab23 \
  --scores "i love you"
```

Verified locally: all three bundles downloaded and passed integrity checks;
each loaded with `lily2Vec` and returned `wave`, `sad`, `affection`,
`disapprove`, `agree` for `hi`, `i dont feel well`, `i love you`, `NO`,
`yes that will do`. Repeated setup reused valid files without downloads.
A deliberately corrupted tokenizer was reported invalid and repaired by setup.
Unit tests and Clippy passed. The native Blueprint packaged successfully
(2,155,452 bytes) and passed SDK loader/CABI compatibility checks. Native execution was attempted over SSH on 192.168.178.94. The original
reqwest path stalled fetching weights; it was stopped and replaced by the
kernel fetch service. After switching native network requests to the kernel fetch service, using
`?download=true` for Hugging Face downloads, and publishing files with
`trueos::async_fs::write_file`, setup completed on that machine: 8m ready
in 7.806 seconds. Weights and tokenizer were downloaded and hash-verified;
the native completion marker and active manifest were published.

Single host runs, cached files, five short inputs (not a quality benchmark):

| Model | Model + example load | Warm embedding + selection per input |
| --- | ---: | ---: |
| 8m | 43.8 ms | 0.005–0.007 ms |
| 32m | 127.2 ms | 0.009–0.011 ms |
| 4m | 24.3 ms | 0.004–0.007 ms |

## Inference placement

The existing selector performs CPU vector lookup/pooling and a tiny similarity
scan. The SDK can package this setup utility in its own VMX/Hull; no persistent
VMX is needed for data preparation. Bringing inference to TRUEOS is a separate
integration: the host Model2Vec/tokenizer dependencies have not been ported by
this change.

Codex can eventually load the selected data in its own Hull, keeping one warm
model per process, or a separate on-demand inference Blueprint can share one
warm model among clients. SQLite remains a database service.

The current `probes/xerj_service` launches the engine with no Potion model or
emotion-selector configuration. XERJ supports lexical/neural embedding modes,
but we found no Model2Vec/Potion implementation in the engine crates searched.
No XERJ inference latency, memory sharing, or semantic benefit was measured
here, so this change does not move expression inference into xerj. A measured
comparison and a compatible embedding/selection interface would precede that
decision. No engine or SQLite code was changed.
