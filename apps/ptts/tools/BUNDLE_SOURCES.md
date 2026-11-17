# English Azelma bundle inputs

[asset-sources.json](../asset-sources.json) pins the exact files used by this
package. The official Pocket TTS source reference is
`d28c519caa36766ec917ec006902c20ff1a77e0b`; the inference dependency is the
`xn-ptts` fork at `7efa26973f81bba42cf2ace8da2c1eb9b605d54d`.

The Blueprint embeds Azelma's English voice state and the matching JSON
tokenizer. The English model weights live in a separate checkpoint file that
the service opens at startup. All three assets are pinned to the same model
configuration.

| Input | Bytes | SHA-256 | Runtime |
| --- | ---: | --- | --- |
| `model.safetensors` | 219,029,196 | `4c56cbd171c63e8a26ea61f5d48b8a0cd8f3c51f4652c0ab9a19bfcced8c931d` | Persisted checkpoint |
| `azelma.safetensors` | 7,178,040 | `cc09e106a1acf5fe0b4c5de61b2b7686080b7bab853f9c11bfef219801bf3126` | Embedded |
| `tokenizer.json` | 245,020 | `f498428e1eafee50492f7be13dc9bfafcfc12e508cd0eb1b01c92ecd5d8c6687` | Embedded |

The two embedded assets total **7,423,060 bytes** before executable code, model
configuration, fixed-sample token IDs, policy and package compression. The
checkpoint accounts for **219,029,196 bytes** in app storage.

## Pinned sources

The weights are the official `english_2026-10` model without voice-cloning
weights, from `kyutai/pocket-tts-without-voice-cloning` at
`a7056f505a4a8ca72d8b5fb1440071d6f1a35739`:

[English model checkpoint](https://huggingface.co/kyutai/pocket-tts-without-voice-cloning/resolve/a7056f505a4a8ca72d8b5fb1440071d6f1a35739/languages/english_2026-10/model.safetensors).

The matching Azelma state is the official English preset, from the same model
repository at `a3055dcb533cf2ec90458008a01e7cef550bae2a`:

[English Azelma voice state](https://huggingface.co/kyutai/pocket-tts-without-voice-cloning/resolve/a3055dcb533cf2ec90458008a01e7cef550bae2a/languages/english/embeddings/azelma.safetensors).

The official voice catalog maps Azelma to the enhanced VCTK reference recording
`kyutai/tts-voices/vctk/p303_023_enhanced.wav`. The Blueprint embeds the prepared
state above, not that recording. This identifies voice provenance; the preset
name is not an independently asserted demographic or emotion label.

The tokenizer comes from the official `english_2026-09` tokenizer path reused by
the `english_2026-10` configuration, at
`00eac05ed3d16bdc3f6b5d598874019c34a89214`:

[English tokenizer](https://huggingface.co/kyutai/pocket-tts-without-voice-cloning/resolve/00eac05ed3d16bdc3f6b5d598874019c34a89214/languages/english_2026-09/tokenizer.json).

Do not substitute the older Candle fork's `b6369a24` weights, tokenizer or voice
state. The current model and matching preset use a different prepared voice
state format.

## Reproduce preparation

Use the Python standard library on the developer host:

```sh
python3 /home/t4ce/Repositories/TRUEOS-Blueprints/apps/ptts/tools/prepare_bundle.py
```

The script prepares the Azelma voice state and tokenizer under ignored
`models/`. Add `--with-model` to prepare the checkpoint for Ubuntu synthesis
or to copy it to the seed directory. The script verifies byte counts and
SHA-256 for new downloads and existing files. A mismatched file stops
preparation with an error.

`build.rs` verifies the voice state and tokenizer and embeds them in aligned
static storage. It also prepares token IDs for the retained `--sample` proof
from [sample.txt](../sample.txt). Changing that demo text requires a rebuild.
The warm service independently tokenizes arbitrary runtime text using the
embedded tokenizer. Tokenizer parallelism is disabled to avoid a tokenizer
worker pool. FlowLM and Mimi generate audio on the target CPU.

On TRUEOS, `ptts` loads `userdata/models/english_2026-10/model.safetensors`
under its app root. If it is missing, the service fetches the checkpoint from
the configured seed URL, verifies its length and hash, then publishes it as a
new file. Later launches verify the saved file and use it locally. The
[seed server instructions](../README.md#seed-the-model-on-trueos) describe the
developer host setup.

## Notices

The upstream Pocket TTS software notice is copied verbatim to
[licenses/POCKET-TTS-MIT.txt](licenses/POCKET-TTS-MIT.txt). That notice covers its
source code; it does not establish licensing for the model, tokenizer, source
recording or prepared voice state. Their pinned download URLs and hashes above
record provenance without replacing the publishers' respective terms. Preserve
upstream model and voice attribution when distributing these assets.

The runtime also reuses only the shared English frontend crate's emoji-removal
helper. It does not embed Kokoro weights, its English G2P model or its voice
archive. Preserve that helper's
[Unicode data notice](../../ttstt/crates/trueos-kokoro/g2p/LICENSE-UNICODE).
