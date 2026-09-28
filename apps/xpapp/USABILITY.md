# WC3 usability work

## Required outcome

Current user direction: keep `-opengl` and the accurate CPU renderer, expose a
1280x720 logical guest desktop, and use two shared P-core raster workers with
a 66% work budget. DirectX bring-up is deferred. The game binary and Gamma's
physical display mode setter remain unchanged. No separate guest program.
60 FPS (16.67 ms/frame) remains unmet.

## 720p guest display, 2026-09-28

`GAME_VIEWPORT_SIZE` is the single default/runtime source for the logical
1280x720 desktop. The physical UI4 output dimensions are queried only for the
startup diagnostic. `EnumDisplaySettingsA`, fullscreen mode validation through
`ChangeDisplaySettingsExA`, GDI resolution caps, and desktop client/window
rectangles use the logical size. Children inherit it. Window presentation and
GL drawable binding use the session's window size; guest `glViewport` calls
retain their existing semantics.

This reduces a 2560x1440 framebuffer from 3,686,400 pixels to 921,600 pixels.
It does not establish a fourfold end-to-end speedup: guest execution and other
costs also remain. The focused display change passed 395 library tests (one
ignored) and a non-test binary check before worker integration.

## Shared P-core raster pool, 2026-09-28

CPU draws can now use two persistent P-core workers on the ordinary executors.
Each worker measures its work against a 66% duty budget, yields after roughly
4 ms of bounded work, and accounts for actual timer wake time. Workers retain
their core and execution realm while active, with 250 ms of idle grace before
release. These are shared executor workers; guest thread execution remains
serialized. Startup records the actual worker slots and scalar fallback.

Each draw is split into two owned framebuffer row bands. Draw order, blend,
depth and scalar interpolation semantics are preserved within each band.
Workers process bounded row steps; the caller joins both before copying results
back. Texture and framebuffer copying still have a cost. This change targets
CPU raster time; it does not remove guest/provider round trips.

Validation: 396 XPApp library tests passed, two ignored; seven worker-budget
tests, seven lifetime tests and the native worker ABI contract check passed.
An independent 96-scene comparison covered 7,166,722 shaded pixels: the previous
scalar renderer, current scalar renderer and two concurrent host band workers
produced identical RGBA8 and exact depth bits. Evidence is in TRUEOS
`bld/xpapp-usability/pool-parity/parity.json`. Host parity does not validate the
live TRUEOS scheduler or establish WC3 frame rate.

The release XPApp build is published as
`fc6115a513ee41b7a08f0fbae5ee839a9111b9e7fcb95f9b9f6b3b980aab8765`.
The matching kernel image is built at TRUEOS `bld/trueos.iso`, with ELF artifacts
under `bld/artifacts/debug-xpapp-720p-pool/`. The new kernel must be booted before
launching this XPApp because it adds the resumable compute worker ABI. No reboot
or live validation was performed. Build and test logs are in
`/tmp/xpapp-720p-pool-*.log` on the development host.

## DirectX startup attempt, 2026-09-28

Outcome: the user supplied a run log confirming an actual D3D8 call, stopping
at `IDirect3D8::EnumAdapterModes` (provider 949, caller `0x6f0cc47d`). At the
user's subsequent request, `-opengl` is restored in the command line and CRT
argv (argc 5); CPU rendering is again selected. No D3D8 methods were added.
The restored OpenGL build passes 393 library tests (one ignored) and is published
as `1654354a79eb9fb75082882cdda9f4827f53ea0d662e403f938fbf0fed00b4ac`.
It has not been launched by this change; the currently running instance is unaffected.
The build/access notes below describe the initial attempt before that run.

The child command line and CRT argv now both contain
`"war3.exe" -nosound -swtnl -window` (argc 4). Startup records the requested
Direct3D8 path explicitly. The CPU OpenGL renderer remains available in source.
Blueprint commit `e9170548` is the preceding CPU launch/reference checkpoint.

