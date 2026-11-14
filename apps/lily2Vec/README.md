# lily2Vec — VNNI vibe classifier

Words in, one of ten expression tokens out. CPU AVX-VNNI performs the actual
classification scores. This version has no model download, Python, GPU, or
pretrained embedding dependency in its default build.

```sh
cd /home/t4ce/Repositories/lily2Vec
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
`../TRUEOS/src/turbo/vnni.rs`. The kernel registers that module, and this
experiment imports the same source file through `#[path]`; it does not keep
a duplicate implementation. Lily's dot-product loop calls `dpbusd_256`.
All four instruction families are exposed at both 128 and 256 bits, with
wrapping and saturating variants. The wrapper is a reconstruction, not a
recovered historical file. Keep the TRUEOS and lily2Vec sibling layout when
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

This is the host experiment. The VNNI classifier has not been deployed to
TRUEOS by this change; the separate emomodel Blueprint still installs Potion
assets and is not required by this classifier.

## Previous Potion experiment

The embedding implementation remains available as a comparison, behind an
optional feature. Its guide is `emotions.json`; see `POTION.md` for the previous
experiment's notes (use the binary name below instead of its old command).

```sh
cargo run --release --features potion --bin lily2vec-potion -- "i love you"
```

The default classifier does not load its weights or include its dependencies.
