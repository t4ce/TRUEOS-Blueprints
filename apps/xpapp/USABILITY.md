# WC3 usability work

## Required outcome

Current user direction: keep only WC3's existing text and target 60 FPS
(16.67 ms/frame). No separate guest test program. Completion requires hardware
timing and a fresh screenshot; 60 FPS is not met.

## Text-only and critical-section bypass experiment

At the user's explicit request, `bypass-critical-sections` is currently enabled
in the default features. `EnterCriticalSection` and `LeaveCriticalSection`
provider thunks contain `ret 4` and no VMCALL. Both imported and dynamically
resolved provider entries use this selection. Initialization and the original
provider implementation remain available. This disables synchronization; it is
an experimental bypass, not a synchronization implementation. Remove the feature
from defaults and rebuild to restore normal calls. The thunk test passed with
and without the feature.

Deployed Blueprint:
`eb879549fe60f87f1f411b1ea97a3d4eeeb93a26180e9fb2426f4b22a9dca733`.
The running log confirms `critical_sections=bypassed enter_leave=guest-ret4`.
The loading frame interval fell from about 208 seconds to 58.787 seconds.

`debug isolate 2 3 38` is active on this run: retain texture 38 (the observed
font texture), skip other draws before array decoding, clear to gray. Texture
IDs vary between runs; identify the font from a bounded draw capture first.
`debug isolate 2 3 restore` restores all draws. This removes the other rendered
elements; WC3's guest execution and state calls still run.

Text-only frames contain 16 draws / 484 triangles, skipping about 150 scene
draws. Representative completed frame: 3627 ms, text work 166 ms (decode 71,
acquire 24, submit 69), outside draw/swap 3461 ms. Clear is 372 ms **within**
the outside total. This is about 0.28 FPS, not 60 FPS. The capture confirms only
game text on gray; desktop overlays remain independently composed. Artifacts:
TRUEOS `bld/xpapp-usability/text-only-no-critical.log` and
`text-only-no-critical.png`. The earlier text-filter build passed 389 library
tests; both feature configurations passed the new bypass thunk test.

## Current implementation

- CPU mip atlases are retained in a bounded 64-entry / 16 MiB cache; texture
  edits and deletion invalidate the corresponding entry. Repeated draws reuse
  the same immutable allocation instead of rebuilding padded mip rows.
- FixedRenderer retains exact-content texture buffers, bounded to 64 entries /
  16 MiB of source pixels. Dimensions and bytes must match. Avoiding redundant
  writes preserves the kernel's immutable resident sampled texture cache.
- Normal logging does not disable the state-only GL provider fast path; explicit
  `trace-api` retains diagnostics. Clear/draw/swap retain frame coordination.
- Critical-section enter/leave publish the three adjacent state words with one
  guest-memory write. Enter's diagnostic readback and leave's diagnostic
  pre-read run only with `trace-api`; ownership and waiter checks are preserved.
- Frame counters report texture hits/uploads/bytes. Provider timings name the
  slowest operation. Structured diagnostic records reach the host logger.
- `debug draws 2 3 256` additionally reports packed vertex color ranges, first
  vertex, texture, lighting, fog, alpha/blend and depth state. It does not change
  the submitted draw. Use this to inspect the existing text passes.
- TRUEOS `src/gpu/vgpu.rs` now aggregates 128 successful fixed draws into
  `XPAPP GPU TIME`: prepare, mesh, render, retirement and GPU polling durations.
  Polling is included in render time; all phases are inside Blueprint submit
  wall time. Clear draws are not in this fixed-shader aggregate.

The kernel timing change and CPU atlas cache are now deployed; see the recovered
rig evidence below for artifact identities and the current diagnostic build.

## Hardware evidence

### Recovered rig, 2026-09-28

The user recovered the BIOS boot and rebuilt both repositories (TRUEOS
`16d474232`, Blueprints `3f7f3be2`). Fresh PXE reads, artifact hashes and a new
boot marker verified the diagnostic kernel. Blueprint `b81c706f26821b80...`
loaded WC3 in about 208 seconds. Thirteen warm frames averaged 6491 ms
(6398–6787 ms); warm texture uploads were zero. The new screenshot still has
unreadable menu text. Evidence: TRUEOS `bld/xpapp-usability/recovered-run.log`
and `recovered-run.png`.

Representative warm frame: decode 603 ms, atlas 3 ms, acquire 296 ms,
submit 1860 ms, explicit wait 1 ms, outside draw/swap 3696 ms. The new kernel
aggregate attributes most fixed-draw renderer time to completion polling;
several 128-draw windows contain a single approximately 1.04-second draw.
Polling measures elapsed CPU waiting, not isolated GPU execution time.

