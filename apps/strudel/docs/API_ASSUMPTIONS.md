# API assumptions used by the source

- The app is built with `trueos-qjs` feature `trueos`.
- The public Blueprint SDK is available as crate `trueos`.
- `trueos::audio` is the re-export of `v::vaudio`.
- Playback format is fixed to S16LE, two channels, 48,000 Hz.
- `Stream::write_interleaved_i16` returns frames, not samples.
- `ERR_BUSY` means retry after yielding/polling.
- `Workbench::eval(..., EvalMode::Script)` serializes arrays as JSON in `EvalResult.text`.
- Workbench output remains under its 128 KiB cap; a 50 ms pattern query should be far below it.
- The generated upstream file is an IIFE and installs `globalThis.StrudelCore`.
- The adapter only consumes discrete haps with a usable `whole` or `part` span.
