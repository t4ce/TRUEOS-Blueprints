# Dobby

Dobby is a Cerebras-backed TRUEOS Blueprint. All conversation policy, the
five-second autonomous loop, the serialized idea queue, tool-call handling,
and ten-turn summary rollover live in the app. The kernel only supplies generic
HTTPS and silent Spirit presentation capabilities.

On first launch the app creates `config.json` in its persistent TRUEOSFS app
root (`apps/dobby/config.json` for the default instance). Put a Cerebras API
key in that file; `CEREBRAS_API_KEY` is also honored if the launcher already
provides it in the Blueprint environment. The key is never printed.

The requested default cadence is `loop_interval_ms: 5000`. Cerebras currently
lists the Free Trial at 5 requests/minute. For autonomous RPM alone, `15000`
averages about 4.4 requests/minute after the rollover summaries. Direct user
requests and token/hour/day quotas still count separately, so this is not an
unlimited 24/7 free setting. Developer-tier limits can comfortably keep five
seconds. See the [official rate-limit table](https://inference-docs.cerebras.ai/support/rate-limits).

Launch and start the autonomous loop with:

```text
apps start dobby --vmx-minishell start
```

The `--vmx-minishell` marker is required for interactive app commands. Within
that Matrix slot, bare commands go to Dobby; VM controls use the `vmx` prefix
(for example, `vmx stop`). An optional `dobby` prefix is accepted too.

- `start` / `dobby start` — run autonomous turns at the configured start-to-start cadence (five seconds by default).
- `stop` / `dobby stop` — stop autonomous turns without deleting the current conversation.
- `reset` / `dobby reset` — clear all current turns and the propagated summary. If running,
  Dobby continues from a fresh chat.
- `reload` / `dobby reload` — reload `config.json` after changing the key, model, reasoning effort, or cadence.
- `status` / `dobby status` — show redacted state.
- `dobby <request>` — atomically stop the loop and serialize a user turn.
- `quit` / `dobby quit` — leave the app.

To launch without starting the loop, omit the trailing `start`.

Text uses the silent Spirit ingress. This Blueprint never calls the Lumen reply
presenter or any TTS/audio API.
