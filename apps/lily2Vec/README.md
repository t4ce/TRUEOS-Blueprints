# lily2Vec — VNNI vibe classifier

Words in, one of ten expression tokens out. CPU AVX-VNNI performs the actual
classification scores. This version has no model download, Python, GPU, or
pretrained embedding dependency in its default build.

```sh
cd /home/t4ce/Repositories/TRUEOS-Blueprints/apps/lily2Vec
cargo build --release
./target/release/lily2vec
```

Type text, press Enter, get one token. Repeat; Ctrl-D exits. The interface also
accepts one-shot arguments or piped lines:

```sh
./target/release/lily2vec "i love you"
# affection
./target/release/lily2vec --scores "not angry, just explaining"
# neutral (backend, integer scores, timing go to stderr)
```

## The model we own

`vibes.json` is the entire model: ten labels, biases, and guided word/phrase
associations with signed integer weights. It is embedded in the binary.
For live adjustments without rebuilding:

```sh
./target/release/lily2vec --guide vibes.json --scores
```

Labels: `neutral`, `wave`, `happy`, `sad`, `affection`, `disapprove`, `agree`,
`confused`, `idea`, `angry`. Labels can be added up to 256; the same scorer
handles ten or a hundred. No training step is required to edit this guide.

1. Lowercase text, normalize punctuation, retain whole-word boundaries.
2. Mark guided word/phrase matches as unsigned byte features (0 or 1).
3. Multiply features by each label's signed byte weight row and accumulate
   with **AVX-VNNI VPDPBUSD**, then add its bias.
4. Return the highest-scoring label. First label wins ties; neutral has a bias
   of one, so unknown text selects neutral with this guide.

Phrase features provide explicit corrections: `not angry` subtracts anger
weight and adds neutral weight. This is a guided association model, not a
parser or pretrained general-language model. Its behavior comes entirely from
our editable guide. Repeated words do not multiply their influence.

Feature rows are padded to 32 bytes, matching one 256-bit VNNI dot-product
step. Each instruction accumulates 32 unsigned-byte × signed-byte products
into eight i32 lanes. We horizontally sum those lanes for the label score.
Negative guide weights are genuine inhibitory contributions, not a second
floating-point scoring path. The default matrix is only a few kilobytes.
The score chooses the output; VNNI is not a demonstration run alongside some
other classifier.

The eight raw instruction wrappers live in one canonical file:
`../../../TRUEOS/src/turbo/vnni.rs`. The kernel registers that module, and this
experiment imports the same source file through `#[path]`; it does not keep
a duplicate implementation. Lily's dot-product loop calls `dpbusd_256`.
All four instruction families are exposed at both 128 and 256 bits, with
wrapping and saturating variants. The wrapper is a reconstruction, not a
recovered historical file. Keep the TRUEOS and TRUEOS-Blueprints repository layout when
building this experiment.

Runtime detection requires AVX2 + AVX-VNNI and OS-enabled vector state. The
CLI errors if unavailable, rather than silently using another backend.
`score_reference` exists for correctness tests and benchmarks only. Inputs
are bounded to 16384 UTF-8 bytes; guides to 4096 features, with i8 weights and
i16 biases to keep i32 accumulation in range. Blank interactive lines are skipped.

## Evidence

```sh
cargo test
cargo clippy --all-targets -- -D warnings
./target/release/lily2vec --bench 1000000
objdump -d -C target/release/lily2vec | rg -B3 -A3 vpdpbusd
```

Verified on the development i9-13900K:

- VNNI execution matched integer reference scores, including negative weights,
  unsigned feature values through 255, and dimensions 32 through 4096.
- All eight shared wrappers passed hardware arithmetic checks, including
  overflowing accumulators and full-range byte/word operands.
- Twelve guided phrase/label cases passed; CLI line input was checked separately.
- Disassembly identifies VEX `vpdpbusd` in `lily2vec::dot_vnni`, using YMM registers.
- Default release executable was 782 KiB; the editable JSON guide 12151 bytes.

One measured release run, 1,000,000 iterations, seven rotating queries:

| Work | Time per selection |
| --- | ---: |
| Reference integer scoring + bias + winner | 215.4 ns |
| VNNI integer scoring + bias + winner | 29.0 ns |
| Text features + VNNI scoring + winner | 1406.8 ns |

Both scoring paths produced checksum 24999983. Scoring includes output-vector
allocation. Whole-selector timing excludes terminal IO. The scoring speedup
was about 7.4× in this run; this is not a claim of 7.4× whole-selector speedup.
The reference is ordinary optimized Rust, not deliberately disabled SIMD.

