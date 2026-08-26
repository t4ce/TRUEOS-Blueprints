# Test report

## JavaScript and frontend

```text
node --check js/00_fallback_core.js
node --check js/10_trueos_adapter.js
node --check js/20_demo_pattern.js
node --check web/app.js
node --check tests/live-eval-smoke.mjs
node tests/live-eval-smoke.mjs
```

Result:

```text
strudel_core live expression smoke passed
```

The smoke covers canonical `sequence("a", ["b", "c"])`, a successful live
commit, rejection of a non-Pattern value, and rejection of a nested commit.
Both failure cases preserve the active event rows and revision.

## Reference PCM

The new expression-style demo was submitted through the same adapter entrypoint
as the HTTP UI, then rendered for 96,000 frames.

```text
sample rate: 48000
channels: 2
frames: 96000
seconds: 2
peak: 20246
WAV SHA-256: 96df96c82189dcb419869daa9a9f249d4a00373014c4a2923874b2cba0b51dd6
```

The result is byte-for-byte identical to the existing reference WAV.

## Uploaded target capture

```text
file SHA-256: e7d9c7be59a49b1a6f978296681fe0b1cfa11d430ef6960998c0af7311e46e7f
format: PCM s16le, stereo, 48000 Hz
frames: 720000
seconds: 15
minimum sample: -20246
maximum sample: 20215
peak: -4.182214 dBFS
longest all-zero stereo run: 1 frame
```

Decoded PCM SHA-256 for every complete region 0-2, 2-4, ..., 12-14 s:

```text
90826a4e4e64924daf4f7d80230e035f36cc1134d4f9ef46cd0eb28e177dd62b
```

## Static checks

- Cargo.toml parsed with Python `tomllib`
- every JavaScript `getElementById` target exists exactly once in the HTML
- CSS braces balanced
- no CRLF or trailing whitespace in proposed text files
- `git diff --check` passed
- combined root/app patch applied cleanly to the exact current app baseline

## Not run

The execution container does not contain rustc, Cargo, the TRUEOS target
compiler, or a complete repository checkout. Rust target compilation is left to
GitHub/repository analysis.
