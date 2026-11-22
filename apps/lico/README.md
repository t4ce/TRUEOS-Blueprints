# Lico

Lico is an on-demand TRUEOS Blueprint for live-coding sound in a terminal.
The starter patch plays a sequenced lead and kick. The compiler embeds the
Glicol engine, parser, DSP code, node reference, eleven example patches, and
four samples. The examples and samples require no runtime file access.

From the Blueprint SDK root, build a local package with:

```sh
TRUEOS_BLUEPRINT_SKIP_APPS_PUBLISH=1 cargo bp lico
```

The package is written to `dist/lico.bp`. Launch `lico` from Shell3 once the
Blueprint is installed. Open `http://<TRUEOS-IP>:82/` to listen through the
kernel audio server, then press **Alt+R** to play or apply edits. **Alt+S**
stops playback, **Alt+D** shows node help, **Alt+[** and **Alt+]** change
tempo, and **Esc** returns to the shell.

Right-click Lico’s Shell3 window and choose **Add Drum**, or press **Alt+I**.
This adds an audible kick track to the draft. Press **Alt+R** to apply it.
The drum reference is displayed as `~🥁` using Microemoji. Older patches
can still use `~drum`. Lico converts that spelling when you run the draft.

A successful run colors the parsed patch and briefly highlights its border.
The colors track the parsed draft until you edit it or load another patch.

Press **Alt+E** to browse the embedded patches. Use **↑** and **↓** to
select an example, then press **Enter** to replace the draft. **Alt+R**
plays it. The patches cover sampled and synthesized drums, filter bass,
FM, stereo modulation, delay, sample patterns, automation, and Rhai DSP.

**Cosmic soundscape** adapts the Games Blueprint's ambient audio with a
sweeping stereo tone, a five-voice pad, filtered surf, and sparse chimes.
At 120 BPM, the first chord starts to glide after 48 seconds. The chimes
use a repeating phrase whose pitches, timing, and panning are editable.

The example mixes are leveled near −22 LUFS integrated at the default
120 BPM. Adjust the final `mul` in an example's `out` chain to change
its whole mix, or change a track's own `mul` to rebalance it. Lico renders
these gains into PCM and the port 82 player receives that PCM over HTTP.

At 80 columns and above, the waveform and spectrum stack to the right
of the editor. On narrower terminals they sit side by side below it.
Below 25 rows, the editor fills the screen, and **Alt+E** still opens
the example picker.

The editor and renderer run cooperatively on the same current-thread Tokio
runtime. Each audio tick renders up to four 128-frame blocks and hands one
48 kHz stereo PCM batch to the kernel. The sink accepts one producer at a time
and caps each stream at two minutes. Lico lets the tail drain before opening
the next stream, so rollover can introduce a short gap.

To verify that every embedded patch renders audio on the development host:

```sh
cargo run -p lico -- --self-check
```

The host WAV export uses the same PCM renderer as live playback.

```sh
cargo run -p lico -- --render apps/lico/assets/examples/808.glicol /tmp/lico-808.wav 16
```

The desktop TUI preview renders its waveform and spectrum without a host
audio backend. Rhai `meta` nodes are supported. The engine source under
`vendor/` comes from the [Glicol fork](https://github.com/t4ce/glicol/tree/0317db2e3f157b911bfc28c72ad5db90db963f24)
and retains its MIT licenses. Lico caps each Rhai evaluation at 25,000 operations
so a runaway script cannot hold the cooperative executor indefinitely.

## Codex editing

Launch `lico` and Codex normally in separate Shell3 slots under the same
signed-in account. The TRUEOS kernel authorizes the connection by app and
account. If Codex is already open, it discovers Lico on the next request.
Use the updated kernel and both updated Blueprints.

Codex can read the draft and its revision with `get_patch`, check Glicol syntax
with `validate_patch`, and send a proposal with `propose_patch`. In Lico,
**Alt+P** opens the proposal diff. Use **Alt+A** to accept it into the draft,
**Alt+X** to reject it, or **Esc** to close the review. **Alt+R** applies the
draft to the running audio engine. Editing the draft while a proposal is
pending clears that proposal so it cannot overwrite newer work.
