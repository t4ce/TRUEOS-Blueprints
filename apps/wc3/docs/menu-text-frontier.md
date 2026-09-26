# Menu text and draw evidence (2026-09-26)

The captured run `wc3-20260926-194436.baremetal.snapshot.log` records one
wglCreateContext, one wglMakeCurrent, three glClear calls, two wglSwapLayerBuffers
calls, and no glDrawElements, glTexImage2D or glTexSubImage2D calls. The live
capture still had those counts at inspection. OPENGL CALL logging is unconditional
for providers in asupersync_child_vmcall_special_cases.rs, independent of trace-api.
These are provider-boundary counts, not measurements of native instructions or
proof of a rendered menu. glDrawBuffer selects a buffer; it is not geometry submission.

The logged DTTEXT records are the launcher copyright text. No menu-label text
request was established from this capture.

Read-only extraction of the local War3Patch.mpq with mpyq found:

- UI\\FrameDef\\Glue\\MainMenu.fdf
- UI\\FrameDef\\GlobalStrings.fdf

The main-menu button definitions resolve as follows (color/highlight markup removed):

| Frame | Text key | English text |
|---|---|---|
| SinglePlayerButton | KEY_SINGLE_PLAYER | Single Player |
| BattleNetButton | KEY_BATTLE_NET | Battle.net |
| LocalAreaNetworkButton | KEY_LOCAL_AREA_NETWORK | Local Area Network |
| OptionsButton | KEY_OPTIONS | Options |
| CreditsButton | KEY_CREDITS | Credits |
| ExitButton | KEY_QUIT | Quit |

The resources are present in the patch archive; this does NOT establish that
this run loaded them or created their frames. The base archive's corresponding
files are encrypted and were not decoded by mpyq. No assumption about archive
precedence or the runtime-loaded resource is needed for the above inventory.

The matching Game.dll binary has a concrete probe site:

- 0x6f24a2b3 pushes SinglePlayerButton (string VA 0x6f810e1c).
- 0x6f24a2bd calls 0x6f248560; the returned object is kept in ESI and later
  stored in [EBX+0x224].
- 0x6f24a2e2 pushes BattleNetButton (string VA 0x6f810e0c).
- 0x6f24a2ef calls the same helper; the next field is [EBX+0x228].
- 0x6f24a314 pushes LocalAreaNetworkButton.

A narrowly placed guest execution probe at these calls/returns could establish
whether menu-button binding is reached and whether it returns an object. This is
not yet instrumented: the new read-only debug mem command reads bytes, not execution
history. Further probing of FDF text resolution would distinguish a successful
button lookup from a resolved text label, and neither alone proves a draw submission.

Reproduce binary inspection:

```sh
rizin -q -e scr.color=0 -c 'pd 45 @ 0x6f24a29d' '/home/t4ce/Programmme/XPvm/tree/Warcraft III/Game.dll'
```

Archive-reader source/API: https://github.com/eagleflo/mpyq

## Live read-only confirmation after debug-pack relaunch

On 2026-09-26 the operator-authorized sequence was `vmx_stop`, confirmation of
stopped lifecycle, a 3.002-second wait, then `wc3`. `debug help` confirmed the
new pack. Shell2 capture remained connected throughout. The per-run source
byte offsets are in TRUEOS `bld/baremetal-logs/wc3-debug-relaunch-offsets.json`.
Immutable evidence is `wc3-debug-menu-live.snapshot.log`, with its hash and
exact decoded strings in `wc3-debug-menu-live.evidence.json` in that directory.

This run reaches the same two-event polling state, but live memory now proves
more than the earlier provider trace:

- `[0x6f87248c] = 0x09090090`: CGlueMgr getter at 0x6f241d70 reads this global.
- Manager +0x180 = 2; +0x184 = 0x0b1e0090. The screen table at
  0x6f80ba50 uses 12-byte entries; index 2 names `MainMenuFrame`.
  0x6f241f90 writes the selected state and returned frame into those fields.
- Main frame vtable = 0x6f71ebe0. Its button fields, matched against
  0x6f24a280's actual named lookups/stores, are populated:

| Button | Main frame offset | Button object | Text object | String buffer |
|---|---|---|---|---|
| Single Player | 0x224 | 0x0b3904c8 | 0x0b2c09d8 | 0x0b2e01e8 |
| Battle.net | 0x228 | 0x0b3906d0 | 0x0b2c0c20 | 0x0b2e0210 |
| Local Area Network | 0x230 | 0x0b3908d8 | 0x0b2c0e68 | 0x0b2e0230 |
| Options | 0x234 | 0x0b390ae0 | 0x0b2c10b0 | 0x0b2e0258 |
| Credits | 0x238 | 0x0b390ce8 | 0x0b2c12f8 | 0x0b2e0278 |
| Quit | 0x23c | 0x0b390ef0 | 0x0b2c1540 | 0x0b2e0298 |

The observed pointer chain is button +0x1e4 -> text object, text +0x1e8 ->
string buffer. The first text object has vtable 0x6f7082e0; its destructor
0x6f020b60 leads to CTextFrame code (source-name string in the binary).
All six live, NUL-terminated strings match the English resources above,
including `|Cffffffff...|R` markup. This establishes populated menu objects and
resolved label data, not execution of their draw methods or displayed pixels.

The fresh run still records only three glClear and two wglSwapLayerBuffers
calls, with no glDrawElements, glTexImage2D or glTexSubImage2D provider calls.
The sampled main window has nonzero guest-set user_data, paint_pending=false,
and the child message queue is empty.

The engine work item at 0x066400b8 is also real: a sampled stack contains the
return 0x0044468c after the call to 0x00444920, with this item as ESI's saved
value. Its event-5 callback list contains 0x6f03d2f0 (time accumulation) and
0x6f292290 (network update); event-6's list is empty. Do not confuse the engine's
numbered callback lists with the two Win32 event handles. These observations
narrow investigation toward frame update/render registration and activation;
they do not yet identify the missing transition or authorize fabricated signals.

`debug stack` correctly refused the sample because guest TIB StackBase and
StackLimit were both zero. Subsequent explicit mapped-memory reads are recorded
as MEMORY, not a validated backtrace. All investigations above were read-only.
