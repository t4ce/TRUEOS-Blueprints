# WC3 usability work

## Required outcome

Readable existing menu text and approximately 30 FPS (33.3 ms/frame), using the
existing textures, geometry and draw count. Completion requires hardware timing
and a fresh screenshot. Neither requirement is met yet.

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

Current internally published stable Blueprint:
`4c928a19d683ee31cf4c9aa8fbd491dc9d5ef8be6377dbb8b86a3387b5cd8648`.
The kernel timing change is checked locally and has not been deployed. A newer
local Blueprint build adds CPU atlas caching; it is not yet published.

## Hardware evidence

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

Restore rig access, deploy the matched kernel timing build, launch the stable
Blueprint, capture steady frame timings and bounded debug draws for the text.
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
