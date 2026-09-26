# Window notification probe audit — 2026-09-26

No notification has been injected yet. The running pack supports read-only
inspection; the next pack adds `debug post`. The disconnected Shell2 capture was
reattached at 20:21 without restarting WC3. Capture resumed in
`TRUEOS/bld/baremetal-logs/wc3-shell2-20260926-202134.raw.log`.

## First probe: size, not activation

Live `[0x6f862bc8] = 0x064900c8`; its vtable is 0x6f70c3f8.
Vtable +0x38 points to 0x6f0d5e90. That function computes
`(float[+0xbf0] - float[+0xbe8]) * (float[+0xbec] - float[+0xbe4])`
and succeeds only when the area is greater than 1.0. The live 16 bytes at
0x06490cac (object +0xbe4) are all zero, including after reconnect.

Engine callback slot 17 contains Game.dll 0x6f03d330. It calls 0x6f0c8ab0,
which invokes that vtable method, and takes an early return when it returns zero.
The WM_SIZE branch at 0x6f0cbae4 writes precisely these four float fields from
the message's width/height. This connects an observed zero-sized rectangle to a
specific render eligibility gate. It does not prove that fixing this gate is
sufficient to render a whole menu.

First experiment after repack/relaunch:

```
debug state
debug mem 2 0x06490cac 16
debug post 2 0x57434003 5 0 0x05a00a00
```

Use the current run's HWND/object/dimensions, not blindly these addresses. They
were verified here as 2560x1440. Send only this notification, then inspect guest
DispatchMessage completion, the four floats (expected 0,0,1440,2560), OpenGL
provider activity and any frontier. Do not send the activation series until its
result is understood. This is queued experimental delivery, not a claim to
implement the full synchronous Win32 resize/activation lifecycle.

Default WM_SIZE processing now returns zero so the actual guest handler can
complete. Other unimplemented defaults remain frontiers. In particular, do not
replace DefWindowProc(WM_ACTIVATE) with an unconditional zero: its default
processing includes focus behavior.

