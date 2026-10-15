# ratatest

A small, deterministic TRUEOS Blueprint for exploring Ratatui 0.30.2 and the
terminal patterns used by the workspace Codex Rust CLI. No model, credentials,
network service or account is needed by the demo itself.

Build and publish this app from the Blueprint SDK root:

```sh
cargo bp ratatest
```

In Shell3, submit `§§ratatest` to fetch and launch the published app. After it
is installed in AppDB, `ratatest` launches it locally. This uses the normal
terminal lease, acknowledges the first rendered frame, restores terminal modes
on exit, and supports F10 parking and subsequent Matrix-slot reentry. The app
is not marked replicatable.

On a desktop terminal:

```sh
cargo run -p ratatest -- --tour
cargo run -p ratatest -- --stress --load 3
cargo run -p ratatest -- --case damage-storm
cargo run -p ratatest -- --case combined
cargo run -p ratatest -- --inline
cargo run -p ratatest -- --self-check
cargo run -p ratatest -- --stress-check 1200
cargo test -p ratatest
```

The five sections contain 29 cases. The app opens on the new Arena menu:

| Section | Coverage |
| --- | --- |
| Scenarios | Assistant session, dashboard, file/command chooser, help dialog |
| Widgets | Block, Paragraph, Tabs, List, Table, Scrollbar, Gauge, LineGauge, Sparkline, BarChart, Chart, Canvas, Monthly calendar, Clear, Fill, RatatuiLogo, RatatuiMascot |
| Codex patterns | Styled prose/code, tool output/diffs, editable composer, approval/search chooser, spinner/status/footer, streaming/wrapping/selection, redraw/Unicode/custom Buffer widget, combined session |
| Terminal / ANSI | Cursor/erase/SGR, protected history scroll region, capability/default-color probes, terminal/input modes, event inspector |
| Arena | Mission control, patch review/approval, reflow/Unicode lab, damage storm |

The gallery includes all 12 border types, all nine marker types, all four chart
graph types, named/indexed/RGB colors, underline colors, all style modifiers,
stateful selections and scrollbars, overlapping layers, wide characters,
combining characters and emoji. It uses the restored vendored Ratatui sources
under `vendor/ratatui`, including the optional calendar widget, and the SDK's
TRUEOS Crossterm 0.29 backend.

Use `1`–`5` to choose a section, `n`/`p` for next/previous case, Enter to open,
and `h` to show the selection menu. Up/Down changes the selection; PgUp/PgDn
scrolls the transcript. `i` edits, Enter submits, `o` toggles an overlay,
Space freezes animation, `t` starts/stops the tour, and `r` forces a fresh
draw. `q` or Ctrl-Q quits. F10 returns to Shell3 while retaining app state.
Tab/Shift-Tab are also supported by the app; Shell3 currently reserves Tab for
leaving the active VM, so `n`/`p` work on every frontend.
Native UI4 also reserves F10 globally; app F10 parking is available when the
frontend delivers that key, including SSH.

## Arena and live stress

Press `s` to start the Arena stress tour. It opens mission control at load 2
and cycles through the four scenes every six seconds, with periodic overlays.
Press `s` again to stop cycling. `+`/`-` chooses load 1/2/3, targeting
10/20/40 animation updates per second. Space freezes both animation and
automatic case changes. Manual `n`/`p` or a section selection stops the stress
tour; `t` starts the slower tour of every case. Editing temporarily holds
automatic case changes so the composer stays visible while you type.

Mission control combines a task sidebar, streaming transcript, patch and
composer with graphs and an observation rail. Patch review pairs old/new code
with file selection, tool text and approval layering. The reflow lab combines
styled wrapping, CJK/emoji/combining specimens and grapheme-width inspection.
Damage storm changes an RGB cell field, bars, long/short text and moving
overlapping panels while four corner sentinels remain fixed. `o` toggles the
overlay workload; `i`, Enter, selection and scrolling remain usable.

The scenes label their simulated content. The observation rail shows actual
completed draw count, emitted backend bytes, last/peak draw duration, and draws
that exceeded the selected cadence. These are application/backend measurements,
not GPU paint or display frame rates. Paused screens redraw only for input or
resize, so they do not create an artificial stream of benchmark samples.
At least 100×32 is useful; 120×40 gives the full layout. Smaller surfaces use
fewer panes, and tiny surfaces retain a short status view.

The tour only visits visual cases. Press `a` in an ANSI case to run raw escape
specimens outside Ratatui's frame buffer. Esc or `q` returns, or a 20-second
timeout restores the modes. Raw probes use the whole terminal; use at least
40×18. They restore scrolling margins, colors, cursor visibility/shape, mouse,
focus, modifyOtherKeys and the keyboard enhancement stack. Clipboard writes
are deliberately absent.

## Codex source mapping

Paths below are relative to `../codex/codex-rs/tui/src`:

| Demo | Codex implementation |
| --- | --- |
| Text, wrapped transcript, custom Buffer rendering | `render/renderable.rs`, `history_cell/`, `markdown_render.rs` |
| Composer and selection | `bottom_pane/chat_composer.rs`, `bottom_pane/textarea.rs` |
| Chooser/approval, status, footer | `approval_overlay.rs`, `status_indicator_widget.rs`, `bottom_pane/footer.rs` |
| Diff, wide-cell replacement, styled blanks, cursor | `custom_terminal.rs`, `custom_terminal/cursor.rs` |
| Scroll-region history insertion | `insert_history.rs`, `tui/scrollback.rs` |
| CPR, DA, default-color and keyboard queries | `terminal_probe.rs`, `tui/keyboard_modes.rs` |
| Alt-screen, focus, paste, mouse, synchronized output | `tui.rs`, `tui/alternate_screen.rs` |

This app recreates the rendering/control patterns with small fixtures. It does
not embed Codex's renderer, Markdown parser or full product flows. Codex has
its own terminal diff implementation, so a passing Ratatui backend case does
not prove every Codex-specific diff behavior.

The raw modes probe includes OSC8 hyperlinks and synchronized updates. Other
Codex extensions worth testing separately are OSC52 clipboard, OSC9
notifications, OSC22 pointer shape, and OSC7501/21337 terminal status. These
are distinct from Ratatui widgets and are not advertised as native support.

## Native compatibility findings

SSH sends app ANSI bytes to the user's terminal. Native UI4 parses those bytes
through `trueos-terminal` and paints the resulting cells. Compare both paths;
an SSH screenshot alone cannot validate TRUEOS's parser or glyph rendering.

Three confirmed issues have regression fixes in TRUEOS:

* SGR 7/27 inverse video was discarded. The parser now retains it, and Shell3
  swaps resolved foreground/background colors, including the visible cursor's
  second inversion and underlines.
* Unsupported CSI namespaces were interpreted as ordinary cursor controls.
  CSI `?u`, `>…u` and `<u` could restore the cursor; secondary DA could receive
  a false primary identity. Unsupported prefixes/intermediates are now ignored
  without moving the cursor or inventing a capability response.
* CSI `@`, `P`, and `X` were ignored. Insert/delete/erase-character controls now
  edit only the current row, clamp counts, retain shifted cell styles and fill
  erased cells using the existing background erase policy. Zero/omitted counts
  mean one character; cursor and saved-cursor positions remain intact.

The specimens retain known native limitations so they remain easy to inspect:

| Pattern | Native limitation |
| --- | --- |
| Unicode | One Unicode scalar per cell; wide and combining grapheme semantics incomplete |
| Text modifiers | Bold, dim, italic, blink and strike are not represented by the native terminal cell renderer |
| Terminal state | Alternate-screen preservation, cursor shapes and synchronized-output buffering incomplete |
| Capabilities | OSC10/11 default colors and Kitty keyboard capability responses absent |
| Input/extensions | Native bracketed-paste/focus/enhanced keyboard semantics and OSC8 metadata incomplete |

No capability response is fabricated for unsupported native features. Braille
and block glyphs already have microfont support and should be checked in the
canvas, chart, gauge and mascot pages.

## Verification

`--self-check` renders every case/menu at five terminal sizes and three
animation phases (870 frames), including tiny 1×1 and 20×5 surfaces. Unit tests
check overlay closure/redraw and grapheme-aware composer clipping. The redraw
test compares visible cells; TestBackend does not clear hidden wide-glyph
continuation placeholders the way a physical terminal paints them.

Five compatibility tests capture the actual vendored CrosstermBackend output,
feed it to TRUEOS's production parser in three-byte chunks, and check shrinking
ASCII lines, inverse selections/reset, protected scroll regions, and safe
capability queries, plus the raw character-editing specimens. TRUEOS also has
parser and Shell3 projection regressions.

`--stress-check N` checks N frames at each of four sizes, twice, including
three real resizes per pass. It compares the parsed incremental output with
both the intended Ratatui buffer and a fresh full redraw, and feeds real
Crossterm bytes in 1/3/7-byte fragments. Output includes measured byte savings,
elapsed time, a deterministic digest and mismatch count. The equivalence
workload uses ASCII deliberately; a separate fragmented UTF-8 check verifies
transport without claiming native wide-grapheme support. The finite bound is
1–10,000 frames per size; the default is 600. A geometry adapter supplies the
headless terminal size and delegates escape output to the real backend.

The reusable SSH smoke test requires an already published package:

```sh
python3 apps/ratatest/verify_ssh.py --output /tmp/ratatest-ssh-check
python3 apps/ratatest/verify_stress_ssh.py --output /tmp/ratatest-live-check
```

It checks the menu, every case, composer, resize, all raw probes, restoration
and quit, and writes captured bytes plus JSON evidence. Its PTY capture
answers cursor-position queries so Ratatui's cursor-preserving clears can run;
those synthetic responses are recorded separately. It tests process/input/
geometry/escape transport, not native pixels or full terminal emulation.
The live stress verifier additionally observes automatic Arena transitions,
high-load animation, responsive input, resizing, freeze/restart and clean exit.

See [the first validation](validation/2026-10-15.md) and
[the stress-pass record](validation/2026-10-15-stress.md) for tested artifact
identities, results and evidence paths.
