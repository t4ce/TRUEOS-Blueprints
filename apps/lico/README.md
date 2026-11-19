# Lico

Lico is an on-demand TRUEOS Blueprint for live-coding sound in a terminal.
The starter patch plays a sequenced lead and kick. The compiler embeds the
Glicol engine, parser, DSP code, node reference, and four samples. Sample
playback requires no runtime file access.

From the Blueprint SDK root, build a local package with:

```sh
TRUEOS_BLUEPRINT_SKIP_APPS_PUBLISH=1 cargo bp lico
```

The package is written to `dist/lico.bp`. Launch `lico` from Shell3 once the
Blueprint is installed. Open `http://<TRUEOS-IP>:82/` to listen through the
kernel audio server, then press **Alt+R** to play or apply edits. **Alt+S**
stops playback, **Alt+D** shows node help, **Alt+[** and **Alt+]** change
tempo, and **Esc** returns to the shell.

The editor and renderer run cooperatively on the same current-thread Tokio
runtime. Each audio tick renders up to four 128-frame blocks and hands one
48 kHz stereo PCM batch to the kernel. The sink accepts one producer at a time
and caps each stream at two minutes. Lico lets the tail drain before opening
the next stream, so rollover can introduce a short gap.

To verify the embedded starter on the development host:

```sh
cargo run -p lico -- --self-check
```

The desktop TUI preview renders its waveform and spectrum without a host
audio backend. Rhai `meta` nodes are supported. The engine source under
`vendor/` comes from the [Glicol fork](https://github.com/t4ce/glicol/tree/0317db2e3f157b911bfc28c72ad5db90db963f24)
and retains its MIT licenses. Lico caps each Rhai evaluation at 25,000 operations
so a runaway script cannot hold the cooperative executor indefinitely.

## Codex editing

On TRUEOS, launch `lico --mcp` in Shell3. Lico shows a Codex launch command
with a key for that Lico session. Run it in another Shell3 slot. Append
`resume --last` if you want to continue your last Codex conversation. **Alt+M**
reopens the connection panel. A running Codex process keeps its existing tool
configuration, so resume or reopen it with the displayed command.

The listener is bound to `http://127.0.0.1:42127/mcp`. The launch command
uses Codex's `-c` overrides and needs no persistent Codex configuration.
The displayed command contains the session key, which stops working when Lico
exits. For scripted launches, setting `LICO_MCP_TOKEN` still enables bearer
authentication on the same endpoint.

Codex can read the draft and its revision with `get_patch`, check Glicol syntax
with `validate_patch`, and send a proposal with `propose_patch`. In Lico,
**Alt+P** opens the proposal diff. Use **Alt+A** to accept it into the draft,
**Alt+X** to reject it, or **Esc** to close the review. **Alt+R** applies the
draft to the running audio engine. Editing the draft while a proposal is
pending clears that proposal so it cannot overwrite newer work.