393 library tests passed, one ignored. Release build and publication succeeded:
`184dbb96c142c3fdd274d78b5bb564aac76822c49766704cccbab6582eba37f8`.
This artifact has not been launched or observed: the rig refuses Shell2 TCP
4245 and logging TCP 1 while TRUEOSFS HTTP on port 80 still responds. No reboot
was issued. Build/test logs are `/tmp/xpapp-d3d-build.log` and
`/tmp/xpapp-d3d-tests.log` on the development host.

The current D3D8 provider can create the IDirect3D8 object, return its adapter
identifier, and release it. The remaining COM methods are observation thunks;
there is no IDirect3DDevice8 renderer. The next run must establish the actual
WC3 call sequence before extending these contracts. Removing the flag alone
does not provide a DirectX render loop.

The measured CPU frame below spends 6.222 s drawing and 3.097 s elsewhere.
Even perfect eight-core scaling of drawing alone gives about 3.875 s/frame;
the zero-cost raster limit is still 3.097 s/frame. CPU parallel rendering would
need disjoint framebuffer tiles with draw order preserved within each tile.
It cannot by itself solve the execution remainder.

One unmeasured execution lead remains in `asupersync.rs::run_loop`: every guest
exit checks the host clock for an 8 ms input deadline. Each due input pump calls
the pointer/keyboard queues and snapshots cursor routes through host APIs.
This needs bounded aggregate timing before attributing the remaining time or
changing input behavior. It is not evidence of a three-second input cost.

## CPU restoration, 2026-09-28

The production renderer is restored from `fc795aa55`, immediately before
`1a99d723` removed CPU rendering (`apps/wc3` before the xpapp rename).
Clear, indexed drawing, depth and readback use the original CPU rasterizer.
Swap uploads the completed RGBA frame in the original 256-row strips. UI4
producer ownership starts at swap/preview; CPU drawing retains its own buffer.
The native GPU scene implementation remains dormant. Text isolation is disabled.
No kernel rollback is required: its legacy sampled-texture presentation ABI
remains compatible.

Detailed per-exit and provider clock measurements now require `trace-execution`.
Ordinary carrier execution also avoids its four profiling timestamp reads.
Frame timings and sampled startup progress remain enabled. This removes known
measurement overhead; it does not establish that execution speed is fixed.
The user-requested critical-section bypass remains enabled.

Validation: 391 XPApp library tests passed, one ignored; 13 API library tests
passed. Binary checks passed with default features and `trace-execution`.
A production dispatch test covers CPU clear, draw and readback before any GPU
presentation. Fresh hardware capture confirms sky, logo, terrain, grass, water, shield and
menu frames have returned. Text remains broken; the captured modal dims the
scene. This is a visual restoration, not a pixel-identical animation comparison.

Published/running Blueprint:
`7df91093a561cb0a541efb01379b5526b44a6710d22fd3d9a7c4b198a5049deb`.
Startup confirms `renderer=cpu execution_timing=frame-only`. Warm frames are
about 11.7 seconds: 8.54 seconds in CPU draws, 0.17 seconds in upload/publication,
and 3.0 seconds outside those phases. Execution speed remains unresolved.
The next work is execution and CPU cost; text/menu follows, then Picasso GPU
scene rendering. GPU work is not part of this restoration.

Evidence in TRUEOS `bld/xpapp-usability/`: `cpu-restored.log`,
`cpu-restored.png`, `cpu-restored-summary.json`. The summary records the fresh
capture URL, artifact identity and measured warm frames.

## Removing bring-up overhead, 2026-09-28

Scope remains this WC3 process and the restored CPU scene.

- Execution diagnostics previously used six pthread mutex lock/unlock pairs
  per guest exit. A local atomic snapshot now retains the same sequence,
  stage, PID and TID without entering the pthread shim. Its gate covers only
  three atomic words, with no allocation, callback, await or host operation.
  Guest scheduling and the native carrier are unchanged.
