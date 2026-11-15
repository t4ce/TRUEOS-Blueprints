# English Heart bundle inputs

These are host build inputs under the ignored `models/kokoro/` directory.
The Blueprint embeds the native Kokoro program, the `af_heart` style array,
and English G2P. It does not download or open model files at runtime.
The full voice archive below is required only to authenticate the input and
extract that one voice at build time. Misaki KLEX is not part of this bundle.

| Build input | Bytes | SHA-256 |
| --- | ---: | --- |
| `kokoro.kkaot` | 124,081,360 | `b7d4b9c62f3df01f71fc2585acd60190d1809dae9a6d916fe39c86d7dd4e3217` |
| `voices-v1.0.bin` | 28,214,398 | `bca610b8308e8d99f32e6fe4197e7ec01679264efed0cac9140fe9c29f1fbf7d` |
| `en.g2p` | 6,691,149 | `091347d375e494b5542202201a24a0f724738a3b18c38d56a87022970c70aa9c` |

The extracted `af_heart.npy` array is 522,368 bytes and hashes to
`0212418aafafb1e9878f3300787937aa401ac937cff8c0310ffa32963d96c77b`.

## Reproduce

Use a host Python environment with `onnx` and `numpy`. The verified preparation
used ONNX 1.23.2, NumPy 2.5.4, and Python 3.14. Every intermediate and final
artifact has a byte count and SHA-256 pin; different output is rejected.

```sh
python3 -m venv /tmp/ttstt-assets-venv
/tmp/ttstt-assets-venv/bin/python -m pip install onnx==1.23.2 numpy==2.5.4
/tmp/ttstt-assets-venv/bin/python tools/prepare_bundle.py
python3 tools/prepare_bundle.py --verify
```

Run these commands from `apps/ttstt`. The script accepts `--model-dir` for a
different host cache, verifies reusable files, and never uploads or publishes.
It does not compile a Blueprint. The compiler and narrow ONNX preparation
script are retained here so a build does not depend on the sibling host tool.

## Kokoro weights and Heart styles

The underlying [hexgrad/Kokoro-82M model](https://huggingface.co/hexgrad/Kokoro-82M)
declares Apache-2.0. Its model card at revision
`f3ff3571791e39611d31c381e3a41a3af07b4987` is retained in
`licenses/KOKORO-82M-MODEL-CARD.md`; the Apache license text is retained as
`licenses/APACHE-2.0.txt`. Preserve these model notices with redistributed assets.
The first-party TRUEOS license does not replace the model's license.

The quantized ONNX input is the original
[taylorchu v0.2.0 release](https://github.com/taylorchu/kokoro-onnx/releases/tag/v0.2.0)
file `kokoro-quant-convinteger.onnx`, 92,361,271 bytes, SHA-256
`6e742170d309016e5891a994e1ce1559c702a2ccd0075e67ef7157974f6406cb`.
The export project's MIT notice is retained separately as
`licenses/TAYLORCHU-KOKORO-ONNX-LICENSE.txt`.

`prepare_kokoro_rten.py` converts only six DynamicQuantizeLSTM operators and
one FusedMatMul to standard operators, preserving the remaining quantization.
Its deterministic `kokoro-rten.onnx` output is 124,604,222 bytes, SHA-256
`239d9f4df112a375bea52146570b97eb5c5af727c007761ee121ed123fd1ab29`.
`compile_kokoro_aot.py` converts that graph and authenticates the complete voice
archive before producing the pinned native KKAOT artifact.

The voice archive is fetched from the official
[kokoro-onnx model-files-v1.1 release](https://github.com/thewh1teagle/kokoro-onnx/releases/tag/model-files-v1.1).
That release's published SHA-256 matches the input above. Retaining the original
archive is unnecessary in the distributed Blueprint; only Heart is embedded.

## English G2P

The byte-identical English model is available in the upstream
[g2p2-group-germanic 0.2.2 Python wheel](https://pypi.org/project/g2p2-group-germanic/0.2.2/).
The pinned wheel is 20,585,622 bytes, SHA-256
`2b90141f4c5b3c9f9d1d9848eeb6ed2d784290bb311c569b64660b26f2b44219`.
The script extracts exactly `g2p2_group_germanic/en.g2p.xz`, decompresses it,
and verifies the resulting English model against the runtime pin.

The publisher's package metadata declares MIT OR Apache-2.0. Its metadata,
download URL, and distribution hashes are retained in
`licenses/G2P2-GERMANIC-METADATA.json`; the inherited g2p2 copyright/MIT notice
is retained in `licenses/G2P2-MIT.txt`. The upstream g2p2 package describes
model sources as WikiPron, epitran, and LLM data. The original GitHub repository
was unavailable during this preparation; no additional per-language training
data notices were included in the wheel. This records the verified publisher
metadata without replacing upstream source-data attribution.
