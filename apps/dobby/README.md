# Dobby

Dobby is an OpenAI-compatible remote-inference TRUEOS Blueprint. All
conversation policy, the five-second autonomous loop, the serialized idea
queue, bounded multi-tool loop, and ten-turn summary rollover live in the app.
The Dobby-only Spirit/UI4 bridge supplies live brokered-window inventory,
Lilly-scoped focus, gridded selected-window capture, and serialized cursor and
keyboard operations. It is separate from Shell2 and Lumen and is restricted to
the Dobby Blueprint owner. Cerebras remains the default endpoint, but a
compatible multimodal facade can be selected in `config.json`.

Ordinary turns expose the three small Spirit actions (`text`, `play_emotion`,
and whole-screen `move`) plus six UI4 actions: list windows, focus a returned
opaque window ID, observe the focused window, point/click, type text, and press
a named key. An observation is an adaptively reduced PNG capped at 64 KiB with
a light `0..1000` grid; pointer coordinates use that same window-local scale.
PNG bytes exist only in the active request and are replaced by compact text in
conversation memory.

Spirit `move` coordinates are centre-relative: `(0, 0)` is the screen centre;
each axis is inclusive `-0.5..=0.5`. This is converted inside Dobby to the
kernel's existing `0..=1` scanout API.

One logical turn may use at most four remote rounds and sixteen ordered tool
calls (at most eight in one response). Busy Lilly cursor/keyboard programs are
retried serially for a bounded interval, so a returned `pointer -> type -> key`
batch cannot interleave half-actions. The completed logical turn is compacted
to one action record, keeping the ten-turn context small; only the existing
single 500-token summary request crosses a chat rollover.

On first launch the app creates `config.json` in its persistent TRUEOSFS app
root (`apps/dobby/config.json` for the default instance). The default template
targets the local remoteAI facade at `http://192.168.178.111:3042` and does not
require a key for private HTTP. `REMOTE_AI_API_KEY` is still honored for HTTPS
providers (`CEREBRAS_API_KEY` remains a legacy fallback). The key is never
printed.

HTTPS is required for remote providers. A local remoteAI facade on this trusted
LAN is configured explicitly here:

```json
{
  "api_key": "",
  "endpoint": "http://192.168.178.111:3042/v1/chat/completions",
  "allow_insecure_http": true,
  "model": "auto",
  "reasoning_effort": null,
  "loop_interval_ms": 5000
}
```

Insecure HTTP is accepted only for a literal `127.0.0.0/8`, `10.0.0.0/8`,
`172.16.0.0/12`, or `192.168.0.0/16` IPv4 address. Hostnames, public IPs, and
other schemes remain rejected. The opt-in defaults to `false`, including when
loading an older config without the field. A TRUEOS kernel whose generic JSON
POST path supports private HTTP is also required; HTTPS-only kernels will
reject the request even after Blueprint validation succeeds.

`reasoning_effort` defaults to `"low"` for `gpt-oss-120b`. Cerebras supports
`low|medium|high` for that model and `none` for `zai-glm-4.7`; set the value to
`null` to omit the parameter for another model and use its provider default.

The requested default cadence is `loop_interval_ms: 5000`. For the default
Cerebras endpoint, the Free Trial currently lists 5 requests/minute. For
autonomous RPM alone, `15000` averages about 4.4 requests/minute after the
rollover summaries. Direct user requests and token/hour/day quotas still count
separately, so this is not an unlimited 24/7 free setting. Other providers and
facades have their own limits. See the [official Cerebras rate-limit table](https://inference-docs.cerebras.ai/support/rate-limits).

Build and publish the Blueprint from the Ubuntu host with:

```sh
cargo bp dobby
```

Invoke it on TRUEOS with `online dobby` or `§§dobby`. The VMX minishell is
available automatically; only a TUI needs an additional process. Within that
Matrix slot, bare commands go to Dobby and VM controls use the `vmx` prefix
(for example, `vmx_stop`). An optional `dobby` prefix is accepted too. Use the
TRUEOS `stop`/`leave` lifecycle or Escape to leave the Blueprint. App-level
`start` applies to artifacts compiled directly on TRUEOS after invoking the
Rust compiler and registering them in the database; Dobby's command named
`start` below controls only its autonomous loop.

- `start` / `dobby start` — run autonomous turns at the configured start-to-start cadence (five seconds by default).
- `stop` / `dobby stop` — stop autonomous turns without deleting the current conversation.
- `reset` / `dobby reset` — clear all current turns and the propagated summary. If running,
  Dobby continues from a fresh chat.
- `reload` / `dobby reload` — reload `config.json` after changing the endpoint, transport opt-in, key, model, reasoning effort, or cadence.
- `status` / `dobby status` — show redacted state.
- `dobby <request>` — atomically stop the loop and serialize a user turn.
- `quit` / `dobby quit` — legacy in-app alias for leaving; prefer the TRUEOS
  `stop`/`leave` lifecycle or Escape.

Invocation leaves the autonomous loop stopped until `start` is entered.

Text uses the silent Spirit ingress. This Blueprint never calls the Lumen reply
presenter or any TTS/audio API. The private `remoteAI` Copilot facade accepts
the inline PNG and translates ordered multi-tool responses; providers without
vision support still work for the original non-visual Spirit actions.
