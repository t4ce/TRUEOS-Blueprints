# Quiet release execution

Normal WC3 builds have no default trace features. The Blueprint metadata selects
the release profile explicitly; the packer already defaulted to release before
this change. This is a runtime-overhead change, not a debug-to-release claim.

Routine application logs are gated before their argument expressions execute.
Idle provenance collection and diagnostic guest reads are omitted as well.
Fatal errors and terminal provider frontiers remain visible. Failed execution
captures stopped registers, 64 code bytes and 128 stack bytes where readable.
Existing framework crash-file collection remains separate. Explicit `debug`
command replies remain available, including with `nolog`.

## Execution changes

### Kernel spin/yield policy

The kernel previously requested PAUSE exiting for every Hull and then slept
for 1 ms on each intercepted instruction. Rust `spin_loop` in synchronization
and completion waits emits PAUSE, so this imposed timed sleeps on short waits
even with application logging disabled. The kernel now lets PAUSE execute
natively when its VMX preemption timer is available. Without that timer it keeps
the interception fallback. `allcaps::hv::VMX_NATIVE_PAUSE_WITH_TIMER=false`
restores PAUSE interception for comparison.

Explicit host yield and zero-duration sleep now suspend for one executor turn
without a minimum 1-ms sleep. Positive sleeps keep their duration. Hull timer
exits also yield an executor turn for bounded same-AP fairness; the existing
125-ms Hull timer, 500-ms x86 slice and host stop checks remain in place. XP
zero-timeout wait results and scheduling safepoints are unchanged.

These changes require a **kernel rebuild and boot**, not just a Blueprint
repack. The existing kernel VMCS-control receipt includes actual `pause_exit`
and `timer` values; the expected timer-backed path is `pause_exit=0 timer=1`.
Host policy tests and a kernel check validate the code, not the hardware speedup.
The next run must establish the combined effect. Native worker submission and
fresh VMCS setup still occur for each x86 slice.

### Blueprint path

- The coordinator owns stopped x86 contexts directly. `run` and `resume` still
  await the existing native carrier, without an additional Tokio actor and
  request/response channels for every exit. Extended/debug state is read on
  demand for consumers such as SEH, `_ftol` and checkpoints. Pending state writes
  are applied before the next execution permit.
- Quiet ordinary GL setters/queries dispatch directly through their existing
  typed handlers. Import operation classifications are cached. Errors still
  reach the original failure path without executing the provider twice.
- `_stricmp`, `isdigit` and `_ismbcspace` use guest-native x86 helpers matching
  the existing Rust personality: ASCII case folding, digit bit 4, whitespace
  bit 8. These join the existing native memmove/string/decimal helpers.
- Routine input pumping is limited to an 8-ms interval; message/cursor queries
  pump immediately. With no host frames, HID enumeration is skipped.
- The carrier self-test and first-draw preview are opt-in. Actual context state
  handling remains enabled. Normal GPU publication follows guest swaps.

## Rendering changes

Client arrays use bounded per-draw snapshots. Overlapping interleaved attribute
ranges merge into one source read; sparse or unreadable bulk ranges fall back to
the original attribute reads. A snapshot never survives a draw. Dense index
remapping uses a vector instead of hashing every index.

The rasterizer steps edge/interpolation planes across rows, shares perspective
reciprocals, specializes constant-Q texture sampling and simple opaque Replace,
and avoids unnecessary mip/LOD work. Power-of-two repeat wrapping uses masks.
Clip outcodes accept or reject whole triangles before allocating clipped
polygons; crossing triangles keep the full clipper. Clears fill intersected
rows directly. Vertex lighting avoids a duplicate square root and unused
specular work.

Presentation reuses strip-upload memory and omits the diagnostic full-frame
nonblack scan. It retains the proven 256-row GPU strip path: a full-size upload
plus resident sampler copy would exceed the current 32-MiB device quota at
2560x1440. GPU completion, surface leases and UI4 publication remain ordered.

## Measurements and comparison switches

The host release raster fixture processes 22.1 million pixels at 1280x720:
24.6 to 36.9 Mpixels/s, approximately **1.5x** (0.899 to 0.599 seconds).
This is raster throughput, not whole-game FPS. A mixed-render differential
against pre-change `a518c1a0` compared 16,384 RGBA bytes: two changed by one LSB,
none by more, and alpha coverage matched. Incremental floating-point arithmetic
can round differently from the prior barycentric implementation.

The 1,023-vertex interleaved fixture reduces 3,069 high-level attribute reads to
one contiguous source read. The underlying guest-memory ABI still transfers
bounded page chunks; this is neither one hardware crossing nor a 3,069x timing
claim. Mutation between draws and bulk-read failure fallback are tested.

Native IA32 harnesses cover 1,044 classification cases and ten string/ABI cases.
Host tests do not execute the TRUEOS carrier or validate physical scanout.
Combined startup and frame-rate gains require the next hardware run; no 1000x
claim follows from these component measurements. Fixed-function shading still
runs on the CPU, with GPU publication of the completed image.

| Feature | Comparison behavior |
| --- | --- |
| `actor-execution` | Restore the previous coordinator actor/channel path. |
| `replay-arrays` | Restore individual vertex attribute reads. |
| `host-stricmp`, `host-isdigit`, `host-ismbcspace` | Restore each Rust CRT provider. |
| `carrier-selftest` | Run startup x86 extended-state self-tests. |
| `preview-first-draw` | Publish the first draw before a guest swap. |
| `diagnostics`, `trace-all` | Restore general or detailed tracing. |

Use `debug perf` once for a baseline, then again after an interval. It reports
draw and guest-swap deltas, guest swaps per second, execution exits and the
direct/actor transport. This uses existing counters and performs no periodic
logging. Guest swaps are not physical scanout FPS; counter deltas also lose
history if their owning GL context is destroyed between samples.
