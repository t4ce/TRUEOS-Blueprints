# Bare-metal verification checklist

1. Boot the app with the placeholder bundle. Confirm the fallback smoke result and audible demo.
2. Log `buffer_frames()` and observe `queued_frames()` while running for at least ten minutes.
3. Verify that `ERR_BUSY` retries do not starve `Workbench::poll()` or the Blueprint scheduler.
4. Generate the upstream bundle and compare its startup memory/time against the fallback.
5. Confirm Fraction.js BigInt operations in the exact TRUEOS QuickJS revision:
   - construction;
   - add/sub/mul/div;
   - `Number(fraction)` / `valueOf`;
   - long-running `queryArc`.
6. Query arcs around exact cycle boundaries and check for missing or duplicated PCM onsets.
7. Tune `BLOCK_FRAMES` and queue target. Suggested sweep: 480, 960, 2,400, 4,800 frames.
8. Confirm HDA output on at least the controller/codec combinations TRUEOS currently supports.
9. Decide shutdown behavior. The proof loops forever; an interactive app should call stream drop/close.
10. Before distributing an image with generated Strudel code, include corresponding source and AGPL notices.

## Later, not required for first sound

- persistent voice state for filters and sample playback;
- parameter patterns beyond note/velocity/wave/pan;
- mini notation and transpiler;
- editor/UI;
- hot pattern replacement;
- native QuickJS `trueos:audio` module;
- timestamped audio submission instead of block lookahead.