Observed outputs:

| Input | Token |
| --- | --- |
| hi | wave |
| i dont feel well | sad |
| i love you | affection |
| lets go and have a nice talk because you cute | happy |
| cute | affection |
| sweet | happy |
| NO | disapprove |
| yes that will do | agree |
| not angry, just explaining | neutral |
| what if we tried it | idea |
| i have no clue | confused |
| i am furious | angry |

The host CLI remains available. Native Codex response presentation is described
below; it uses this same classifier. The separate emomodel Blueprint installs
Potion assets and is not required by this integer classifier.

## Previous Potion experiment

The embedding implementation remains available as a comparison, behind an
optional feature. Its guide is `emotions.json`; see `POTION.md` for the previous
experiment's notes (use the binary name below instead of its old command).

```sh
cargo run --release --features potion --bin lily2vec-potion -- "i love you"
```

The default classifier does not load its weights or include its dependencies.


## Live Codex response presentation

TRUEOS Codex feeds live assistant-message text to `response::ResponsePresenter`.
The immutable VNNI model is initialized once per process and stays warm. No
model download, subprocess, SQLite, or AI tool call participates.

Snapshots contain recent contiguous verbatim response text, including Markdown.
User input, reasoning, tool output, and replayed history do not feed the sampler.
It aims for 384 Unicode characters with natural boundaries and retains at most
768. Large deltas replace older pending text rather than fabricating adjacency.
It samples at most once every 2.5 seconds; identical labels are suppressed for
five seconds. Short replies can be sampled on completion; authoritative item
completion can supply the final response tail. Cancellation, turn start, and
new assistant items discard pending text. Snapshots are coalesced rather than
queued. A failed native admission is not remembered as a successful cue.

The Blueprint API `trueos::spirit::play_expression` validates the ten labels and
sends a reserved `vibe:` word through the existing emotion ABI (under 16 bytes).
The matching kernel maps it to an existing resident clip:

| Label | Existing clip |
| --- | --- |
| neutral | idle.crossed.neutral_breath |
| wave | wave.neutral |
| happy | happy.cheer |
| sad | cry.wipe |
| affection | flirt.heart_hands |
| disapprove | disapprove.head_shake |
| agree | agree.firm_nod |
| confused | confused.question |
| idea | idea.finger_up |
| angry | angry.fists_clenched |

Requests enter the original package queue and clip-boundary sequencer.
Playback priorities, frames, pose transitions, and idle behavior are unchanged.
Unprefixed emotion requests retain their original randomized variants.

Both the updated kernel and Codex Blueprint are required. An older kernel can
reject these reserved presentation words. If VNNI is unavailable, Codex logs
once and disables expression cues; terminal output continues without an
alternate arithmetic backend. The `emomodel` asset installer is not required.

Checks cover the sampler/VNNI classifier, Blueprint API, and all ten mappings
through the real resident catalog/sequencer harness. The kernel check uses the
normal hardware features without embedding app payloads. Build native Codex
with `cargo bp codex` from the Blueprint repository.


Native build verification: `cargo bp codex` completed and published `codex.bp`
with SHA256 `2dcb52a39bc90216e372d633606e95e69d80737c7e186ed89f544e003cd7b28e`.
Loader and CABI compatibility checks passed. The matching kernel needs its
normal build/boot cycle. No machine was rebooted; live sprite playback from
this new Codex path has not been verified on hardware.


## Standalone Blueprint build

From `TRUEOS-Blueprints`, run `cargo bp lily2Vec` (lookup is case-insensitive).
The registered package is `lily2vec`, at `apps/lily2Vec`; the output is
`dist/lily2vec.bp`. The native CLI retains the same text-in, vibe-out interface.
The build script locates the canonical kernel VNNI helper and emits a module
reference that also works inside SDK staging. It does not copy the helper.
For a different checkout layout, set `LILY2VEC_VNNI_SOURCE` to the authored
`TRUEOS/src/turbo/vnni.rs` file before building.


## Codex AppDB bundle

`appbundle.json` now declares `lily2vec.bp` as a requirement of `codex.bp`,
alongside SQLite and xerj-service. The normal Apps download/install flow resolves
all members, fetches and verifies them, and imports the complete bundle in one
AppDB transaction before launching Codex. Missing or invalid Lily prevents a
partial Codex installation. Installing Lily by itself does not request Codex.

Bundle membership installs the standalone CLI; it does not launch it as a
service. Codex's automatic response presentation continues to use the in-process
warm classifier. The bundle metadata is embedded in the AppDB seed and takes
effect after the normal kernel/builtins preparation and boot/update cycle.
