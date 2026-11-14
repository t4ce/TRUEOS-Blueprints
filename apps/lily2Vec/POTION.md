# lily2Vec

Rust-only experiment: words in, one Lily expression token out. No Python,
GPU, chat model, tool calls, or Spirit integration. Uses Model2Vec Potion 8M
and nearest-example cosine similarity. This is a selectable-label baseline,
not a trained emotion classifier.

## Run

```bash
cd /home/t4ce/Repositories/lily2Vec
cargo build --release
./target/release/lily2vec "hello, waving at you!"
# wave
```

For continuous use, run without text, type a line, press Enter, and receive a
token. Repeat; Ctrl-D exits. The model stays loaded between lines.

```bash
./target/release/lily2vec
```

Pipes work too: one nonempty input line produces one output token.

```bash
printf '%s\n' 'we finally fixed it, awesome!' 'I have no clue what you mean' |
  ./target/release/lily2vec
# happy
# confused
```

Use `--scores` to print the five closest labels and warm selection time to
stderr. Stdout still contains only the chosen token per input.

```bash
./target/release/lily2vec --scores "sending you a big hug"
```

First use downloads the model into the Hugging Face cache; later runs reuse
the files. A local directory containing `tokenizer.json`, `config.json`, and
`model.safetensors` can be supplied with `--model /path/to/model`.
The default model is English, with roughly 30 MB of weights; total process
memory is higher. This host program is not a verified TRUEOS/no_std port.

## Labels and tuning

`emotions.json` contains the editable labels and example phrases. Built-in
labels are embedded at build time; rebuild after editing, or load the file
directly for tuning without rebuilding:

```bash
./target/release/lily2vec --catalog emotions.json --scores "your words here"
```

Current tokens: neutral, wave, happy, sad, angry, confused, idea, agree,
disapprove, shy, affection, playful, scared, tired, facepalm. They are inspired
by the character preview's available performances, not a complete animation
mapping. No assets or existing character files were changed.

Examples are embedded once at startup. Each incoming text is embedded once,
then compared with all examples. Each label's score is its best matching
example; the highest score wins deterministically. Scores are similarities,
not confidence probabilities. Blank input lines are skipped; a whitespace-only
one-shot input is an error. Text with a zero embedding falls back to neutral.
The encoder uses its default 512-token input limit.

## Initial checks

Release build and `cargo clippy -- -D warnings` passed. A 12-input CLI smoke
check confirmed one valid label per line. Some observed selections:

| Input | Output |
| --- | --- |
| hey there! | wave |
| we finally fixed it, awesome! | happy |
| I have no clue what you mean | confused |
| yes, exactly right | agree |
| I am furious about this | angry |
| time to sleep, good night | tired |
| here is the requested file | neutral |
| sending you a big hug | affection |
| oh no, I forgot the obvious thing again | facepalm |
| what if we use a smaller model? | idea |
| could you wave back at me? | wave |

Known failure: `not angry, just explaining the issue` selected `angry`.
Static embeddings can miss negation and conversational nuance. These examples
are observations, not an accuracy benchmark.

On this development machine, one cached load plus catalog encoding took
38.4 ms; the short inputs then took about 0.005–0.008 ms each for embedding
and ranking, excluding output. The first run including downloads took about
1.9 seconds. These are single-run observations, not performance guarantees.

Foundation: https://github.com/MinishLab/model2vec-rs

Model: https://huggingface.co/minishlab/potion-base-8M
