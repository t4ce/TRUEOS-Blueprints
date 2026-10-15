# Allocation contract and fragmentation probe

This probe uses the native Rust allocator and the imported C allocation ABI.
It makes real allocations, preserves initialized witnesses through realloc,
checks alignments through 64 KiB, checks zeroed Rust/C allocations, and checks
`malloc_usable_size` on live C blocks. It never intentionally passes an invalid
pointer, double frees, reads uninitialized usable tail space, or logs pointers
or allocation contents.

The performance cases allocate 3,000, 10,000 and 50,000 small live Rust blocks,
sort their addresses without allocating, and free alternating addresses. The
largest case creates approximately 25,000 separate free fragments in a region
larger than 10 MiB. Each address quartile then gets 256 replacement/free pairs,
followed by 256 reallocations and deterministic randomized final frees. Every
block is checked before release. Explicitly owned blocks and RAII cleanup keep
valid allocation ownership on failure paths.

`BEGIN`, `PASS`, `PROGRESS`, `LIMIT`, and `FAIL` records contain fixed case names,
block/operation counts and elapsed times. `max_batch_us` measures at most 256
operations plus witness checks and the two batch clock reads; it is not a
single allocation latency. The phase total includes progress logging overhead.
No performance threshold is presented as an allocator correctness contract.

The normal run has a cooperative 15-second phase budget, checked between
batches. It stops before a larger level if the previous level took over two
seconds. A `LIMIT` exit is incomplete coverage, never a full pass. A single
native allocation/free call cannot be interrupted, and valid outstanding
allocations must still be released after a limit, so these checks cannot
replace an external watchdog on a kernel with a stuck allocator.
If a phase stops early, the mandatory `cleanup` phase logs the remaining valid
frees and their progress; it has no cancellation point that would leak them.

Host baseline (Linux's allocator, not TRUEOS's):

```sh
cargo run --release --manifest-path probes/alloc_probe/Cargo.toml
```

Build without publication:

```sh
TRUEOS_BLUEPRINT_SKIP_APPS_PUBLISH=1 cargo bpp alloc_probe
```

Native artifact: `dist/alloc_probe.bp`. It is registered only in `probes.json`.
When authorized to publish it to the existing Apps folder, use:

```sh
sh probes/alloc_probe/build_publish.sh
```

Then launch `§§alloc_probe` in Shell3. The short name fits the fast-spawn
selector. No disk files, startup files, account state or network are used by
the probe itself. The Appstore loader can still use network to obtain it.

A loader that supports arguments can select a single comparable level with
`--blocks=50000 --phase-ms=120000`. Block counts are limited to 16–50,000;
phase budgets to 1–120,000 milliseconds. The explicit block count bypasses
only admission of the next level, not the cooperative phase budget.

A Linux pass establishes the witness, ownership and probe control paths. A
TRUEOS pass needs the exact probe hash, kernel runtime hash, all requested
levels, and a final `PASS case=all` record from that native run. Host timings
and native timings are not interchangeable.

The [Linux baseline](validation/linux.json) passed all default levels and odd
block counts 17, 31, 33 and 1,025. Its [budget-path receipt](validation/linux-limit.log)
records an incomplete allocation phase, complete release of its valid live
blocks, and `LIMIT` rather than a full pass. A separate
[AddressSanitizer and leak-detection run](validation/linux-asan.json) passed the
full default geometry without sanitizer diagnostics. The
[native build receipt](validation/native-build.json) records the guarded
package identity; it does not claim a native execution pass.