- Provider-name construction now runs only with `trace-api`; ordinary logging
  previously allocated and discarded these tracing labels.
- The CPU sampler now avoids a second fetch when only one mip contributes.
  The real scene's costly terrain/grass draws use `LinearMipmapNearest`.
  Filtering, state and output pixels are preserved.
- Bounded `debug draws` captures now report decode and raster cost separately;
  neither phase includes diagnostic output time. Normal draws add no new clocks.

393 library tests pass, including coherent diagnostic snapshots under concurrent
readers/writers. The binary checks with default features and with API/execution
tracing. A differential render across 96 filter/wrap/scale combinations produced
identical bytes before and after the sampler change (SHA-256
`4d4b832cf66364e2f9cd61a4b6d8eef59b70d80998791bde7a1c5c6b273ebac2`).
The 14,745,600-pixel host mip benchmark's three-run median fell from 1.043 s to
0.871 s (16.5%). This fixture is not an end-to-end frame rate.

Published Blueprint:
`6b4b10659b6c5568fc7ac18a9005ff8b68a3c5d7fd20817937e99f06f08b9009`.
Hardware frame comparison (swaps 7–9, 166–167 draws, approximately 18 million
shaded pixels/frame) against the diagnostic-only build:

| Mean phase | Before sampler change | After |
| --- | ---: | ---: |
| CPU draw | 7064.7 ms | 6222.3 ms |
| Upload/publication | 162.3 ms | 162.3 ms |
| Outside draw/swap | 2936.3 ms | 2934.7 ms |
| Whole frame | 10163.3 ms | 9319.3 ms |

This is 11.9% less draw time and 8.3% less total frame time. Animation varies;
these are matching early warm-frame windows, not a deterministic replay. The
outside-draw cost remains unresolved, and the diagnostic storage change has
no isolated speedup claim. The game continued publishing more than 25 frames.
A fresh WD screenshot request remains busy without storing a new file, so
visual verification of this build is limited to the exact render comparison;
do not treat the earlier `cpu-restored.png` as a new capture.

Hardware evidence: `cpu-trim.log`, `cpu-sampler.log` and
`cpu-sampler-summary.json` in TRUEOS `bld/xpapp-usability/`.
The earlier diagnostic-only build was
`3337747070a83aaec1543263b75c42a509ab9fe522625c39867df2f9354897f4`.

Reproducible host fixture and results are in TRUEOS `bld/xpapp-usability/`:
`cpu-sampler-host-benchmark.rs`, `cpu-sampler-host-benchmark.json`,
`cpu-sampler-baseline-raster.rs` and `xpapp-raster-under-test.rs`. Compile the
harness with `rustc --edition=2024 -O`, then run with an output RGBA path. For
the original behavior, copy the baseline source to `xpapp-raster-under-test.rs`
in a separate temporary directory with the harness. The fixture covers 96
small renders and times a separate general-path mipmapped quad.

## Earlier text-only and critical-section bypass experiment

At the user's explicit request, `bypass-critical-sections` is currently enabled
in the default features. `EnterCriticalSection` and `LeaveCriticalSection`
provider thunks contain `ret 4` and no VMCALL. Both imported and dynamically
resolved provider entries use this selection. Initialization and the original
provider implementation remain available. This disables synchronization; it is
an experimental bypass, not a synchronization implementation. Remove the feature
from defaults and rebuild to restore normal calls. The thunk test passed with
and without the feature.

Earlier GPU experiment Blueprint:
`eb879549fe60f87f1f411b1ea97a3d4eeeb93a26180e9fb2426f4b22a9dca733`.
The running log confirms `critical_sections=bypassed enter_leave=guest-ret4`.
The loading frame interval fell from about 208 seconds to 58.787 seconds.

`debug isolate 2 3 38` was active on that run: retain texture 38 (the observed
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

## Retained GPU experiment implementation

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
