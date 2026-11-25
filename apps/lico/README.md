# Lico

Lico is an on-demand TRUEOS Blueprint for live-coding sound in a terminal.
The starter patch plays a sequenced lead and kick. The compiler embeds the
Glicol engine, parser, DSP code, node reference, examples, and fourteen samples.
The examples and samples require no runtime file access. Follow a patch from
trigger to stereo output in the [animated DSP cheat sheet](assets/lico-dsp-cheat-sheet.gif).

From the Blueprint SDK root, build a local package with:

```sh
TRUEOS_BLUEPRINT_SKIP_APPS_PUBLISH=1 cargo bp lico
```

The package is written to `dist/lico.bp`. Launch `lico` from Shell3 once the
Blueprint is installed. Open `http://<TRUEOS-IP>:82/` to listen through the
kernel audio server, then press **Alt+R** to play or apply edits. **Alt+S**
stops playback, **Alt+D** shows node help, **Alt+[** and **Alt+]** change
tempo, and **Ctrl+Q** returns to the shell.

Left-click in the patch editor to place the caret in Shell3 or over SSH.

Right-click Lico’s Shell3 window and choose **Add Drum**, or press **Alt+I**.
This adds an audible kick track to the draft. Press **Alt+R** to apply it.
The drum reference is displayed as `~🥁` using Microemoji. Older patches
can still use `~drum`. Lico converts that spelling when you run the draft.

A successful run colors the parsed patch and briefly highlights its border.
The colors track the parsed draft until you edit it or load another patch.
On a local UI4 Shell3 window, thin amber outlines follow the current `seq`
notes and rests as Lico renders audio. Editing the draft clears the playheads
until the next successful run.

Press **Alt+V** to open or close Lico's native UI4 visualizer on the TRUEOS
display. It runs in its own window at a target of 30 Hz. A precompiled Intel
shader draws the output waveform, a 40-band spectrum, and up to eight
sequencer lanes driven by the running graph's actual playheads and hits.
**Esc** dismisses the visualizer while it is open and keeps playback and
the editor running. With the visualizer closed, **Esc** returns to the shell.
The visualizer needs the matching TRUEOS kernel shader catalog. Its worker
samples the latest audio snapshot and handles GPU submission separately.
For fullscreen profiling, the kernel entry `lico visualizer render` reports
the backing size, strip count, page-table reuse, and render timings every 120
frames.
To check the embedded shader against its source and kernel contract, run
`python3 apps/lico/tools/bake_visualizer.py --check` from the Blueprint root.

Press **Alt+E** to browse the embedded patches. Use **↑** and **↓** to
select an example, then press **Enter** to replace the draft. **Alt+R**
plays it. The patches cover sampled and synthesized drums, filter bass,
FM, stereo modulation, delay, sample patterns, automation, and Rhai DSP.