Bounded text diagnostics show black shadow colors followed by gold/white
foreground colors. The first foreground UV is (0,0), while its shadow has
glyph coordinates. This is a lead, not a confirmed cause: the initial marker
did not record UV-array enablement or the first guest index. A diagnostic-only
Blueprint `667a2a9d8e41c1c9b0b2a6fd01e4752a197feddf4fe67c79a03c473341ca31a6`
adds those fields and per-captured-draw costs; it was published and launched
without rebooting the OS.

The second bounded capture completed (`recovered-draw-debug.log`). Foreground
font draws have `uv_array=false`, first index 0; adjacent shadow draws have
`uv_array=true`, also first index 0, with the same UV pointer. For example,
seq 598/599 use texture 39 and 42 indices. The bridge therefore substitutes
(0,0,0,1) for foreground UVs. Why the enable state changes is still unresolved;
do not force array enablement globally or treat this as a verified fix.
The repeated slow submission is texture 27, 4701 indices / 3105 vertices:
seq 504 took 1061 ms and seq 670 took 1078 ms. Texture 1 / 8103 indices took
159 ms in both captured frames. This diagnostic build changes no draw state.

Baseline: TRUEOS `abcb4ae0f`, Blueprints `a3ff1f35`. Warm menu frames approximately
6.49 seconds, 168–169 draws. Representative frame: 614 ms decode, 50 ms atlas,
123 ms acquire, 1969 ms submit, 3 ms explicit wait, 3725 ms outside draw/swap.
Submission already waits internally; the explicit wait is not total GPU time.
One 2-second main-thread window recorded 507 ms request/reply and 30 ms native
execution. VMCS setup/cleanup averaged about 10 us per entry. Different windows
overlap and must not be summed into one frame total.

Texture-cache comparison (`e7fd4de6...`): all 166–167 warm draws hit the cache,
zero texture uploads, but frames remain about 6–6.5 seconds. A fresh screenshot
still shows dark unreadable text. Removing uploads alone is insufficient.

Bulk-read comparison (`ffa5b899...`): native read attempts up to 64 KiB with page
fallback did not materially change decode or frame time (about 611 ms decode,
6.4–6.6 seconds/frame). Named provider timings identify `GlClear` at about
370 ms and `GlDrawElements` up to 1.1 seconds. The bulk-read experiment was
removed from the working implementation.

Native coordinator comparison (`43e1321d...`): coordinator on a normal native
worker, guest entry retained on a separate carrier, no kernel/VMCS changes.
Guest execution advanced, but the rig then became unreachable near first GPU
setup. No usable frame timings were obtained. Native stdout was also absent
from the host capture. This execution change was removed; the stable artifact
uses the original Hull coordinator. The failure cause is not established.

Artifacts in TRUEOS `bld/xpapp-usability/`:
`baseline.log`, `cache-run.log`, `cache-run.png`, `bulk-run.log`,
`native-coordinator-run.log`, and the retired experiment patch/test.

## Validation and recovery

389 XP library tests passed, one ignored. Recording-device texture-cache test
passed. Binary checks with and without `trace-api`, native release Blueprint
builds, and kernel `cargo check --features wc3 --offline` passed.
These do not prove FPS or readable fonts.

After preserving the native-run capture and restoring the stable publication,
recovery was requested using the existing kernel image (SHA256
`96aa4b0c319d1fe804ef3be22f8d629e0570d527ef09e5867be8c28634352dec`).
The ESP32 reset controller acknowledged the physical button press. The helper
timed out after 240 seconds without fresh PXE reads; inspect `/tmp/xpapp-recovery-reset.log` and
TRUEOS `bld/testrig-physical-reset-receipt.json` before further rig operations.
That verification is terminal. A local kernel/ISO build with deployment disabled
is preparing the GPU timing change; see `/tmp/xpapp-timing-iso-build.log`.
The user was asked whether the rig is powered on and what its screen shows.

## Next evidence needed

Rig access and diagnostic deployment are verified. Correlate individual slow
draws with their texture/geometry and resolve the foreground UV discrepancy.
Every current draw imports/maps/unmaps the complete drawable, creates/releases
a resident mesh, and submits a synchronous scene. Use the new timings to choose
which of these costs to remove; do not infer GPU time from the separate wait.

The local ISO build completed with deployment and SMB publication disabled.
Packaged kernel marker `XPAPP GPU TIME` was verified in the ELF.
Kernel SHA256: `4f0bc30b940c11b828c4bfe03a8ee607f5eb5ebc4e268a52e8a7a11de8ddcb68`.
ISO SHA256: `dcd4bb79e866c9d12b7699fb3808ff27a54b4a620f2ebac1f9bcb8a8d6b887bb`.
These artifacts are staged for the next boot, not hardware-verified.
A new log collector was started after the failed reset verification (run ID
`9006a12e3491b53a627812276f41e691`, initial PID 295635, slot 1). Revalidate its
process and new log contents; the LatestOfThree link alone is not boot evidence.
