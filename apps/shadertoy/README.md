# Shadertoy archive blueprint

Fetch one or more public Shadertoy URLs and preserve their complete render-pass
graph under a directory named after each shader:

```text
shadertoy https://www.shadertoy.com/view/mslfR2
shadertoy -o shaders <url-or-id> [<url-or-id> ...]
```

If Shadertoy blocks its legacy page endpoint, create an app key at
`https://www.shadertoy.com/myapps` and pass it through the launch environment:

```text
SHADERTOY_API_KEY=<key> shadertoy <url-or-id>
```

`--api-key <key>` is also accepted. The key is used for the official API
request and is never written to the archive.

Each shader-title directory contains the untouched HTTP response, the complete
shader and info JSON, and one ordered directory per render pass. Every pass
retains `code.glsl`, `inputs.json`, `outputs.json`, and `pass.json`; this covers
Image, Buffer, Common, Sound, texture/cubemap/media channels, keyboard,
microphone, webcam, and future input types without narrowing their schema.