**Acoustic drum kit** plays a two-bar beginner groove on a five-piece kit.
The first bar covers kick, snare, and closed, pedal, and open hi-hats. The
second adds crash, ride, and a descending fill across the three toms. Each
sound has its own editable sequence and level. The recordings come from
[Virtuosity Drums](https://versilian-studios.com/virtuosity-drums/).

**Tidepool** is an eight-bar groove inspired by eddyflux's “coastline” in
[the Strudel workshop](https://strudel.cc/workshop/getting-started/#examples).
It combines the acoustic kit, four soft chord voices, a synthesized
upright-style bass, and an FM melody with stereo delay. The bass layers a
filtered triangle body, a short second harmonic, and the sound of a finger
attack made from filtered noise. A muted ride accents the second half, and
the melody answers the chords in bars 3–4 and 7–8. Edit the named chains in
[the patch](assets/examples/tidepool.glicol) to change individual parts.

Choose **Synthesized piano** from the examples and press **Alt+R**. Play
your MIDI keyboard or press **Alt+M** to hear the built-in test phrase.
`midisynth 0 0.003 0.16 piano` models struck strings with a brighter attack
at higher velocities and a natural decay while a key is held. It generates
each note in real time from oscillators.

Choose **Piano and 808** to play the same piano over a kick, snare, and
closed hi-hat loop. A second voice answers each key-down with a fifth,
octave, and fifth above the played note. Use `~beat` to adjust the drum
level and the offsets in `midiarp` to change the answer.

The **MIDI instrument** example uses the default sine voice. Both voices
take attack and release times in seconds. Use channel `0` to listen on
every channel or `1`–`16` to choose a channel. The test phrase uses channel `1`.

Press **Alt+N** to open MIDI input. Use **↑** and **↓** to choose a source
and **Enter** to apply it. You can switch while a patch is playing.

- **USB MIDI** is the default on TRUEOS. Lico reads note edges from the
  kernel's MIDI input ring. The current kernel enables the Casio CTK-3500.
- **Wi-Fi piano** opens UDP port 9696. Point the sender at the TRUEOS
  address. Lico accepts the existing TPNO binary and `piano mask=…` text
  formats and plays them on channel 1. This uses the kernel's Lico UDP
  9696 networking rule.
- **Off** disconnects the live input. **Alt+M** can still play the test
  phrase on a running MIDI patch.

**Cosmic soundscape** adapts the Games Blueprint's ambient audio with a
sweeping stereo tone, a five-voice pad, filtered surf, and sparse chimes.
At 120 BPM, the first chord starts to glide after 48 seconds. The chimes
use a repeating phrase whose pitches, timing, and panning are editable.

The looping example mixes are leveled near −22 LUFS integrated at the default
120 BPM. Adjust the final `mul` in an example's `out` chain to change
its whole mix, or change a track's own `mul` to rebalance it. Lico renders
these gains into PCM and the port 82 player receives that PCM over HTTP.

At 80 columns and above, the waveform and spectrum stack to the right
of the editor. On narrower terminals they sit side by side below it.
Below 25 rows, the editor fills the screen, and **Alt+E** still opens
the example picker.

The editor and renderer run cooperatively on the same current-thread Tokio
runtime. Each tick submits 48 kHz stereo PCM to the kernel. Regular patches
render four 128-frame blocks per tick. MIDI patches poll input and render two
blocks per tick, about 5.33 ms apart.

For live monitoring through the rig's mainboard jack, enable Lico's **Jack**
route in **Audio Link**. When Lico opens a stream for a MIDI patch, a supporting
TRUEOS kernel uses a 32 ms prebuffer for that jack. Earlier kernels accept the
ordinary stream request. The port 82 player uses the HTTP route, which
prebuffers 120 ms in TRUEOS and schedules audio ahead in the browser.

The sink caps each stream at two minutes. Lico lets the tail drain before
opening the next stream, so rollover can introduce a short gap.

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

Codex reads the current draft and revision with `get_patch`. Before applying a
musical change, it can use `audit_patch` to render the candidate and the live
patch at the current tempo in separate, silent Glicol engines. When playback
is stopped, the editor draft supplies the comparison. The report shows peaks,
RMS levels, clipping, frequency balance, and how the levels change over the
beat grid, with level and frequency balance differences calculated for Codex.
An optional `focus_chain` measures one instrument alongside the mix.
The report gives Codex measurements to reason from and leaves the listening
judgment to the DJ.

The audit covers the first eight seconds by default. Use `start_seconds` and
`seconds` for a later section, such as an eight-second break at 48 seconds.
Each audit starts the engines at the beginning and runs them to the chosen
window. It measures the generated patch audio with zero external MIDI events.

`apply_patch` replaces the draft and restarts live playback from the beginning.
`validate_patch` is available for a syntax check. If the draft changes in the
meantime, Codex needs to read the new revision before applying.

Press **Ctrl+U** in Lico to restore the draft and playback state from before
the latest Codex edit. If you typed after the Codex edit, **Ctrl+U** undoes
your typing first. **Alt+R** and **Alt+S** control playback for manual editing.