References: [WM_SIZE](https://learn.microsoft.com/en-us/windows/win32/winmsg/wm-size)
defines packed client dimensions; [Wine default procedure](https://github.com/wine-mirror/wine/blob/master/dlls/win32u/defwnd.c)
has zero fallback for WM_SIZE and explicit WM_ACTIVATE focus processing.

## Bound window handlers

Live Game.dll wndproc 0x6f0cba40 forwards to `[0x6f862c00] = 0x6f00af34`,
a thunk to War3.exe ordinal 493, VA 0x0041f590. The latter's optional further
callback `[0x0045db90]` is zero in this run.

| Notification | Observed binary action |
|---|---|
| WM_CREATE (1) | Guest attaches lpCreateParams through GWL_USERDATA; already delivered. |
| WM_DESTROY (2) | Releases resources/hides; excluded from probes. |
| WM_SIZE (5) | Writes render rectangle, forwards; War3 records dimensions. |
| WM_ACTIVATE (6) | War3 enqueues internal activation kind 6, then default processing. |
| WM_SETFOCUS (7) | Conditional device/DC reacquisition; returns locally. |
| WM_KILLFOCUS (8) | Conditional release/reset; returns locally. |
| WM_PAINT (15) | BeginPaint/EndPaint; already completed. |
| WM_ERASEBKGND (20) | Returns zero locally. |
| WM_CLOSE (16) | War3 queues shutdown; excluded. |
| WM_ACTIVATEAPP (28) | War3 stores wParam at 0x454774; it is already 1. |
| WM_DISPLAYCHANGE (126) | Conditional dimension bookkeeping. |
| WM_SYSCOMMAND (274) | Game filters screensaver/monitor-power; War3 handles close/menu; excluded. |

Mouse, keyboard and IME branches are excluded from the proposed experiments.
SHOWWINDOW/NCACTIVATE are not special render triggers in these two procedures;
they reach the default path. The probe parser accepts bounded scalar forms but
that does not assert their default semantics are implemented.

The activation path is specifically 0x41f5e1 -> 0x41fcc0 (internal kind 6),
consumed by 0x446ef0 -> 0x4471f0 -> engine callback slot 2 -> 0x443980.
These internal kind/slot numbers are not Win32 message IDs or event handles.

## Complete callback-slot inventory for the sampled engine item

Item 0x066400b8: 29 list heads beginning at +0x5c, stride 12, before +0x1b8.
Each listed function is read from live nodes at +8. This includes input slots
for completeness, without proposing to invoke them. Slot 5 contains time and
network updates; slot 7 startup; slot 4 teardown; slot 17 the render gate above.
Unknown slot semantics are deliberately left unnamed.

| Slot | Bound callbacks |
|---|---|
| 0 | empty |
| 1 | 0x004434b0 |
| 2 | 0x00443980 |
| 3 | empty |
| 4 | 0x6f006720 |
| 5 | 0x6f03d2f0, 0x6f292290 |
| 6 | empty |
| 7 | 0x6f006220 |
| 8 | 0x00443500 |
| 9 | 0x004435a0, 0x6f03d310 |
| 10 | 0x00443550 |
| 11 | 0x004435f0 |
| 12 | 0x004436f0 |
| 13 | 0x004437f0 |
| 14 | 0x00443670 |
| 15 | 0x00443870 |
| 16 | 0x00443770 |
| 17 | 0x6f03d330 |
| 18 | empty |
| 19 | empty |
| 20 | empty |
| 21 | empty |
| 22 | empty |
| 23 | empty |
| 24 | 0x6f28a7b0 |
| 25 | 0x6f28a810 |
| 26 | empty |
| 27 | 0x004438f0 |
| 28 | 0x00443940 |

## Hardware result: one WM_SIZE advances past the idle loop

The new pack was launched after a confirmed `vmx_stop` and three-second wait.
`debug help` confirmed `debug post`. Startup completed, followed by repeated
polls at both known sites, an empty message queue, paint_pending=false, and
another direct read of sixteen zero bytes at 0x06490cac.

Exactly one experiment was submitted:

```
debug post 2 0x57434003 5 0 0x05a00a00
```

The chronological capture establishes:

1. POST QUEUED, then PeekMessageA finds the message.
2. GetMessageA removes message 5.
3. TranslateMessage and DispatchMessage pass HWND 0x57434003, message 5,
   wParam 0, lParam 0x05a00a00 to the actual guest wndproc.
4. Guest GWL_USERDATA resolves to 0x064900c8.
5. Both wndproc and DispatchMessage return zero.
6. New GetWindowRect, glViewport, glDepthRange, glScissor, glMatrixMode and
   glLoadMatrixf calls follow. The projection matrix contains 2.5 and
   3.3333333 scaling values.
7. Execution stops at a NEW provider frontier: MSVCRT.dll!iswspace,
   provider 796, EIP 0x00302558, ESP 0x043ffa08, caller 0x6f0df3f2,
   last_exec_seq=133048. No other notification was sent.

This is direct evidence that the size notification breaks the old idle loop.
It does not prove a visible menu frame: no new geometry submission or swap was
recorded before the new frontier. The post-frontier memory request could not be
serviced because the execution loop had returned; do not claim an observed
post-message memory dump. Independently, the native IA32 test now executes the
actual size handler and render gate and verifies float fields 0,0,1440,2560,
gate transition 0->1, and preserved stack/nonvolatile registers.

Evidence under TRUEOS `bld/baremetal-logs/`:

- `wc3-size-probe.before.log`
- `wc3-size-probe.after.log`
- `wc3-size-probe.result.json` (offsets, hashes, GL counts, exact command)
- `wc3-size-probe-offsets.json` and `wc3-size-injection-offsets.json`

The next compatibility change should deliver the initial size notification at
the appropriate window lifecycle point, through guest code, rather than assign
the private rectangle or require a manual debug command. The newly reached
iswspace provider is a separate next frontier.
