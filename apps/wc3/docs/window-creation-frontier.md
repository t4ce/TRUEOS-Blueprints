# Window creation and the two idle polls (2026-09-26)

Audited local inputs: `/home/t4ce/Programmme/XPvm/tree/Warcraft III/`.

- `Game.dll`: SHA-256 `c8e21c031e52c06a91c0b110b1d5d6f978112b395da7d05f029120d7b8219b42`.
  This matches the runtime LOADLIBRARY record in `trueos-baremetal.1.log` (line 892 of the capture inspected).
- `War3.exe`: SHA-256 `e3789bc11da75e68efb8ed832d9160a543fcb8815ad192fb98f0f3c8e70c8124`.
  Its two poll return sites match the capture's stack candidates. Do not substitute the older hashes in `tools/wc3-pe-preflight.txt`.

## Binary evidence

Rizin disassembly at Game.dll VA `0x6f0cba40`:

- `0x6f0cba4f`: calls `GetWindowLongA(hwnd, -21)`.
- Messages 1..15 dispatch through byte table `0x6f0cbc34` and address table `0x6f0cbc18`.
  Message 1 selects table index 0, target `0x6f0cba85`.
- `0x6f0cba85..0x6f0cba8e`: loads lParam, reads its first DWORD, calls
  `SetWindowLongA(hwnd, -21, *(u32*)lParam)`. The first DWORD of CREATESTRUCTA is lpCreateParams.
- That branch returns zero. It is the WM_CREATE branch, not WM_NCCREATE.
- WM_NCCREATE (0x81) and WM_NCCALCSIZE (0x83), with no attached userdata, reach
  `DefWindowProcA` at `0x6f0cbc06`.
- WM_PAINT uses BeginPaint/EndPaint without requiring userdata. Its prior successful return did not establish creation initialization.
- Other messages can forward through `0x6f862c00` only when both that pointer and userdata are nonzero (`0x6f0cbbe6..0x6f0cbbf7`).

The native regression test extracts this exact code and jump table from the supplied file,
checks its hash, and executes it as IA32 with narrow USER32 test doubles. Our production
Rust CREATESTRUCT encoder supplies the bytes. It verifies actual guest userdata attachment,
subsequent paint, return values, stack cleanup and nonvolatile register preservation.
It is not a whole-game or VMX test.

War3.exe disassembly:

- `0x00403e20..0x00403e31` is a wrapper around WaitForSingleObject; it returns the API's value unchanged.
- `0x0044459e` calls it with timeout 0 and the object at `0x0045f8e8`.
  At `0x004445a3`, result zero branches to `0x00444739`, which cleans up and returns from this engine routine.
  Forcing success here would leave the loop, not make it render.
- `0x004445f7` calls it with a timeout computed from a work item's deadline minus GetTickCount, clamped to zero.
  `0x004445fc` retains the result. With an item present and WAIT_TIMEOUT, `0x00444633` falls through into timed work;
  it does not wait indefinitely for the second event to become signaled.
- The wrapper's eight-word stack sample alone cannot show what each timed-work callback does.
  This audit does not claim that the renderer is executing or that creation is the only remaining omission.

Reproduce disassembly:

```sh
rizin -q -e scr.color=0 -c 'pd 200 @ 0x6f0cba40; px 48 @ 0x6f0cbc18' '/home/t4ce/Programmme/XPvm/tree/Warcraft III/Game.dll'
rizin -q -e scr.color=0 -c 'pd 25 @ 0x403e20; pd 260 @ 0x444530' '/home/t4ce/Programmme/XPvm/tree/Warcraft III/War3.exe'
```

## Implemented change

The child CreateWindowExA coordinator still allocates its HWND and opens the UI4 frame immediately.
It then runs WM_NCCREATE, WM_NCCALCSIZE (wParam=FALSE, RECT pointer), and WM_CREATE synchronously on the creating thread.
A 48-byte x86 CREATESTRUCTA and 16-byte RECT live above the callback frame on guest stack space,
retaining the original guest string pointers and lpCreateParams. The API's original stack and continuation
are restored only when the protocol completes. No host assignment links param to userdata.

DefWindowProcA now accepts WM_NCCREATE and provides default zero results for the modeled creation/destruction messages.
Unmodeled messages remain a frontier. SetWindowLongA supports the observed GWL_USERDATA index with process ownership
validation and returns the previous value. Other indices remain explicit frontiers.

Guest rejection returns NULL after destruction notifications and removal of the session window/paint state.
The callback mechanism retains its existing limitation on nested window callbacks; an attempted nested creation fails explicitly.
This change does not implement every Win32 activation, nonclient-layout or input behavior.
The always-visible UI4 policy and the zero-timeout safepoint remain unchanged.

Microsoft contract references:
[CreateWindowExA](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-createwindowexa),
[CREATESTRUCTA](https://learn.microsoft.com/en-us/windows/win32/api/winuser/ns-winuser-createstructa),
[WM_NCCREATE](https://learn.microsoft.com/en-us/windows/win32/winmsg/wm-nccreate),
[WM_CREATE](https://learn.microsoft.com/en-us/windows/win32/winmsg/wm-create).

## Validation and next run

```sh
cargo check -p wc3 --bin wc3 --offline
cargo test -p wc3 --lib --offline
python3 apps/wc3/tests/test_window_creation_native.py '/home/t4ce/Programmme/XPvm/tree/Warcraft III/Game.dll'
```

The host test harness supplies GPU ABI link seams that panic if called; these tests cannot claim to validate graphics hardware.
Several pre-existing test wiring issues (a wrong fixture name, a binary test in the session test module,
and outdated gamma/glFinish expectations) were corrected to make the suite runnable.

Expected runtime evidence: CREATEWINDOWEXA BEGIN with `creation_callbacks=synchronous`, callback returns for
0x81/0x83/1, SETWINDOWLONGA with `source=guest`, then CREATEWINDOWEXA RESULT with `creation_callbacks=delivered`
and the guest-established userdata. Reaching the main menu remains unverified until a new hardware run.
